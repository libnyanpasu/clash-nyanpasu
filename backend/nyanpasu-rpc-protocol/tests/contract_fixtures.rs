//! Published JSON examples are independent of the current sending definitions.

use std::collections::BTreeSet;

use nyanpasu_rpc_protocol::{
    COMMAND_CATALOG, RpcTarget, operation::RpcOperation, operations, types::RpcError,
};
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct Fixture {
    request: Value,
    successes: Vec<Value>,
    failures: Vec<Value>,
}

fn accepts<O: RpcOperation>(fixtures: &Value) {
    let fixture: Fixture = serde_json::from_value(fixtures[O::NAME].clone()).unwrap();
    let request: O::Request = serde_json::from_value(fixture.request.clone())
        .unwrap_or_else(|error| panic!("{} request: {error}", O::NAME));
    assert_eq!(
        serde_json::to_value(request).unwrap(),
        fixture.request,
        "{}",
        O::NAME
    );
    assert!(!fixture.successes.is_empty(), "{} successes", O::NAME);
    for response in fixture.successes {
        serde_json::from_value::<O::ReceivedResponse>(response)
            .unwrap_or_else(|error| panic!("{} success: {error}", O::NAME));
    }
    assert!(!fixture.failures.is_empty(), "{} failures", O::NAME);
    for response in fixture.failures {
        let envelope: RpcError = serde_json::from_value(response.clone()).unwrap();
        assert_eq!(
            serde_json::to_value(&envelope).unwrap(),
            response,
            "{}",
            O::NAME
        );
        if envelope.kind == "application_error" {
            assert!(
                envelope
                    .decode_domain_error::<O::DomainError>()
                    .unwrap()
                    .is_some(),
                "{} domain error",
                O::NAME
            );
        } else {
            assert_eq!(envelope.kind, "invalid_params", "{}", O::NAME);
            assert!(envelope.domain_error.is_none());
        }
    }
}

#[test]
fn current_contract_accepts_the_supported_previous_fixtures() {
    // This commit is the first baseline; the next contract release adds its own set,
    // retains this one as previous, and removes the oldest set beyond current + previous.
    let baseline = include_str!("fixtures/contracts/2026-11-12.json");
    let fixtures: Value = serde_json::from_str(baseline).unwrap();
    let names: BTreeSet<_> = fixtures
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let catalog: BTreeSet<_> = COMMAND_CATALOG
        .iter()
        .filter(|command| command.targets.contains(&RpcTarget::Local))
        .map(|command| command.name)
        .collect();
    assert_eq!(names, catalog);
    accepts::<operations::GetProfiles>(&fixtures);
    accepts::<operations::CreateProfile>(&fixtures);
    accepts::<operations::DeleteProfile>(&fixtures);
    accepts::<operations::ReadProfileFile>(&fixtures);
    accepts::<operations::GetConfigurationStatus>(&fixtures);
    accepts::<operations::GetCoreStatus>(&fixtures);
    accepts::<operations::GetStorageItem>(&fixtures);
    accepts::<operations::SetStorageItem>(&fixtures);
    accepts::<operations::RemoveStorageItem>(&fixtures);
    accepts::<operations::ListLogFiles>(&fixtures);
    accepts::<operations::OpenLogSession>(&fixtures);
    accepts::<operations::QueryLogs>(&fixtures);
    accepts::<operations::CloseLogSession>(&fixtures);
}
