use std::path::Path;

use anyhow::{Context, Result};
use aya::{
    Ebpf, EbpfLoader,
    maps::{HashMap, RingBuf},
    programs::TracePoint,
};

use crate::{kernel_event, syscall::Architecture};

pub struct Observer {
    _ebpf: Ebpf,
    events: RingBuf<aya::maps::MapData>,
}

impl core::fmt::Debug for Observer {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.debug_struct("Observer").finish_non_exhaustive()
    }
}

impl Observer {
    pub fn load(path: &Path, syscall_names: &[String], architecture: Architecture) -> Result<Self> {
        let mut ebpf = EbpfLoader::new()
            .load_file(path)
            .context("load eBPF object")?;
        attach(&mut ebpf, "okoscope_exec", "sched", "sched_process_exec")?;
        attach(&mut ebpf, "okoscope_sys_enter", "raw_syscalls", "sys_enter")?;
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
        Ok(Self {
            _ebpf: ebpf,
            events,
        })
    }

    pub fn next(&mut self) -> Result<Option<agent_ebpf_common::KernelEvent>> {
        self.events
            .next()
            .map(|item| kernel_event::decode(&item).map_err(Into::into))
            .transpose()
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
