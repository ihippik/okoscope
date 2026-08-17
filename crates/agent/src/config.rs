use std::collections::BTreeMap;

use serde::Deserialize;
use thiserror::Error;
use uuid::Uuid;

use crate::syscall::{self, Architecture};

pub const API_VERSION: &str = "okoscope.io/v1alpha1";
pub const KIND: &str = "AgentConfiguration";

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AgentConfig {
    pub api_version: String,
    pub kind: String,
    pub server: ServerConfig,
    pub identity: IdentityConfig,
    pub scope: ScopeConfig,
    pub observation: ObservationConfig,
    #[serde(default)]
    pub safety: SafetyLimits,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ServerConfig {
    pub endpoint: String,
    pub credential_file: String,
    #[serde(default)]
    pub development_plaintext: bool,
    pub ca_file: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct IdentityConfig {
    pub node_name: String,
    pub cluster_name: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ScopeConfig {
    pub workloads: Vec<WorkloadSelector>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkloadSelector {
    pub project_id: Uuid,
    pub application_id: Uuid,
    pub namespace: String,
    pub kind: String,
    pub name: String,
    pub release: Option<String>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}

impl WorkloadSelector {
    #[must_use]
    pub fn matches(&self, workload: &WorkloadMetadata) -> bool {
        self.namespace == workload.namespace
            && self.kind == workload.kind
            && self.name == workload.name
            && self
                .labels
                .iter()
                .all(|(key, value)| workload.labels.get(key) == Some(value))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkloadMetadata {
    pub namespace: String,
    pub kind: String,
    pub name: String,
    pub labels: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ObservationConfig {
    pub process_exec: bool,
    #[serde(default)]
    pub syscalls: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct SafetyLimits {
    pub queue_capacity: usize,
    pub batch_size: usize,
    pub max_events_per_second: u32,
}

impl Default for SafetyLimits {
    fn default() -> Self {
        Self {
            queue_capacity: 4096,
            batch_size: 256,
            max_events_per_second: 10_000,
        }
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("invalid YAML: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("unsupported apiVersion {0:?}")]
    ApiVersion(String),
    #[error("unsupported kind {0:?}")]
    Kind(String),
    #[error("at least one workload selector is required")]
    MissingWorkload,
    #[error("at least one observation capability is required")]
    MissingObservation,
    #[error("invalid workload selector: {0}")]
    InvalidSelector(String),
    #[error("TLS is required unless developmentPlaintext is explicitly enabled")]
    TlsRequired,
    #[error(transparent)]
    Syscall(#[from] syscall::SyscallError),
}

impl AgentConfig {
    pub fn from_yaml(input: &str, architecture: Architecture) -> Result<Self, ConfigError> {
        let mut config: Self = serde_yaml::from_str(input)?;
        for selector in &mut config.scope.workloads {
            if let Some(release) = &mut selector.release {
                *release = release.trim().to_owned();
            }
        }
        config.validate(architecture)?;
        Ok(config)
    }

    pub fn validate(&self, architecture: Architecture) -> Result<(), ConfigError> {
        if self.api_version != API_VERSION {
            return Err(ConfigError::ApiVersion(self.api_version.clone()));
        }
        if self.kind != KIND {
            return Err(ConfigError::Kind(self.kind.clone()));
        }
        if self.scope.workloads.is_empty() {
            return Err(ConfigError::MissingWorkload);
        }
        if !self.observation.process_exec && self.observation.syscalls.is_empty() {
            return Err(ConfigError::MissingObservation);
        }
        if !self.server.development_plaintext
            && self
                .server
                .ca_file
                .as_deref()
                .unwrap_or_default()
                .is_empty()
        {
            return Err(ConfigError::TlsRequired);
        }
        if self.safety.queue_capacity == 0
            || self.safety.batch_size == 0
            || self.safety.batch_size > self.safety.queue_capacity
        {
            return Err(ConfigError::InvalidSelector(
                "safety limits must be non-zero and batchSize must not exceed queueCapacity".into(),
            ));
        }
        for selector in &self.scope.workloads {
            if selector.namespace.is_empty() || selector.kind.is_empty() || selector.name.is_empty()
            {
                return Err(ConfigError::InvalidSelector(
                    "namespace, kind, and name are required".into(),
                ));
            }
            if let Some(release) = &selector.release
                && (release.is_empty() || release.len() > 200)
            {
                return Err(ConfigError::InvalidSelector(
                    "release must be trimmed and contain 1..=200 bytes".into(),
                ));
            }
        }
        for name in &self.observation.syscalls {
            syscall::resolve(name, architecture)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"
apiVersion: okoscope.io/v1alpha1
kind: AgentConfiguration
server:
  endpoint: http://server:4317
  credentialFile: /secrets/credential
  developmentPlaintext: true
identity:
  nodeName: node-1
  clusterName: local
scope:
  workloads:
    - projectId: 018f4f9c-3f9a-7de1-8000-000000000001
      applicationId: 018f4f9c-3f9a-7de1-8000-000000000002
      namespace: production
      kind: Deployment
      name: payment-api
      labels:
        app.kubernetes.io/name: payment-api
observation:
  processExec: true
  syscalls: [ptrace, setns]
"#;

    #[test]
    fn parses_strict_valid_configuration() {
        let config = AgentConfig::from_yaml(VALID, Architecture::X86_64).unwrap();
        assert_eq!(config.scope.workloads.len(), 1);
    }

    #[test]
    fn release_is_optional_bounded_and_trimmed() {
        let with_release = VALID.replace(
            "      name: payment-api",
            "      name: payment-api\n      release: 1.7.2",
        );
        let config = AgentConfig::from_yaml(&with_release, Architecture::X86_64).unwrap();
        assert_eq!(config.scope.workloads[0].release.as_deref(), Some("1.7.2"));
        let normalized = AgentConfig::from_yaml(
            &with_release.replace("1.7.2", "\" 1.7.2 \""),
            Architecture::X86_64,
        )
        .unwrap();
        assert_eq!(
            normalized.scope.workloads[0].release.as_deref(),
            Some("1.7.2")
        );
        assert!(
            AgentConfig::from_yaml(
                &with_release.replace("1.7.2", &"x".repeat(201)),
                Architecture::X86_64
            )
            .is_err()
        );
    }

    #[test]
    fn rejects_unknown_fields_and_syscalls() {
        assert!(
            AgentConfig::from_yaml(
                &VALID.replace("processExec: true", "processExec: true\n  typo: true"),
                Architecture::X86_64
            )
            .is_err()
        );
        assert!(
            AgentConfig::from_yaml(&VALID.replace("ptrace", "101"), Architecture::X86_64).is_err()
        );
    }

    #[test]
    fn labels_are_and_and_selectors_can_be_or() {
        let selector = &AgentConfig::from_yaml(VALID, Architecture::X86_64)
            .unwrap()
            .scope
            .workloads[0];
        let mut labels = BTreeMap::new();
        labels.insert("app.kubernetes.io/name".into(), "payment-api".into());
        assert!(selector.matches(&WorkloadMetadata {
            namespace: "production".into(),
            kind: "Deployment".into(),
            name: "payment-api".into(),
            labels
        }));
    }
}
