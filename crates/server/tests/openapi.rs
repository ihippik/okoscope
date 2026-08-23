use std::collections::HashSet;

const LIVE_OPERATIONS: &[(&str, &str)] = &[
    ("/api/v1/build-info", "get"),
    ("/api/v1/attention-summary", "get"),
    (
        "/api/v1/projects/{project_id}/applications/{application_id}/attention-summary",
        "get",
    ),
    ("/api/v1/organization", "get"),
    ("/api/v1/projects", "get"),
    ("/api/v1/projects/{project_id}", "get"),
    ("/api/v1/projects/{project_id}/applications", "get"),
    (
        "/api/v1/projects/{project_id}/applications/{application_id}",
        "get",
    ),
    (
        "/api/v1/projects/{project_id}/applications/{application_id}/workers",
        "get",
    ),
    (
        "/api/v1/projects/{project_id}/applications/{application_id}/runtime-inventory",
        "get",
    ),
    (
        "/api/v1/projects/{project_id}/applications/{application_id}/runtime-inventory/summary",
        "get",
    ),
    (
        "/api/v1/projects/{project_id}/applications/{application_id}/runtime-inventory/distribution",
        "get",
    ),
    (
        "/api/v1/projects/{project_id}/applications/{application_id}/runtime-inventory/facets/{facet}",
        "get",
    ),
    (
        "/api/v1/projects/{project_id}/applications/{application_id}/runtime-inventory/{item_id}",
        "get",
    ),
    (
        "/api/v1/projects/{project_id}/applications/{application_id}/runtime-inventory/{item_id}/releases",
        "get",
    ),
    (
        "/api/v1/projects/{project_id}/applications/{application_id}/runtime-inventory/{item_id}/sightings",
        "get",
    ),
    (
        "/api/v1/projects/{project_id}/applications/{application_id}/runtime-inventory/{item_id}/groups",
        "get",
    ),
    (
        "/api/v1/projects/{project_id}/applications/{application_id}/runtime-inventory/{item_id}/occurrences",
        "get",
    ),
    ("/api/v1/runtime-groups", "get"),
    ("/api/v1/runtime-groups/{group_id}", "get"),
    ("/api/v1/runtime-groups/{group_id}/occurrences", "get"),
    ("/api/v1/runtime-groups/{group_id}/acknowledge", "post"),
    ("/api/v1/runtime-groups/{group_id}/resolve", "post"),
    ("/api/v1/runtime-groups/{group_id}/reopen", "post"),
    (
        "/api/v1/projects/{project_id}/applications/{application_id}/releases",
        "get",
    ),
    (
        "/api/v1/projects/{project_id}/applications/{application_id}/releases",
        "post",
    ),
    (
        "/api/v1/projects/{project_id}/applications/{application_id}/releases/{release_id}",
        "get",
    ),
    (
        "/api/v1/projects/{project_id}/applications/{application_id}/releases/{target_id}/runtime-diff",
        "get",
    ),
    (
        "/api/v1/projects/{project_id}/applications/{application_id}/releases/{target_id}/runtime-diff/summary",
        "get",
    ),
    ("/api/v1/projects/{project_id}/webhook-destinations", "get"),
    ("/api/v1/projects/{project_id}/webhook-destinations", "post"),
    (
        "/api/v1/projects/{project_id}/webhook-destinations/{destination_id}",
        "get",
    ),
    (
        "/api/v1/projects/{project_id}/webhook-destinations/{destination_id}",
        "patch",
    ),
    (
        "/api/v1/projects/{project_id}/webhook-destinations/{destination_id}/disable",
        "post",
    ),
    (
        "/api/v1/projects/{project_id}/webhook-destinations/{destination_id}/rotate-secret",
        "post",
    ),
    (
        "/api/v1/projects/{project_id}/webhook-destinations/{destination_id}/test",
        "post",
    ),
    (
        "/api/v1/projects/{project_id}/notification-deliveries",
        "get",
    ),
    (
        "/api/v1/projects/{project_id}/notification-deliveries/{delivery_id}",
        "get",
    ),
    (
        "/api/v1/projects/{project_id}/notification-deliveries/bulk-retry",
        "post",
    ),
    (
        "/api/v1/projects/{project_id}/notification-deliveries/{delivery_id}/retry",
        "post",
    ),
    (
        "/api/v1/projects/{project_id}/notification-deliveries/{delivery_id}/cancel",
        "post",
    ),
    (
        "/api/v1/projects/{project_id}/notification-recovery-operations",
        "get",
    ),
    (
        "/api/v1/projects/{project_id}/notification-recovery-operations/{operation_id}",
        "get",
    ),
    ("/api/v1/projects/{project_id}/notification-health", "get"),
];

