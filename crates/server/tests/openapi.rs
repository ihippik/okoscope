use std::collections::HashSet;

const LIVE_OPERATIONS: &[(&str, &str)] = &[
    ("/api/v1/build-info", "get"),
    ("/api/v1/organization", "get"),
    ("/api/v1/projects", "get"),
    ("/api/v1/projects/{project_id}", "get"),
    ("/api/v1/projects/{project_id}/applications", "get"),
    (
        "/api/v1/projects/{project_id}/applications/{application_id}",
        "get",
    ),
    ("/api/v1/runtime-groups", "get"),
    ("/api/v1/runtime-groups/{group_id}", "get"),
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
