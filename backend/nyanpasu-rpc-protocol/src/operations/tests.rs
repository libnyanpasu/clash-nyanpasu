use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

use crate::{COMMAND_CATALOG, RpcTarget, operation::RpcOperation, operations};

#[test]
fn operation_methods_match_the_local_catalog() {
    let mut names = vec![
        operations::GetProfiles::NAME,
        operations::CreateProfile::NAME,
        operations::DeleteProfile::NAME,
        operations::ReadProfileFile::NAME,
        operations::GetConfigurationStatus::NAME,
        operations::GetCoreStatus::NAME,
        operations::GetStorageItem::NAME,
        operations::SetStorageItem::NAME,
        operations::RemoveStorageItem::NAME,
        operations::ListLogFiles::NAME,
        operations::OpenLogSession::NAME,
        operations::QueryLogs::NAME,
        operations::CloseLogSession::NAME,
    ];
    let mut catalog: Vec<_> = COMMAND_CATALOG
        .iter()
        .filter(|command| command.targets.contains(&RpcTarget::Local))
        .map(|command| command.name)
        .collect();
    names.sort_unstable();
    catalog.sort_unstable();
    assert_eq!(names, catalog);
}

fn sample<T: DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).unwrap()
}

fn request<O: RpcOperation>(expected: Value) {
    let fixtures: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/contracts/2026-11-12.json"
    ))
    .unwrap();
    assert_eq!(expected, fixtures[O::NAME]["request"], "{}", O::NAME);
    let request: O::Request = sample(expected.clone());
    assert_eq!(
        serde_json::to_value(request).unwrap(),
        expected,
        "{}",
        O::NAME
    );
}

#[test]
fn operation_requests_keep_existing_json_shapes() {
    request::<operations::GetProfiles>(Value::Null);
    request::<operations::GetConfigurationStatus>(Value::Null);
    request::<operations::GetCoreStatus>(Value::Null);
    request::<operations::CreateProfile>(json!({
        "request": {"metadata": {"name": "profile"},
            "definition": {"type": "config", "config": {"type": "composition"}},
            "folder": null},
        "fileData": "proxies: []\n"
    }));
    request::<operations::DeleteProfile>(json!({"uid": "profile-1"}));
    request::<operations::ReadProfileFile>(json!({"uid": "profile-1"}));
    request::<operations::GetStorageItem>(json!({"key": "key"}));
    request::<operations::SetStorageItem>(json!({"key": "key", "value": "value"}));
    request::<operations::RemoveStorageItem>(json!({"key": "key"}));
    request::<operations::ListLogFiles>(json!({"source": "app"}));
    request::<operations::OpenLogSession>(json!({"source": "service", "request": {
        "request_id": "request-1", "file": null
    }}));
    request::<operations::QueryLogs>(json!({"source": "app", "request": {
        "session": "session-1", "filter": {"levels": [], "target": null, "from_ms": null, "to_ms": null, "text": null},
        "direction": "latest", "cursor": null, "limit": 20
    }}));
    request::<operations::CloseLogSession>(json!({"source": "app", "session": "session-1"}));

    let omitted: operations::CreateProfileRequest = sample(json!({"request": {
        "metadata": {"name": "profile"},
        "definition": {"type": "config", "config": {"type": "composition"}}
    }, "unknown": true}));
    assert!(omitted.file_data.is_none());
    assert!(omitted.request.folder.is_none());
}

fn receive<O: RpcOperation>(response: O::Response)
where
    O::ReceivedResponse: Serialize,
{
    let expected = serde_json::to_value(&response).unwrap();
    let fixtures: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/contracts/2026-11-12.json"
    ))
    .unwrap();
    assert!(
        fixtures[O::NAME]["successes"]
            .as_array()
            .unwrap()
            .contains(&expected),
        "{}: {expected}",
        O::NAME
    );
    let received: O::ReceivedResponse = sample(expected.clone());
    assert_eq!(
        serde_json::to_value(received).unwrap(),
        expected,
        "{}",
        O::NAME
    );
}

#[test]
fn operation_receivers_decode_existing_success_values() {
    receive::<operations::GetProfiles>(nyanpasu_config::profile::Profiles::default());
    for degraded in [false, true] {
        let create = if degraded {
            nyanpasu_runtime_values::runtime::MutationOutcome::CommittedDegraded {
                value: sample(json!("profile-1")),
                commits: vec![],
                notifications_pending: true,
                degradations: vec![],
            }
        } else {
            nyanpasu_runtime_values::runtime::MutationOutcome::from_parts(
                sample(json!("profile-1")),
                vec![],
            )
        };
        receive::<operations::CreateProfile>(create);
        let delete = if degraded {
            nyanpasu_runtime_values::runtime::MutationOutcome::CommittedDegraded {
                value: (),
                commits: vec![],
                notifications_pending: true,
                degradations: vec![],
            }
        } else {
            nyanpasu_runtime_values::runtime::MutationOutcome::from_parts((), vec![])
        };
        receive::<operations::DeleteProfile>(delete);
    }
    receive::<operations::ReadProfileFile>("proxies: []\n".into());
    receive::<operations::GetConfigurationStatus>(sample(json!({
        "event_seq": 1, "maintenance": null, "active": null, "effects": [], "sources": [],
        "recent_operations": [], "source_versions": {"application": 1, "clash": 2, "session": 3, "profiles": 4},
        "runtime": {"health": "healthy", "operation_id": null, "attempts": 0, "automatic_remaining": 0, "message": null}
    })));
    receive::<operations::GetCoreStatus>(sample(json!({
        "controller": null, "host": "local", "connectivity": {"kind": "shut_down"},
        "generation": 1, "state": null, "state_changed_at": 0, "revision": null, "healthy": null
    })));
    for value in [Value::Null, json!("stored")] {
        receive::<operations::GetStorageItem>(sample(json!({"value": value, "revision": 7})));
    }
    receive::<operations::SetStorageItem>(sample(json!(8)));
    receive::<operations::RemoveStorageItem>(sample(json!(9)));
    receive::<operations::ListLogFiles>(sample(
        json!([{"id": "file-1", "name": "app.log", "bytes": "123"}]),
    ));
    receive::<operations::OpenLogSession>(sample(json!({"id": "session-1", "lease_ms": 45000})));
    receive::<operations::QueryLogs>(sample(json!({
        "rows": [], "cursor": {"generation": "1", "offset": "0"}, "head": {"generation": "1", "offset": "0"},
        "start": "0", "file": "file-1", "building": false, "more": false, "partial": false,
        "malformed": "0", "truncated": "0", "indexed_bytes": "0", "file_bytes": "0"
    })));
    receive::<operations::CloseLogSession>(());
}