#[test]
fn openapi_is_valid_unique_secure_and_matches_router_inventory() {
    let source = include_str!("../../../openapi/okoscope-v1.yaml");
    let document: serde_json::Value = serde_yaml::from_str(source).expect("valid OpenAPI YAML");
    assert_eq!(document["openapi"], "3.1.0");
    assert_eq!(document["security"][0]["bearerAuth"], serde_json::json!([]));

    let paths = document["paths"].as_object().expect("paths object");
    let mut operation_ids = HashSet::new();
    for &(path, method) in LIVE_OPERATIONS {
        let operation = &paths[path][method];
        assert!(
            operation.is_object(),
            "missing live operation {method} {path}"
        );
        let operation_id = operation["operationId"].as_str().expect("operationId");
        assert!(
            operation_ids.insert(operation_id),
            "duplicate operationId {operation_id}"
        );
        if path == "/api/v1/build-info" {
            assert_eq!(operation["security"], serde_json::json!([]));
        } else {
            assert!(
                operation.get("security").is_none(),
                "protected route overrides bearer security: {method} {path}"
            );
        }
        assert_success_response_is_typed(&document, operation, path, method);
    }
    let documented = paths
        .values()
        .map(|item| {
            ["get", "post", "patch"]
                .into_iter()
                .filter(|method| item.get(method).is_some())
                .count()
        })
        .sum::<usize>();
    assert_eq!(
        documented,
        LIVE_OPERATIONS.len(),
        "documented route inventory drift"
    );
    assert_navigation_and_group_contract(&document);
    assert_network_contract(&document);
    assert_inventory_contract(&document);
    assert_notification_health_contract(&document);
    assert_delivery_contract(&document);
    assert_recovery_contract(&document);
    assert_query_parameters(
        &document,
        "/api/v1/projects/{project_id}/applications/{application_id}/releases/{target_id}/runtime-diff",
        "get",
        &["baseline_id", "cursor", "limit"],
    );
}

fn assert_navigation_and_group_contract(document: &serde_json::Value) {
    assert_query_parameters(
        document,
        "/api/v1/projects/{project_id}/applications/{application_id}/workers",
        "get",
        &["cursor", "limit"],
    );
    assert_query_parameters(
        document,
        "/api/v1/runtime-groups",
        "get",
        &[
            "project_id",
            "application_id",
            "event_kind",
            "status",
            "namespace",
            "workload_kind",
            "workload_name",
            "since",
            "first_seen_from",
            "first_seen_to",
            "last_seen_to",
            "release_id",
            "cursor",
            "limit",
        ],
    );
    for path in [
        "/api/v1/projects/{project_id}/applications/{application_id}/releases",
        "/api/v1/runtime-groups/{group_id}/occurrences",
    ] {
        assert_query_parameters(document, path, "get", &["cursor", "limit"]);
    }
    assert_required_fields(
        document,
        "ApplicationWorker",
        &[
            "agent_id",
            "cluster_id",
            "cluster_name",
            "node_name",
            "agent_version",
            "architecture",
            "kernel_release",
            "first_observed_at",
            "last_observed_at",
            "agent_last_seen_at",
        ],
    );
    assert_required_fields(
        document,
        "RuntimeGroup",
        &[
            "first_seen_event_id",
            "status_changed_at",
            "status_changed_by",
        ],
    );
}

fn assert_required_fields(document: &serde_json::Value, schema: &str, expected: &[&str]) {
    let required = document["components"]["schemas"][schema]["required"]
        .as_array()
        .unwrap_or_else(|| panic!("{schema} required fields"));
    for field in expected {
        assert!(
            required.iter().any(|value| value == field),
            "{schema} must require {field}"
        );
    }
}

