use std::{
    collections::BTreeMap,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

use dashmap::DashMap;
use event_model::KubernetesAttribution;
use futures::{StreamExt, TryStreamExt};
use k8s_openapi::api::{
    apps::v1::{Deployment, ReplicaSet},
    core::v1::Pod,
};
use kube::{
    Api, Client, Resource, ResourceExt,
    runtime::watcher::{self, Event},
};
use thiserror::Error;
use tokio::time::Instant;

use crate::config::{WorkloadMetadata, WorkloadSelector};
use crate::counters::Counters;

#[derive(Clone, Debug)]
struct Owner {
    uid: String,
    kind: String,
    name: String,
}

#[derive(Clone, Debug)]
struct Controller {
    uid: String,
    owner: Option<Owner>,
    labels: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
struct ContainerRecord {
    pod_uid: String,
    pod_name: String,
    namespace: String,
    container_name: String,
    owner: Owner,
    expires_at: Option<Instant>,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum AttributionError {
    #[error("container is not present in the attribution cache")]
    UnknownContainer,
    #[error("pod owner chain is incomplete")]
    IncompleteOwnerChain,
    #[error("resolved workload does not match configured scope")]
    NotSelected,
}

#[derive(Debug)]
pub struct AttributionCache {
    containers: DashMap<String, ContainerRecord>,
    replica_sets: DashMap<(String, String), Controller>,
    deployments: DashMap<(String, String), Controller>,
    terminated_ttl: Duration,
}

impl AttributionCache {
    #[must_use]
    pub fn new(terminated_ttl: Duration) -> Self {
        Self {
            containers: DashMap::new(),
            replica_sets: DashMap::new(),
            deployments: DashMap::new(),
            terminated_ttl,
        }
    }

    pub fn apply_pod(&self, pod: &Pod) {
        let Some(uid) = pod.meta().uid.clone() else {
            return;
        };
        let Some(namespace) = pod.namespace() else {
            return;
        };
        let Some(owner) = controller_owner(&pod.metadata.owner_references) else {
            return;
        };
        let statuses = pod
            .status
            .as_ref()
            .and_then(|status| status.container_statuses.as_ref());
        for status in statuses.into_iter().flatten() {
            let Some(container_id) = status.container_id.as_deref() else {
                continue;
            };
            self.containers.insert(
                normalize_container_id(container_id),
                ContainerRecord {
                    pod_uid: uid.clone(),
                    pod_name: pod.name_any(),
                    namespace: namespace.clone(),
                    container_name: status.name.clone(),
                    owner: owner.clone(),
                    expires_at: None,
                },
            );
        }
    }

    pub fn delete_pod(&self, pod: &Pod) {
        let Some(uid) = pod.meta().uid.as_deref() else {
            return;
        };
        let expires = Instant::now() + self.terminated_ttl;
        for mut record in self.containers.iter_mut() {
            if record.pod_uid == uid {
                record.expires_at = Some(expires);
            }
        }
    }

    pub fn apply_replica_set(&self, replica_set: &ReplicaSet) {
        let Some(namespace) = replica_set.namespace() else {
            return;
        };
        let Some(uid) = replica_set.meta().uid.clone() else {
            return;
        };
        self.replica_sets.insert(
            (namespace, replica_set.name_any()),
            Controller {
                uid,
                owner: controller_owner(&replica_set.metadata.owner_references),
                labels: labels(&replica_set.metadata.labels),
            },
        );
    }

    pub fn apply_deployment(&self, deployment: &Deployment) {
        let Some(namespace) = deployment.namespace() else {
            return;
        };
        let Some(uid) = deployment.meta().uid.clone() else {
            return;
        };
        self.deployments.insert(
            (namespace, deployment.name_any()),
            Controller {
                uid,
                owner: None,
                labels: labels(&deployment.metadata.labels),
            },
        );
    }

    pub fn cleanup_expired(&self) {
        let now = Instant::now();
        self.containers
            .retain(|_, record| record.expires_at.is_none_or(|expires| expires > now));
    }

    pub fn resolve(
        &self,
        container_id: &str,
        node_name: &str,
        selectors: &[WorkloadSelector],
    ) -> Result<KubernetesAttribution, AttributionError> {
        self.cleanup_expired();
        let container = self
            .containers
            .get(&normalize_container_id(container_id))
            .ok_or(AttributionError::UnknownContainer)?;
        let (workload_uid, workload_kind, workload_name, workload_labels) =
            match container.owner.kind.as_str() {
                "ReplicaSet" => {
                    let rs = self
                        .replica_sets
                        .get(&(container.namespace.clone(), container.owner.name.clone()))
                        .ok_or(AttributionError::IncompleteOwnerChain)?;
                    let deployment_owner = rs
                        .owner
                        .as_ref()
                        .filter(|owner| owner.kind == "Deployment")
                        .ok_or(AttributionError::IncompleteOwnerChain)?;
                    let deployment = self
                        .deployments
                        .get(&(container.namespace.clone(), deployment_owner.name.clone()))
                        .ok_or(AttributionError::IncompleteOwnerChain)?;
                    (
                        deployment.uid.clone(),
                        "Deployment".to_owned(),
                        deployment_owner.name.clone(),
                        deployment.labels.clone(),
                    )
                }
                kind => (
                    container.owner.uid.clone(),
                    kind.to_owned(),
                    container.owner.name.clone(),
                    BTreeMap::new(),
                ),
            };
        let metadata = WorkloadMetadata {
            namespace: container.namespace.clone(),
            kind: workload_kind.clone(),
            name: workload_name.clone(),
            labels: workload_labels,
        };
        let selector = selectors
            .iter()
            .find(|selector| selector.matches(&metadata))
            .ok_or(AttributionError::NotSelected)?;
        Ok(KubernetesAttribution {
            project_id: selector.project_id,
            application_id: selector.application_id,
            node_name: node_name.into(),
            namespace: container.namespace.clone(),
            pod_uid: container.pod_uid.clone(),
            pod_name: container.pod_name.clone(),
            container_id: normalize_container_id(container_id),
            container_name: container.container_name.clone(),
            workload_uid,
            workload_kind,
            workload_name,
        })
    }
}

pub fn resolve_and_count(
    cache: &AttributionCache,
    counters: &Counters,
    container_id: Option<&str>,
    node_name: &str,
    selectors: &[WorkloadSelector],
) -> Option<KubernetesAttribution> {
    let result = container_id
        .ok_or(AttributionError::UnknownContainer)
        .and_then(|id| cache.resolve(id, node_name, selectors));
    match result {
        Ok(attribution) => Some(attribution),
        Err(AttributionError::NotSelected) => {
            tracing::debug!(container_id, "event belongs to a non-selected workload");
            counters.filtered.fetch_add(1, Ordering::Relaxed);
            None
        }
        Err(
            error @ (AttributionError::UnknownContainer | AttributionError::IncompleteOwnerChain),
        ) => {
            tracing::debug!(container_id, %error, "Kubernetes attribution failed");
            counters.unattributed.fetch_add(1, Ordering::Relaxed);
            None
        }
    }
}

pub async fn run_watches(client: Client, cache: Arc<AttributionCache>) -> anyhow::Result<()> {
    let pods: Api<Pod> = Api::all(client.clone());
    let replicas: Api<ReplicaSet> = Api::all(client.clone());
    let deployments: Api<Deployment> = Api::all(client);
    tokio::try_join!(
        watch_pods(pods, cache.clone()),
        watch_replica_sets(replicas, cache.clone()),
        watch_deployments(deployments, cache),
    )?;
    Ok(())
}

async fn watch_pods(api: Api<Pod>, cache: Arc<AttributionCache>) -> anyhow::Result<()> {
    let mut stream = watcher::watcher(api, watcher::Config::default()).boxed();
    while let Some(event) = stream.try_next().await? {
        match event {
            Event::Apply(pod) | Event::InitApply(pod) => cache.apply_pod(&pod),
            Event::Delete(pod) => cache.delete_pod(&pod),
            Event::Init | Event::InitDone => {}
        }
    }
    Ok(())
}

async fn watch_replica_sets(
    api: Api<ReplicaSet>,
    cache: Arc<AttributionCache>,
) -> anyhow::Result<()> {
    let mut stream = watcher::watcher(api, watcher::Config::default()).boxed();
    while let Some(event) = stream.try_next().await? {
        if let Event::Apply(resource) | Event::InitApply(resource) = event {
            cache.apply_replica_set(&resource);
        }
    }
    Ok(())
}

async fn watch_deployments(
    api: Api<Deployment>,
    cache: Arc<AttributionCache>,
) -> anyhow::Result<()> {
    let mut stream = watcher::watcher(api, watcher::Config::default()).boxed();
    while let Some(event) = stream.try_next().await? {
        if let Event::Apply(resource) | Event::InitApply(resource) = event {
            cache.apply_deployment(&resource);
        }
    }
    Ok(())
}

fn controller_owner(
    owners: &Option<Vec<k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference>>,
) -> Option<Owner> {
    owners
        .as_ref()?
        .iter()
        .find(|owner| owner.controller.unwrap_or(false))
        .map(|owner| Owner {
            uid: owner.uid.clone(),
            kind: owner.kind.clone(),
            name: owner.name.clone(),
        })
}

fn labels(values: &Option<BTreeMap<String, String>>) -> BTreeMap<String, String> {
    values.clone().unwrap_or_default()
}

fn normalize_container_id(value: &str) -> String {
    value
        .rsplit_once("://")
        .map_or(value, |(_, id)| id)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::{
        api::core::v1::{ContainerStatus, PodStatus},
        apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference},
    };
    use uuid::Uuid;

    fn owner(kind: &str, name: &str, uid: &str) -> OwnerReference {
        OwnerReference {
            api_version: "apps/v1".into(),
            kind: kind.into(),
            name: name.into(),
            uid: uid.into(),
            controller: Some(true),
            block_owner_deletion: None,
        }
    }

    #[test]
    fn resolves_deployment_and_filters_non_selected_workload() {
        let cache = AttributionCache::new(Duration::from_secs(30));
        let deployment = Deployment {
            metadata: ObjectMeta {
                namespace: Some("production".into()),
                name: Some("payment-api".into()),
                uid: Some("deployment-uid".into()),
                labels: Some(BTreeMap::from([("app".into(), "payment-api".into())])),
                ..Default::default()
            },
            ..Default::default()
        };
        let replica_set = ReplicaSet {
            metadata: ObjectMeta {
                namespace: Some("production".into()),
                name: Some("payment-api-abc".into()),
                uid: Some("rs-uid".into()),
                owner_references: Some(vec![owner("Deployment", "payment-api", "deployment-uid")]),
                ..Default::default()
            },
            ..Default::default()
        };
        let pod = Pod {
            metadata: ObjectMeta {
                namespace: Some("production".into()),
                name: Some("payment-api-1".into()),
                uid: Some("pod-uid".into()),
                owner_references: Some(vec![owner("ReplicaSet", "payment-api-abc", "rs-uid")]),
                ..Default::default()
            },
            status: Some(PodStatus {
                container_statuses: Some(vec![ContainerStatus {
                    name: "payment-api".into(),
                    container_id: Some("containerd://abc".into()),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            ..Default::default()
        };
        cache.apply_deployment(&deployment);
        cache.apply_replica_set(&replica_set);
        cache.apply_pod(&pod);
        let selector = WorkloadSelector {
            project_id: Uuid::new_v4(),
            application_id: Uuid::new_v4(),
            namespace: "production".into(),
            kind: "Deployment".into(),
            name: "payment-api".into(),
            labels: BTreeMap::from([("app".into(), "payment-api".into())]),
        };
        let result = cache
            .resolve("abc", "node-1", std::slice::from_ref(&selector))
            .unwrap();
        assert_eq!(result.workload_uid, "deployment-uid");
        assert_eq!(result.container_name, "payment-api");
        let other = WorkloadSelector {
            name: "other".into(),
            ..selector
        };
        assert_eq!(
            cache.resolve("abc", "node-1", &[other]).unwrap_err(),
            AttributionError::NotSelected
        );
    }

    #[test]
    fn missing_container_is_counted_as_unattributed() {
        let cache = AttributionCache::new(Duration::ZERO);
        let counters = Counters::default();
        assert!(resolve_and_count(&cache, &counters, Some("missing"), "node-1", &[]).is_none());
        assert_eq!(counters.snapshot().unattributed, 1);
    }
}
