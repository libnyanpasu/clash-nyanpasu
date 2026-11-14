//! End-to-end validation of committed Draft 2020-12 RPC command JSON Schemas against real Rust
//! `serde_json` representations and real strict `Ajv2020` validation via `@nyanpasu/scripts`.

use std::{
    collections::{BTreeMap, HashMap},
    io::Write as _,
    path::Path,
    process::{Command, Stdio},
};

use nyanpasu_config::{
    application::{ApplicationPreferencesPatch, ClashCore, NyanpasuAppConfigPatch, ReleaseChannel},
    profile::{
        ConfigDefinition, FileConfig, FolderId, LocalBinding, ManagedProfilePath, MaterializedFile,
        NewProfileRequest, ProfileDefinition, ProfileId, ProfileMetadata, ProfileSource, Profiles,
    },
};
use nyanpasu_contracts::logs::LogSource;
use nyanpasu_core::client::{
    http_server::HttpServerStatus,
    logs::{LogFilterSummary, QueryLogSummary},
};
use nyanpasu_ipc::types::ServiceStatus;
use nyanpasu_logging::{Direction, Level};
use nyanpasu_rpc::commands::types::GetSysProxyResponse;
use nyanpasu_rpc_protocol::{
    CommandEffect, DescribedCommand, Principal, RpcDescribeResponse, RpcError, RpcTarget, Scope,
    ScopeSet, ServerInfo,
    operations::{CreateProfileRequest, DeleteProfileRequest},
    schemas::CommandSchemaArtifact,
    types::{
        PublicDegradation, PublicDegradationReason, PublicMutationOutcome, RpcAppUpdateOutcome,
        RpcFolderMutationOutcome, RpcMutationOutcome, RpcProfileMutationOutcome,
        RpcUnitMutationOutcome,
    },
};
use nyanpasu_runtime_values::{
    app_update::{AppUpdatePhase, AppUpdateReleaseSummary, AppUpdateSummary},
    configuration::{ConfigurationStatus, ConvergenceHealth, RuntimeConvergence, SourceVersions},
    core_status::{CoreStatusInfo, EndpointConnectivity, ServicePhase},
    execution_host::ExecutionHost,
    observation::{ServiceCompatSummary, ServiceStatusSummary},
    runtime::{
        CommitReceipt, Degradation, DegradationPhase, DegradationReason, MutationOutcome,
        RuntimeCommitStatus,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use specta::{Type as _, Types};
use specta_serde::Phase;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BatchCase {
    id: &'static str,
    schema_key: &'static str,
    instance: Value,
    expect_valid: bool,
}

#[derive(Serialize)]
struct BatchInput {
    schemas: BTreeMap<String, Value>,
    cases: Vec<BatchCase>,
}

#[derive(Deserialize)]
struct BatchOutput {
    total: usize,
    passed: usize,
    failures: Vec<Value>,
}

fn run_ajv_batch(input: &BatchInput) -> BatchOutput {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut child = Command::new("node")
        .current_dir(&repo_root)
        .args(["--run", "test:command-schemas", "--", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn declared test:command-schemas runner");

    let payload = serde_json::to_vec(input).expect("serialize batch validation input");
    {
        let mut stdin = child.stdin.take().expect("batch child stdin piped");
        stdin
            .write_all(&payload)
            .expect("write batch JSON to validator stdin");
    }

    let output = child.wait_with_output().expect("wait for Ajv batch runner");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "Ajv batch runner exited non-zero ({:?}):\nstdout: {stdout}\nstderr: {stderr}",
        output.status.code()
    );
    let parsed: BatchOutput = serde_json::from_str(stdout.trim()).unwrap_or_else(|_| {
        // Handle optional `node --run` banner lines without accepting invalid JSON reports.
        let json_line = stdout
            .lines()
            .rev()
            .find(|line| line.trim_start().starts_with('{'))
            .unwrap_or_else(|| {
                panic!(
                    "missing Ajv batch JSON output on stdout:\nstdout: {stdout}\nstderr: {stderr}"
                )
            });
        serde_json::from_str(json_line).unwrap_or_else(|err| {
            panic!("parse Ajv batch JSON output ({err}):\nstdout: {stdout}\nstderr: {stderr}")
        })
    });
    assert!(
        parsed.failures.is_empty(),
        "Ajv batch failures detected: {:?}",
        parsed.failures
    );
    assert_eq!(parsed.passed, parsed.total);
    parsed
}

fn case_ok(id: &'static str, schema_key: &'static str, instance: Value) -> BatchCase {
    BatchCase {
        id,
        schema_key,
        instance,
        expect_valid: true,
    }
}

fn case_err(id: &'static str, schema_key: &'static str, instance: Value) -> BatchCase {
    BatchCase {
        id,
        schema_key,
        instance,
        expect_valid: false,
    }
}

#[test]
fn real_serde_inputs_outputs_and_boundaries_validate_with_strict_ajv2020() {
    let artifact = CommandSchemaArtifact::from_embedded().expect("embedded artifact loads offline");

    let mut schemas = BTreeMap::new();
    schemas.insert(
        "error_envelope".to_owned(),
        artifact
            .resolve_error_envelope_schema()
            .expect("resolve errorEnvelope"),
    );

    for name in [
        "get_profiles",
        "rpc_describe",
        "change_clash_core",
        "patch_app_config",
        "query_logs_summary",
        "create_profile",
        "delete_profile",
        "create_profile_folder",
        "get_service_status_summary",
        "get_configuration_status",
        "get_core_status",
        "get_sys_proxy",
        "get_http_server_status",
        "clash_api_get_group_delay",
    ] {
        schemas.insert(
            format!("{name}.input"),
            artifact
                .resolve_input_schema(name)
                .unwrap_or_else(|e| panic!("resolve {name}.input: {e}")),
        );
        schemas.insert(
            format!("{name}.output"),
            artifact
                .resolve_output_schema(name)
                .unwrap_or_else(|e| panic!("resolve {name}.output: {e}")),
        );
    }

    let mut cases = Vec::new();

    // 1. Zero-param commands: accept null & empty {}, reject unknown keys / array / primitive.
    cases.push(case_ok(
        "zero-param-null",
        "get_profiles.input",
        Value::Null,
    ));
    cases.push(case_ok(
        "zero-param-empty-obj",
        "get_profiles.input",
        json!({}),
    ));
    cases.push(case_err(
        "zero-param-reject-extra",
        "get_profiles.input",
        json!({ "unexpected": 1 }),
    ));
    cases.push(case_err(
        "zero-param-reject-array",
        "get_profiles.input",
        json!([]),
    ));
    cases.push(case_ok(
        "profiles-default-serialize",
        "get_profiles.output",
        serde_json::to_value(Profiles::default()).expect("Profiles serializes"),
    ));

    // 2. ClashCore Deserialize vs Serialize aliases and camelCase parameter wrapper.
    assert!(serde_json::from_value::<ClashCore>(json!("clash-meta")).is_ok());
    assert!(serde_json::from_value::<ClashCore>(json!("clash-premium")).is_ok());
    cases.push(case_ok(
        "change-core-canonical",
        "change_clash_core.input",
        json!({ "clashCore": "mihomo" }),
    ));
    cases.push(case_ok(
        "change-core-deserialize-alias",
        "change_clash_core.input",
        json!({ "clashCore": "clash-meta" }),
    ));
    cases.push(case_ok(
        "change-core-open-unknown-param-compat",
        "change_clash_core.input",
        json!({ "clashCore": "mihomo", "extraUnknown": true }),
    ));
    cases.push(case_err(
        "change-core-reject-snake-case-param",
        "change_clash_core.input",
        json!({ "clash_core": "mihomo" }),
    ));
    cases.push(case_err(
        "change-core-reject-invalid-variant",
        "change_clash_core.input",
        json!({ "clashCore": "unknown-core" }),
    ));
    cases.push(case_err(
        "change-core-reject-wrong-type",
        "change_clash_core.input",
        json!({ "clashCore": 42 }),
    ));
    cases.push(case_ok(
        "change-core-output-null",
        "change_clash_core.output",
        Value::Null,
    ));
    cases.push(case_err(
        "change-core-output-non-null",
        "change_clash_core.output",
        json!({}),
    ));

    // 3. Open patch unknown-key compatibility + double-option clear + bad type rejection +
    // privileged field server refusal (structural schema stays open on unrelated unknown keys).
    let empty_patch = ApplicationPreferencesPatch::default();
    let patch_json = json!({
        "patch": {
            "release_channel": null,
            "unknownCompatKey": "ignored-by-non-deny-patch"
        }
    });
    let decoded_open: NyanpasuAppConfigPatch =
        serde_json::from_value(patch_json["patch"].clone()).unwrap();
    assert!(ApplicationPreferencesPatch::try_from(decoded_open).is_ok());
    let privileged_patch: NyanpasuAppConfigPatch =
        serde_json::from_value(json!({ "hotkeys": [] })).unwrap();
    assert!(
        ApplicationPreferencesPatch::try_from(privileged_patch).is_err(),
        "server TryFrom must reject privileged hotkeys even though structural schema allows open unknown keys"
    );
    cases.push(case_ok(
        "app-patch-default-valid",
        "patch_app_config.input",
        json!({ "patch": serde_json::to_value(&empty_patch).unwrap() }),
    ));
    cases.push(case_ok(
        "app-patch-open-unknown-compat",
        "patch_app_config.input",
        patch_json,
    ));
    cases.push(case_ok(
        "app-patch-explicit-channel",
        "patch_app_config.input",
        json!({ "patch": { "release_channel": ReleaseChannel::Stable } }),
    ));
    cases.push(case_err(
        "app-patch-bad-field-type",
        "patch_app_config.input",
        json!({ "patch": { "proxy_layout_column": "not-an-integer" } }),
    ));

    // 4. Honest deny_unknown_fields boundary check: QueryLogSummary rejects unknown fields in Serde,
    // while structural v0 schema intentionally lacks additionalProperties:false due to specta 2.0.0-rc.25.
    let valid_query_logs = QueryLogSummary {
        session: "sess_1".to_owned(),
        filter: LogFilterSummary {
            levels: vec![Level::Info],
            from_ms: Some(10),
            to_ms: None,
        },
        direction: Direction::Latest,
        cursor: None,
        limit: 50,
    };
    let valid_query_val = serde_json::to_value(&valid_query_logs).unwrap();
    let mut query_with_extra = valid_query_val.clone();
    query_with_extra
        .as_object_mut()
        .unwrap()
        .insert("extra".to_owned(), json!(1));
    assert!(
        serde_json::from_value::<QueryLogSummary>(query_with_extra).is_err(),
        "real Serde decoder rejects unknown keys on #[serde(deny_unknown_fields)] QueryLogSummary"
    );
    assert!(
        artifact
            .defs
            .get("m.nyanpasu_core.client.logs.t.QueryLogSummary.de")
            .is_some_and(|def| def.get("additionalProperties").is_none()),
        "pinned specta 2.0.0-rc.25 omits deny_unknown_fields metadata; structural schema remains open"
    );
    cases.push(case_ok(
        "query-logs-summary-valid",
        "query_logs_summary.input",
        json!({
            "source": LogSource::App,
            "request": valid_query_val,
        }),
    ));
    cases.push(case_err(
        "query-logs-summary-missing-required",
        "query_logs_summary.input",
        json!({
            "source": LogSource::App,
            "request": { "session": "s" },
        }),
    ));

    // 5. Profile & folder requests + Full/Public MutationOutcomes.
    let create_req = CreateProfileRequest {
        request: NewProfileRequest {
            metadata: ProfileMetadata {
                name: "demo".to_owned(),
                desc: None,
                custom_name: true,
            },
            definition: ProfileDefinition::Config {
                config: ConfigDefinition::File(FileConfig {
                    source: ProfileSource::Local {
                        binding: LocalBinding::Managed {
                            materialized: MaterializedFile {
                                file: ManagedProfilePath::new("demo.yaml").unwrap(),
                                updated_at: None,
                            },
                        },
                    },
                    transforms: vec![],
                }),
            },
            folder: Some(FolderId("folder_1".to_owned())),
        },
        file_data: Some("port: 7890\n".to_owned()),
    };
    cases.push(case_ok(
        "create-profile-request",
        "create_profile.input",
        serde_json::to_value(&create_req).unwrap(),
    ));
    cases.push(case_err(
        "create-profile-missing-request",
        "create_profile.input",
        json!({ "fileData": "x" }),
    ));

    let delete_req = DeleteProfileRequest {
        uid: ProfileId("prof_1".to_owned()),
    };
    cases.push(case_ok(
        "delete-profile-request",
        "delete_profile.input",
        serde_json::to_value(&delete_req).unwrap(),
    ));
    cases.push(case_err(
        "delete-profile-bad-uid-type",
        "delete_profile.input",
        json!({ "uid": 123 }),
    ));

    let unit_full: RpcUnitMutationOutcome = MutationOutcome::Committed {
        value: (),
        commits: vec![CommitReceipt {
            operation_id: None,
            domain: "profiles".to_owned(),
            source_version: 1,
            runtime: RuntimeCommitStatus::Applied,
        }],
        notifications_pending: false,
    }
    .into();
    let unit_public: RpcUnitMutationOutcome =
        RpcMutationOutcome::Public(PublicMutationOutcome::CommittedDegraded {
            value: (),
            commits: vec![],
            notifications_pending: true,
            degradations: vec![PublicDegradation {
                phase: DegradationPhase::RuntimeApply,
                reason: PublicDegradationReason::CleanupDeferred,
                retryable: true,
            }],
        })
        .into();
    let profile_full: RpcProfileMutationOutcome = MutationOutcome::CommittedDegraded {
        value: ProfileId("p1".to_owned()),
        commits: vec![],
        notifications_pending: false,
        degradations: vec![Degradation {
            phase: DegradationPhase::UiEffect,
            reason: DegradationReason::CleanupDeferred,
            message: "cleanup pending".to_owned(),
            retryable: true,
        }],
    }
    .into();
    let folder_full: RpcFolderMutationOutcome = MutationOutcome::Committed {
        value: FolderId("f1".to_owned()),
        commits: vec![],
        notifications_pending: false,
    }
    .into();
    cases.push(case_ok(
        "unit-outcome-full",
        "delete_profile.output",
        serde_json::to_value(&unit_full).unwrap(),
    ));
    cases.push(case_ok(
        "unit-outcome-public-degraded",
        "delete_profile.output",
        serde_json::to_value(&unit_public).unwrap(),
    ));
    cases.push(case_err(
        "unit-outcome-reject-legacy-ok-tag",
        "delete_profile.output",
        json!({ "status": "ok", "value": null }),
    ));
    cases.push(case_ok(
        "profile-outcome-full-degraded",
        "create_profile.output",
        serde_json::to_value(&profile_full).unwrap(),
    ));
    cases.push(case_ok(
        "folder-outcome-full",
        "create_profile_folder.output",
        serde_json::to_value(&folder_full).unwrap(),
    ));

    // 6. Status/SysProxy/HttpServer/Describe/GroupDelay/RpcError corpus.
    let service_status = ServiceStatusSummary {
        name: "nyanpasu-service".to_owned(),
        version: "2.0.0".to_owned(),
        status: ServiceStatus::Running,
        server_reported: true,
        phase: ServicePhase::Ready,
        compat: ServiceCompatSummary::Compatible {
            server_version: "2.0.0".to_owned(),
        },
        restart_attempts: 0,
    };
    let config_status = ConfigurationStatus {
        event_seq: 3,
        maintenance: None,
        source_versions: SourceVersions {
            application: 1,
            clash: 2,
            session: 3,
            profiles: 4,
        },
        runtime: RuntimeConvergence {
            health: ConvergenceHealth::Healthy,
            operation_id: None,
            attempts: 0,
            automatic_remaining: 0,
            message: None,
        },
        effects: vec![],
        sources: vec![],
        active: Some("p1".to_owned()),
        recent_operations: vec![],
    };
    let core_status = CoreStatusInfo {
        controller: None,
        host: ExecutionHost::Local,
        connectivity: EndpointConnectivity::Connected,
        generation: 7,
        state: None,
        state_changed_at: 1_700_000_000,
        revision: None,
        healthy: Some(true),
    };
    let sys_proxy = GetSysProxyResponse {
        enable: true,
        host: "127.0.0.1".to_owned(),
        port: 7890,
        bypass: "localhost".to_owned(),
        server: "127.0.0.1:7890".to_owned(),
    };
    let http_status = HttpServerStatus {
        enabled: true,
        url: Some("http://127.0.0.1:41234/".to_owned()),
        port: Some(41234),
        fault: None,
    };
    let describe = RpcDescribeResponse {
        protocol: 1,
        server: ServerInfo::desktop("2.0.0"),
        transport: RpcTarget::Http,
        principal: Principal::Token {
            id: "tok_1".to_owned(),
            name: "cli".to_owned(),
        },
        scopes: ScopeSet::from([Scope::Read]),
        commands: vec![DescribedCommand {
            name: "get_profiles".to_owned(),
            effect: CommandEffect::Query,
            scope: Scope::Read,
            allowed: true,
        }],
    };
    let err_envelope = RpcError {
        kind: "invalid_params",
        message: "bad input".to_owned(),
        code: Some("invalid_params".to_owned()),
        retryable: Some(false),
        operation_id: Some("op_1".to_owned()),
        domain_error: None,
    };
    let group_delay: HashMap<String, u16> = HashMap::from([("ProxyA".to_owned(), 120)]);

    cases.push(case_ok(
        "service-status-summary",
        "get_service_status_summary.output",
        serde_json::to_value(&service_status).unwrap(),
    ));
    cases.push(case_ok(
        "configuration-status",
        "get_configuration_status.output",
        serde_json::to_value(&config_status).unwrap(),
    ));
    cases.push(case_ok(
        "core-status-info",
        "get_core_status.output",
        serde_json::to_value(&core_status).unwrap(),
    ));
    cases.push(case_ok(
        "sys-proxy-response",
        "get_sys_proxy.output",
        serde_json::to_value(&sys_proxy).unwrap(),
    ));
    cases.push(case_err(
        "sys-proxy-negative-port",
        "get_sys_proxy.output",
        json!({ "enable": true, "host": "127.0.0.1", "port": -1, "bypass": "", "server": "" }),
    ));
    cases.push(case_ok(
        "http-server-status",
        "get_http_server_status.output",
        serde_json::to_value(&http_status).unwrap(),
    ));
    cases.push(case_ok(
        "rpc-describe-response",
        "rpc_describe.output",
        serde_json::to_value(&describe).unwrap(),
    ));
    cases.push(case_ok(
        "rpc-error-envelope",
        "error_envelope",
        serde_json::to_value(&err_envelope).unwrap(),
    ));
    cases.push(case_err(
        "rpc-error-envelope-missing-kind",
        "error_envelope",
        json!({ "message": "oops" }),
    ));
    cases.push(case_ok(
        "group-delay-map",
        "clash_api_get_group_delay.output",
        serde_json::to_value(&group_delay).unwrap(),
    ));
    cases.push(case_err(
        "group-delay-map-string-value-rejected",
        "clash_api_get_group_delay.output",
        json!({ "ProxyA": "fast" }),
    ));

    run_ajv_batch(&BatchInput { schemas, cases });
}

#[test]
fn edge_types_and_custom_wire_rules_validate_with_strict_ajv2020() {
    #[derive(Serialize, Deserialize, specta::Type)]
    #[allow(dead_code)]
    struct WithDefaultAndOptional {
        required_flag: bool,
        #[serde(default)]
        with_default: u8,
        optional_note: Option<String>,
        #[serde(rename = "$ref")]
        ref_field: char,
    }

    #[derive(Serialize, specta::Type)]
    struct BaseFlattened {
        base_id: String,
    }

    #[derive(Serialize, specta::Type)]
    #[allow(dead_code)]
    struct WithFlatten {
        #[serde(flatten)]
        base: BaseFlattened,
        extra: i64,
        unsigned_big: u64,
    }

    #[derive(Serialize, specta::Type)]
    #[serde(tag = "kind", content = "data", rename_all = "snake_case")]
    #[allow(dead_code)]
    enum AdjacentTagged {
        Ping,
        Count(u32),
    }

    #[derive(Serialize, Deserialize, specta::Type, PartialEq, Eq, PartialOrd, Ord, Hash)]
    #[serde(rename_all = "snake_case")]
    enum KeyChoice {
        Alpha,
        Beta,
    }

    #[derive(Serialize, specta::Type)]
    #[allow(dead_code)]
    struct MapContainer {
        chars: BTreeMap<char, bool>,
        ints: BTreeMap<i32, bool>,
        choices: BTreeMap<KeyChoice, bool>,
    }

    let mut types = Types::default();
    let ser_default_dt = WithDefaultAndOptional::definition(&mut types);
    let de_default_dt = WithDefaultAndOptional::definition(&mut types);
    let flatten_dt = WithFlatten::definition(&mut types);
    let enum_dt = AdjacentTagged::definition(&mut types);
    let map_dt = MapContainer::definition(&mut types);
    let update_dt = RpcAppUpdateOutcome::definition(&mut types);
    let core_ser_dt = ClashCore::definition(&mut types);

    let mut schemas = BTreeMap::new();
    schemas.insert(
        "with_default_ser".to_owned(),
        crate::specta_json_schema::export_probe_schema(&ser_default_dt, &types, Phase::Serialize)
            .unwrap(),
    );
    schemas.insert(
        "with_default_de".to_owned(),
        crate::specta_json_schema::export_probe_schema(&de_default_dt, &types, Phase::Deserialize)
            .unwrap(),
    );
    schemas.insert(
        "with_flatten".to_owned(),
        crate::specta_json_schema::export_probe_schema(&flatten_dt, &types, Phase::Serialize)
            .unwrap(),
    );
    schemas.insert(
        "adjacent_enum".to_owned(),
        crate::specta_json_schema::export_probe_schema(&enum_dt, &types, Phase::Serialize).unwrap(),
    );
    schemas.insert(
        "map_container".to_owned(),
        crate::specta_json_schema::export_probe_schema(&map_dt, &types, Phase::Serialize).unwrap(),
    );
    schemas.insert(
        "app_update_outcome".to_owned(),
        crate::specta_json_schema::export_probe_schema(&update_dt, &types, Phase::Serialize)
            .unwrap(),
    );
    schemas.insert(
        "clash_core_ser".to_owned(),
        crate::specta_json_schema::export_probe_schema(&core_ser_dt, &types, Phase::Serialize)
            .unwrap(),
    );

    let update_summary: RpcAppUpdateOutcome = RpcAppUpdateOutcome::Public(AppUpdateSummary {
        revision: 2,
        phase: AppUpdatePhase::Available,
        release: Some(AppUpdateReleaseSummary {
            version: "2.1.0".to_owned(),
            date: Some("2026-11-14".to_owned()),
        }),
        downloaded: 0,
        total: Some(1024),
        speed: 0.0,
        source: None,
        has_error: false,
        last_checked_at: None,
        supported: true,
    });

    let cases = vec![
        // Phase Serialize vs Deserialize on #[serde(default)] + renamed "$ref" char field:
        case_ok(
            "de-allows-default-and-omitted-option",
            "with_default_de",
            json!({ "required_flag": true, "$ref": "😀" }),
        ),
        case_err(
            "ser-requires-with-default-field",
            "with_default_ser",
            json!({ "required_flag": true, "$ref": "a" }),
        ),
        case_err(
            "char-field-rejects-two-chars",
            "with_default_de",
            json!({ "required_flag": true, "$ref": "ab" }),
        ),
        // ClashCore Serialize omits Deserialize alias "clash-meta":
        case_ok(
            "clash-core-ser-canonical",
            "clash_core_ser",
            json!("mihomo"),
        ),
        case_err(
            "clash-core-ser-rejects-de-alias",
            "clash_core_ser",
            json!("clash-meta"),
        ),
        // Flatten + i64/u64:
        case_ok(
            "flatten-valid",
            "with_flatten",
            json!({ "base_id": "id1", "extra": -5, "unsigned_big": 9007199254740991_u64 }),
        ),
        case_err(
            "flatten-rejects-negative-u64",
            "with_flatten",
            json!({ "base_id": "id1", "extra": 0, "unsigned_big": -1 }),
        ),
        // Adjacent-tagged enum:
        case_ok("enum-ping", "adjacent_enum", json!({ "kind": "ping" })),
        case_ok(
            "enum-count",
            "adjacent_enum",
            json!({ "kind": "count", "data": 5 }),
        ),
        case_err(
            "enum-bad-count-type",
            "adjacent_enum",
            json!({ "kind": "count", "data": "five" }),
        ),
        // Map keys (char, bounded i32 decimal regex, string enum):
        case_ok(
            "maps-all-valid",
            "map_container",
            json!({
                "chars": { "a": true, "😀": false },
                "ints": { "0": true, "-2147483648": false, "2147483647": true },
                "choices": { "alpha": true, "beta": false }
            }),
        ),
        case_err(
            "map-char-empty-key-rejected",
            "map_container",
            json!({ "chars": { "": true }, "ints": {}, "choices": {} }),
        ),
        case_err(
            "map-char-multi-key-rejected",
            "map_container",
            json!({ "chars": { "ab": true }, "ints": {}, "choices": {} }),
        ),
        case_err(
            "map-i32-neg-zero-rejected",
            "map_container",
            json!({ "chars": {}, "ints": { "-0": true }, "choices": {} }),
        ),
        case_err(
            "map-i32-leading-zero-rejected",
            "map_container",
            json!({ "chars": {}, "ints": { "01": true }, "choices": {} }),
        ),
        case_err(
            "map-i32-plus-sign-rejected",
            "map_container",
            json!({ "chars": {}, "ints": { "+1": true }, "choices": {} }),
        ),
        case_err(
            "map-i32-overflow-rejected",
            "map_container",
            json!({ "chars": {}, "ints": { "2147483648": true }, "choices": {} }),
        ),
        case_err(
            "map-i32-underflow-rejected",
            "map_container",
            json!({ "chars": {}, "ints": { "-2147483649": true }, "choices": {} }),
        ),
        case_err(
            "map-enum-unknown-key-rejected",
            "map_container",
            json!({ "chars": {}, "ints": {}, "choices": { "gamma": true } }),
        ),
        // RpcAppUpdateOutcome untagged union:
        case_ok(
            "app-update-summary-valid",
            "app_update_outcome",
            serde_json::to_value(&update_summary).unwrap(),
        ),
    ];

    run_ajv_batch(&BatchInput { schemas, cases });
}

#[test]
fn all_exported_command_schemas_compile_under_strict_ajv2020_via_rust_resolver() {
    let artifact = CommandSchemaArtifact::from_embedded().expect("embedded artifact loads offline");
    let mut schemas = BTreeMap::new();
    schemas.insert(
        "error_envelope".to_owned(),
        artifact
            .resolve_error_envelope_schema()
            .expect("errorEnvelope resolves"),
    );
    for name in artifact.command_names() {
        schemas.insert(
            format!("{name}.input"),
            artifact
                .resolve_input_schema(name)
                .unwrap_or_else(|e| panic!("resolve {name}.input: {e}")),
        );
        schemas.insert(
            format!("{name}.output"),
            artifact
                .resolve_output_schema(name)
                .unwrap_or_else(|e| panic!("resolve {name}.output: {e}")),
        );
    }

    let out = run_ajv_batch(&BatchInput {
        schemas,
        cases: vec![],
    });
    assert_eq!(out.total, 0);
}