#[test]
fn inbound_contract_fixture_keeps_remote_clients_in_occurrences_only() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../docs/fixtures/runtime-inventory.json"
    ))
    .expect("valid runtime inventory fixture");
    assert_eq!(fixture["summary"]["kinds"].as_array().unwrap().len(), 5);
    let endpoint = fixture["page"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["inventory_kind"] == "inbound_endpoint")
        .expect("inbound endpoint fixture");
    assert_eq!(endpoint["semantic_summary"]["listener_observed"], true);
    assert_eq!(endpoint["semantic_summary"]["accept_observed"], true);
    assert!(endpoint["semantic_summary"].get("remote_address").is_none());
    assert!(
        fixture["inbound_group"]["semantic_summary"]
            .get("remote_address")
            .is_none()
    );
    assert_eq!(
        fixture["inbound_occurrences"]["items"][0]["payload"]["data"]["remote_address"],
        "203.0.113.9"
    );
    assert!(fixture["inbound_occurrences"]["next_cursor"].is_null());
}

fn assert_inventory_contract(document: &serde_json::Value) {
    let schemas = &document["components"]["schemas"];
    assert_query_parameters(
        document,
        "/api/v1/projects/{project_id}/applications/{application_id}/runtime-inventory",
        "get",
        &[
            "kind",
            "operation",
            "release_id",
            "cluster_id",
            "namespace",
            "workload_kind",
            "workload_name",
            "container_name",
            "observed_from",
            "observed_to",
            "search",
            "identity_token",
            "cursor",
            "limit",
        ],
    );
    assert_query_parameters(
        document,
        "/api/v1/projects/{project_id}/applications/{application_id}/runtime-inventory/distribution",
        "get",
        &[
            "kind",
            "operation",
            "release_id",
            "cluster_id",
            "namespace",
            "workload_kind",
            "workload_name",
            "container_name",
            "observed_from",
            "observed_to",
            "search",
            "limit",
        ],
    );
    assert_eq!(
        schemas["InventoryDistribution"]["properties"]["entries"]["maxItems"],
        10
    );
    assert_eq!(
        schemas["InventoryKind"]["enum"],
        serde_json::json!([
            "process",
            "destination",
            "domain",
            "syscall",
            "inbound_endpoint",
            "file_activity"
        ])
    );
    assert_eq!(
        schemas["InventoryReleasePresence"]["enum"],
        serde_json::json!(["observed", "not_observed", "unknown"])
    );
    for schema in [
        "InventoryProcessIdentity",
        "InventoryDestinationIdentity",
        "InventoryDomainIdentity",
        "InventorySyscallIdentity",
        "InventoryInboundEndpointIdentity",
        "FileActivitySemanticSummary",
        "InventoryItem",
        "InventoryReleaseEvidence",
        "InventorySighting",
    ] {
        assert_eq!(
            schemas[schema]["additionalProperties"], false,
            "{schema} must remain a closed safe contract"
        );
    }
    for page in [
        "InventoryItemPage",
        "InventoryReleasePresencePage",
        "InventorySightingPage",
        "InventoryGroupPage",
        "InventoryOccurrencePage",
    ] {
        assert_eq!(schemas[page]["properties"]["items"]["maxItems"], 200);
    }
}

fn assert_network_contract(document: &serde_json::Value) {
    let schemas = &document["components"]["schemas"];
    assert_eq!(
        schemas["NetworkConnectSemanticSummary"]["additionalProperties"],
        false
    );
    assert_eq!(
        schemas["NetworkConnectPayload"]["properties"]["data"]["additionalProperties"],
        false
    );
    for forbidden in ["payload", "dns_name", "source_port", "url", "http"] {
        assert!(
            schemas["NetworkConnectSemanticSummary"]["properties"][forbidden].is_null(),
            "network semantic summary must not expose {forbidden}"
        );
        assert!(
            schemas["NetworkConnectPayload"]["properties"]["data"]["properties"][forbidden]
                .is_null(),
            "network occurrence payload must not expose {forbidden}"
        );
    }
    for schema in [
        "InboundNetworkSemanticSummary",
        "NetworkListenPayload",
        "NetworkAcceptPayload",
    ] {
        assert_eq!(schemas[schema]["additionalProperties"], false);
    }
    for forbidden in ["remote_address", "remote_port", "payload", "http", "tls"] {
        assert!(
            schemas["InboundNetworkSemanticSummary"]["properties"][forbidden].is_null(),
            "inbound group summary must not expose {forbidden}"
        );
    }
    assert_eq!(
        schemas["NetworkAcceptPayload"]["properties"]["data"]["additionalProperties"],
        false
    );
}

