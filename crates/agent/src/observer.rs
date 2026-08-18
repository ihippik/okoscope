use std::path::Path;

use anyhow::{Context, Result};
use aya::{
    Ebpf, EbpfLoader,
    maps::{HashMap, PerCpuArray, RingBuf},
    programs::TracePoint,
};

use crate::{counters::NetworkKernelCounters, kernel_event, syscall::Architecture};

pub struct Observer {
    _ebpf: Ebpf,
    events: RingBuf<aya::maps::MapData>,
    network_counters: PerCpuArray<aya::maps::MapData, u64>,
}

impl core::fmt::Debug for Observer {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.debug_struct("Observer").finish_non_exhaustive()
    }
}

impl Observer {
    pub fn load(
        path: &Path,
        syscall_names: &[String],
        network_connect: bool,
        architecture: Architecture,
    ) -> Result<Self> {
        let mut ebpf = EbpfLoader::new()
            .load_file(path)
            .context("load eBPF object")?;
        attach(&mut ebpf, "okoscope_exec", "sched", "sched_process_exec")?;
        attach(&mut ebpf, "okoscope_sys_enter", "raw_syscalls", "sys_enter")?;
        if network_connect {
            attach(
                &mut ebpf,
                "okoscope_connect_enter",
                "syscalls",
                "sys_enter_connect",
            )?;
            attach(
                &mut ebpf,
                "okoscope_connect_exit",
                "syscalls",
                "sys_exit_connect",
            )?;
        }
        {
            let map = ebpf
                .map_mut("SYSCALL_ALLOWLIST")
                .context("missing SYSCALL_ALLOWLIST map")?;
            let mut allowlist: HashMap<_, u32, u8> = HashMap::try_from(map)?;
            for name in syscall_names {
                let number = crate::syscall::resolve(name, architecture)?;
                allowlist.insert(number, 1, 0)?;
            }
        }
        let map = ebpf
            .take_map("EVENTS")
            .context("missing EVENTS ring buffer")?;
        let events = RingBuf::try_from(map)?;
        let map = ebpf
            .take_map("NETWORK_COUNTERS")
            .context("missing NETWORK_COUNTERS map")?;
        let network_counters = PerCpuArray::try_from(map)?;
        Ok(Self {
            _ebpf: ebpf,
            events,
            network_counters,
        })
    }

    pub fn next_event(&mut self) -> Result<Option<agent_ebpf_common::KernelEvent>> {
        self.events
            .next()
            .map(|item| kernel_event::decode(&item).map_err(Into::into))
            .transpose()
    }

    pub fn network_counters(&self) -> Result<NetworkKernelCounters> {
        let total = |index: u32| -> Result<u64> {
            Ok(self.network_counters.get(&index, 0)?.iter().copied().sum())
        };
        Ok(NetworkKernelCounters {
            correlation_capacity: total(agent_ebpf_common::NETWORK_COUNTER_CAPACITY)?,
            correlation_miss: total(agent_ebpf_common::NETWORK_COUNTER_CORRELATION_MISS)?,
            decode_failed: total(agent_ebpf_common::NETWORK_COUNTER_DECODE_FAILED)?,
            unsupported_family: total(agent_ebpf_common::NETWORK_COUNTER_UNSUPPORTED_FAMILY)?,
            kernel_lost: total(agent_ebpf_common::NETWORK_COUNTER_KERNEL_LOST)?,
        })
    }
}

fn attach(ebpf: &mut Ebpf, program_name: &str, category: &str, event: &str) -> Result<()> {
    let program: &mut TracePoint = ebpf
        .program_mut(program_name)
        .with_context(|| format!("missing {program_name} program"))?
        .try_into()?;
    program
        .load()
        .with_context(|| format!("load {program_name}"))?;
    program
        .attach(category, event)
        .with_context(|| format!("attach {program_name} to {category}/{event}"))?;
    Ok(())
}
