use serde::{Deserialize, Serialize};
use specta::Type;

// Candidate wire shapes only. P1 must define domain types and validation.
#[derive(Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum BusinessOutcome {
    Succeeded { result: RunResult },
    Failed { code: String, message: String },
    Cancelled,
    Interrupted,
    Skipped { reason: String },
}
#[derive(Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum RunResult {
    OutputUnavailable {
        code: String,
    },
    Refresh {
        revision: String,
        application: ApplicationEffect,
    },
    Fields {
        fields: Vec<ResultField>,
    },
}
#[derive(Debug, PartialEq, Serialize, Deserialize, Type)]
struct ResultField {
    key: String,
    value: String,
}
#[derive(Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ApplicationEffect {
    NotRequired,
    Reconciled,
    Degraded { code: String },
    Unknown { operation_id: Option<String> },
}
#[derive(Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum JournalState {
    Durable,
    Degraded { code: String },
}
#[derive(Debug, PartialEq, Serialize, Deserialize, Type)]
struct CompletionDto {
    run_id: String,
    admitted_sequence: String,
    outcome: BusinessOutcome,
    journal: JournalState,
    last_log_sequence: String,
}
#[derive(Serialize, Deserialize, Type)]
struct PageDto {
    items: Vec<CompletionDto>,
    next_cursor: Option<String>,
}

#[test]
fn finite_dto_exports_without_any_recursive_json_or_bigint() {
    let types = specta::Types::default().register::<PageDto>();
    let path = tempfile::tempdir().unwrap();
    let file = path.path().join("bindings.ts");
    specta_typescript::Typescript::default()
        .export_to(&file, &types, specta_serde::Format)
        .unwrap();
    let generated = std::fs::read_to_string(file).unwrap();
    for token in [
        "CompletionDto",
        "ApplicationEffect",
        "run_id: string",
        "admitted_sequence: string",
        "outcome: BusinessOutcome",
        "last_log_sequence: string",
    ] {
        assert!(generated.contains(token), "missing {token}: {generated}");
    }
    assert!(!generated.contains("any"));
    assert!(!generated.contains("bigint"));
}

#[test]
fn business_success_can_round_trip_with_journal_failure_and_unknown_application() {
    let completion = CompletionDto {
        run_id: "f1b52467-3b4e-40f1-8c90-5a1375a93001".into(),
        admitted_sequence: u64::MAX.to_string(),
        outcome: BusinessOutcome::Succeeded {
            result: RunResult::Refresh {
                revision: "9007199254740993".into(),
                application: ApplicationEffect::Unknown {
                    operation_id: Some("lifecycle-operation".into()),
                },
            },
        },
        journal: JournalState::Degraded {
            code: "commit_failed".into(),
        },
        last_log_sequence: u64::MAX.to_string(),
    };
    let json = serde_json::to_value(&completion).unwrap();
    assert!(json["admitted_sequence"].is_string());
    assert_eq!(json["outcome"]["kind"], "succeeded");
    assert_eq!(json["journal"]["kind"], "degraded");
    assert_eq!(
        serde_json::from_value::<CompletionDto>(json).unwrap(),
        completion
    );
}