fn assert_recovery_contract(document: &serde_json::Value) {
    for schema_name in [
        "DeliveryRecoveryResult",
        "BulkRecoveryResult",
        "RecoveryOperationSummary",
        "RecoveryOperationPage",
    ] {
        assert_eq!(
            document["components"]["schemas"][schema_name]["additionalProperties"], false,
            "{schema_name} must remain concrete"
        );
    }
    for path in [
        "/api/v1/projects/{project_id}/notification-deliveries/bulk-retry",
        "/api/v1/projects/{project_id}/notification-deliveries/{delivery_id}/retry",
        "/api/v1/projects/{project_id}/notification-deliveries/{delivery_id}/cancel",
    ] {
        let parameters = document["paths"][path]["post"]["parameters"]
            .as_array()
            .expect("recovery command parameters");
        assert!(
            parameters
                .iter()
                .any(|parameter| { parameter["$ref"] == "#/components/parameters/IdempotencyKey" })
        );
        assert!(document["paths"][path]["post"]["responses"]["409"].is_object());
    }
}

fn assert_notification_health_contract(document: &serde_json::Value) {
    let schema = &document["components"]["schemas"]["NotificationHealth"];
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(
        schema["properties"]["state"]["enum"],
        serde_json::json!([
            "disabled",
            "idle",
            "backlogged",
            "retrying",
            "failing",
            "draining"
        ])
    );
    assert!(schema["required"].as_array().is_some_and(|fields| {
        ["state", "delivery_enabled", "observed_at"]
            .iter()
            .all(|field| fields.iter().any(|item| item == field))
    }));
}

fn assert_delivery_contract(document: &serde_json::Value) {
    let schema = &document["components"]["schemas"]["DeliverySummary"];
    let required = schema["required"]
        .as_array()
        .expect("delivery required fields");
    for field in [
        "next_attempt_at",
        "terminal_reason",
        "semantic_metadata",
        "destination",
    ] {
        assert!(required.iter().any(|item| item == field), "missing {field}");
    }
    assert_eq!(
        document["components"]["schemas"]["SafeDestination"]["additionalProperties"],
        false
    );
    assert!(
        document["components"]["schemas"]["SafeDestination"]["properties"]["url"].is_null(),
        "safe destination must not expose a URL"
    );
    assert_eq!(
        document["components"]["schemas"]["DeliverySemanticMetadata"]["additionalProperties"],
        false
    );
}

fn assert_query_parameters(
    document: &serde_json::Value,
    path: &str,
    method: &str,
    expected: &[&str],
) {
    let parameters = document["paths"][path][method]["parameters"]
        .as_array()
        .expect("operation parameters");
    let actual = parameters
        .iter()
        .map(|parameter| {
            let parameter = resolve_component(document, parameter, "parameters");
            assert_eq!(parameter["in"], "query", "non-query operation parameter");
            parameter["name"].as_str().expect("parameter name")
        })
        .collect::<HashSet<_>>();
    assert_eq!(
        actual,
        expected.iter().copied().collect(),
        "query parameter drift for {method} {path}"
    );
}

fn assert_success_response_is_typed(
    document: &serde_json::Value,
    operation: &serde_json::Value,
    path: &str,
    method: &str,
) {
    let responses = operation["responses"]
        .as_object()
        .expect("responses object");
    let (_, response) = responses
        .iter()
        .find(|(status, _)| status.starts_with('2'))
        .unwrap_or_else(|| panic!("missing success response for {method} {path}"));
    let response = resolve_component(document, response, "responses");
    let schema = &response["content"]["application/json"]["schema"];
    assert!(
        schema.is_object(),
        "missing JSON success schema for {method} {path}"
    );
    let schema = resolve_component(document, schema, "schemas");
    assert_ne!(
        schema.get("additionalProperties"),
        Some(&serde_json::Value::Bool(true)),
        "untyped success schema for {method} {path}"
    );
    assert!(
        schema.get("properties").is_some()
            || schema.get("items").is_some()
            || schema.get("allOf").is_some(),
        "success schema has no declared shape for {method} {path}"
    );
}

fn resolve_component<'a>(
    document: &'a serde_json::Value,
    value: &'a serde_json::Value,
    component_kind: &str,
) -> &'a serde_json::Value {
    let Some(reference) = value.get("$ref").and_then(serde_json::Value::as_str) else {
        return value;
    };
    let name = reference
        .strip_prefix(&format!("#/components/{component_kind}/"))
        .unwrap_or_else(|| panic!("unexpected component reference {reference}"));
    resolve_component(
        document,
        &document["components"][component_kind][name],
        component_kind,
    )
}
