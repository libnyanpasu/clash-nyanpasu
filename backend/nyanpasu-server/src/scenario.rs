//! The spike scenario: the OpenWrt MVP loop driven through `NyanpasuClient`.
//!
//! Every step prints one JSON line to stdout: `{"run", "step", "ok", "detail"}`.
//! `ok` states whether the step met its expectation, which is not always success:
//! the negative case expects a rejection or a degradation.

use std::{
    cell::Cell,
    time::{Duration, Instant},
};

use anyhow::{Context as _, bail};
use nyanpasu_config::profile::{
    ConfigDefinition, FileConfig, LocalBinding, ManagedProfilePath, MaterializedFile,
    ProfileDefinition, ProfileId, ProfileMetadata, ProfileSource, ScriptRuntime, ScriptTransform,
    TransformDefinition,
};
use nyanpasu_core::{
    actor_v2::CoreStatusProjection,
    client::{NyanpasuClient, runtime::MutationOutcome},
    state::profiles::NewProfileRequest,
};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::broadcast;

use crate::compose::{Server, ServerDirs};

const RUNNING_TIMEOUT: Duration = Duration::from_secs(30);
const STOPPED_TIMEOUT: Duration = Duration::from_secs(15);
const DIRS_MARKER: &str = ".nyanpasu-server-spike";
/// Left in place by `--fresh`: the core binary the data directory is provisioned with.
const KEPT_FILES: &[&str] = &["mihomo"];

const BASE_PROFILE: &str = "\
mixed-port: 17890
allow-lan: false
bind-address: 127.0.0.1
mode: rule
log-level: info
proxies: []
proxy-groups: []
rules:
  - MATCH,DIRECT
";

const JS_MARKER: &str = "JS-MARKER";
const LUA_MARKER: &str = "LUA-MARKER";

const JS_TRANSFORM: &str = "\
function main(config) {
  config['proxy-groups'] = config['proxy-groups'] || [];
  config['proxy-groups'].push({ name: 'JS-MARKER', type: 'select', proxies: ['DIRECT'] });
  return config;
}
";

const LUA_TRANSFORM: &str = "\
config['proxy-groups'] = config['proxy-groups'] or {}
table.insert(config['proxy-groups'], { name = 'LUA-MARKER', type = 'select', proxies = { 'DIRECT' } })
return config
";

const THROWING_TRANSFORM: &str = "\
function main(config) {
  throw new Error('spike: deliberate failure');
}
";

/// What to run.
#[derive(Debug, Clone)]
pub struct Options {
    pub dirs: ServerDirs,
    /// Start from empty directories, keeping the core binary.
    pub fresh: bool,
    /// Attach the JavaScript and Lua transforms and run the negative case.
    pub scripts: bool,
    /// Stop after the core is running and wait for a signal, for a crash test.
    pub hold: bool,
}

/// Runs the scenario and returns how many steps missed their expectation.
///
/// # Errors
/// Returns an error when the directories cannot be prepared or the application
/// graph cannot be built; every later failure is reported as a failed step.
pub async fn run(options: Options) -> anyhow::Result<u32> {
    if options.fresh {
        reset_dirs(&options.dirs)?;
    } else {
        mark_dirs(&options.dirs)?;
    }
    let run = if options.fresh { "1" } else { "2" };
    let reporter = Reporter::new(run);
    let started = Instant::now();
    let server = match Server::start(options.dirs.clone()).await {
        Ok(server) => server,
        Err(error) => {
            reporter.fail("build_client", json!({ "error": format!("{error:#}") }));
            return Ok(reporter.failures());
        }
    };
    reporter.ok(
        "build_client",
        json!({ "millis": started.elapsed().as_millis() }),
    );
    let client = server.client().clone();
    if options.fresh {
        fresh_loop(&client, &options, &reporter).await;
    } else {
        resumed_loop(&client, &reporter).await;
    }
    reporter.ok("resources_final", json!({ "server": memory("self") }));
    let core_pid = core_pid(&client.core_status());
    server.shutdown().await;
    reporter.check(
        "shutdown",
        core_pid.is_none_or(|pid| pid_alive(pid) != Some(true)),
        json!({ "core_pid": core_pid, "core_pid_alive": core_pid.and_then(pid_alive) }),
    );
    Ok(reporter.failures())
}

async fn fresh_loop(client: &NyanpasuClient, options: &Options, reporter: &Reporter) {
    startup(client, reporter).await;
    reporter.result(
        "start_background_sources",
        &client.start_background_sources(),
    );

    let Some(config) = create(
        client,
        reporter,
        "create_config_profile",
        "Spike",
        config_definition(),
        BASE_PROFILE,
    )
    .await
    else {
        return;
    };
    if options.scripts {
        let Some(js) = create(
            client,
            reporter,
            "create_js_transform",
            "JS marker",
            transform_definition(ScriptRuntime::JavaScript, "pending.js"),
            JS_TRANSFORM,
        )
        .await
        else {
            return;
        };
        let Some(lua) = create(
            client,
            reporter,
            "create_lua_transform",
            "Lua marker",
            transform_definition(ScriptRuntime::Lua, "pending.lua"),
            LUA_TRANSFORM,
        )
        .await
        else {
            return;
        };
        attach(client, reporter, &config, [js.clone(), lua]).await;
        activate_and_run(client, reporter, &config).await;
        if options.hold {
            return hold(reporter).await;
        }
        negative_case(client, reporter, &js).await;
    } else {
        activate_and_run(client, reporter, &config).await;
        if options.hold {
            return hold(reporter).await;
        }
    }
    stop(client, reporter).await;
}

async fn resumed_loop(client: &NyanpasuClient, reporter: &Reporter) {
    startup(client, reporter).await;
    reporter.ok("profiles_retained", profiles_summary(client).await);
    runtime_yaml(client, reporter, "runtime_yaml_after_startup").await;
    reporter.ok(
        "core_status_after_startup",
        status_json(&client.core_status()),
    );
    reconcile(client, reporter).await;
    runtime_yaml(client, reporter, "runtime_yaml_after_reconcile").await;
}

async fn startup(client: &NyanpasuClient, reporter: &Reporter) {
    let report = client.startup_reconcile().await;
    reporter.ok(
        "startup_reconcile",
        json!({
            "outcome": format!("{:?}", report.outcome),
            "observation": format!("{:?}", report.observation),
        }),
    );
}

async fn create(
    client: &NyanpasuClient,
    reporter: &Reporter,
    step: &str,
    name: &str,
    definition: ProfileDefinition,
    content: &str,
) -> Option<ProfileId> {
    let request = NewProfileRequest {
        metadata: ProfileMetadata {
            name: name.to_owned(),
            desc: None,
            custom_name: true,
        },
        definition,
        folder: None,
    };
    let result = client
        .create_profile(request, Some(content.to_owned()))
        .await;
    reporter.mutation(step, &result);
    result.ok().map(MutationOutcome::into_value)
}

async fn attach(
    client: &NyanpasuClient,
    reporter: &Reporter,
    config: &ProfileId,
    transforms: [ProfileId; 2],
) {
    let profiles = match client.get_profiles().await {
        Ok(profiles) => profiles,
        Err(error) => return reporter.fail("attach_transforms", error_json(&error)),
    };
    let Some(item) = profiles.items.get(config) else {
        return reporter.fail("attach_transforms", json!({ "error": "profile missing" }));
    };
    let mut definition = item.definition.clone();
    if let ProfileDefinition::Config { config } = &mut definition {
        *config.transforms_mut() = transforms.to_vec();
    }
    let result = client
        .replace_profile_definition(config.clone(), definition)
        .await;
    reporter.mutation("attach_transforms", &result);
}

async fn activate_and_run(client: &NyanpasuClient, reporter: &Reporter, config: &ProfileId) {
    let result = client.activate_profile(Some(config.clone())).await;
    reporter.mutation("activate_profile", &result);
    reconcile(client, reporter).await;
    runtime_yaml(client, reporter, "runtime_yaml").await;
}

/// Starts or applies the core and waits for `Running` without sleeping.
async fn reconcile(client: &NyanpasuClient, reporter: &Reporter) {
    let mut events = client.subscribe_core_events();
    let result = client.reconcile_core().await;
    match &result {
        Ok(report) => reporter.ok(
            "reconcile_core",
            json!({
                "status": status_json(&report.status),
                "applied": format!("{:?}", report.applied),
            }),
        ),
        Err(error) => reporter.fail("reconcile_core", error_json(error)),
    }
    let waited = wait_for_core(client, &mut events, "Running", RUNNING_TIMEOUT).await;
    match waited {
        Ok(status) => {
            let pid = core_pid(&status);
            reporter.check(
                "core_running",
                pid.is_some(),
                json!({
                    "status": status_json(&status),
                    "core_pid_alive": pid.and_then(pid_alive),
                }),
            );
            if let Some(pid) = pid {
                reporter.ok(
                    "resources",
                    json!({ "server": memory("self"), "core": memory(&pid.to_string()) }),
                );
            }
        }
        Err(error) => reporter.fail("core_running", json!({ "error": error })),
    }
}

async fn runtime_yaml(client: &NyanpasuClient, reporter: &Reporter, step: &str) {
    match client.runtime_yaml().await {
        Ok(yaml) => {
            let js = yaml.find(JS_MARKER);
            let lua = yaml.find(LUA_MARKER);
            reporter.ok(
                step,
                json!({
                    "bytes": yaml.len(),
                    "js_marker_at": js,
                    "lua_marker_at": lua,
                    "js_before_lua": js.zip(lua).map(|(js, lua)| js < lua),
                    "yaml": yaml,
                }),
            );
        }
        Err(error) => reporter.fail(step, error_json(&error)),
    }
}

/// Replaces the JavaScript transform's source with a script that throws.
async fn negative_case(client: &NyanpasuClient, reporter: &Reporter, js: &ProfileId) {
    let before_yaml = client.runtime_yaml().await.ok();
    let before_file = client.read_profile_file(js.clone()).await.ok();
    let before_current = current_profile(client).await;
    let before_core = client.core_status();

    let result = client
        .save_profile_file(js.clone(), THROWING_TRANSFORM.to_owned())
        .await;
    let classification = match &result {
        Err(_) => "rejected",
        Ok(MutationOutcome::Committed { .. }) => "committed",
        Ok(MutationOutcome::CommittedDegraded { .. }) => "committed_degraded",
    };
    let after_file = client.read_profile_file(js.clone()).await.ok();
    let after_yaml = client.runtime_yaml().await.ok();
    let after_core = client.core_status();
    let result_json = match &result {
        Ok(outcome) => to_json(outcome),
        Err(error) => error_json(error),
    };
    reporter.ok(
        "negative_throwing_script",
        json!({
            "classification": classification,
            "result": result_json,
            "file_unchanged": before_file.as_ref().zip(after_file.as_ref()).map(|(a, b)| a == b),
            "file_now_throws": after_file.as_deref().map(|file| file.contains("deliberate failure")),
            "runtime_yaml_unchanged": before_yaml.as_ref().zip(after_yaml.as_ref()).map(|(a, b)| a == b),
            "selection_preserved": before_current == current_profile(client).await,
            "selection": before_current,
            "core_before": status_json(&before_core),
            "core_after": status_json(&after_core),
            "core_pid_unchanged": core_pid(&before_core) == core_pid(&after_core),
        }),
    );
    let status = client.configuration_status();
    reporter.ok("configuration_status_after_negative", to_json(&status));
}

async fn stop(client: &NyanpasuClient, reporter: &Reporter) {
    let mut events = client.subscribe_core_events();
    let result = client.stop_core().await;
    match &result {
        Ok(report) => reporter.ok(
            "stop_core",
            json!({ "status": status_json(&report.status) }),
        ),
        Err(error) => reporter.fail("stop_core", error_json(error)),
    }
    match wait_for_core(client, &mut events, "Stopped", STOPPED_TIMEOUT).await {
        Ok(status) => reporter.ok("core_stopped", json!({ "status": status_json(&status) })),
        Err(error) => reporter.fail("core_stopped", json!({ "error": error })),
    }
}

async fn hold(reporter: &Reporter) {
    reporter.ok("holding", json!({ "pid": std::process::id() }));
    if let Err(error) = tokio::signal::ctrl_c().await {
        reporter.fail("holding", json!({ "error": error.to_string() }));
    }
}

/// Waits until the core reports `kind`, checking the current status first so an
/// event sent before the subscription cannot be missed.
async fn wait_for_core(
    client: &NyanpasuClient,
    events: &mut broadcast::Receiver<CoreStatusProjection>,
    kind: &str,
    timeout: Duration,
) -> Result<CoreStatusProjection, String> {
    let current = client.core_status();
    if core_state(&current).0 == kind {
        return Ok(current);
    }
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        match tokio::time::timeout_at(deadline, events.recv()).await {
            Err(_) => {
                let last = client.core_status();
                return Err(format!(
                    "timed out after {timeout:?} waiting for {kind}; last state {:?}",
                    core_state(&last).0
                ));
            }
            Ok(Ok(status)) if core_state(&status).0 == kind => return Ok(status),
            Ok(Ok(_) | Err(broadcast::error::RecvError::Lagged(_))) => {
                let status = client.core_status();
                if core_state(&status).0 == kind {
                    return Ok(status);
                }
            }
            Ok(Err(broadcast::error::RecvError::Closed)) => {
                return Err("the core event stream closed".to_owned());
            }
        }
    }
}

/// The state name and pid of the host's published snapshot.
fn core_state(status: &CoreStatusProjection) -> (String, Option<u64>) {
    let Some(state) = status
        .snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.state.as_ref())
    else {
        return ("Unknown".to_owned(), None);
    };
    let Ok(Value::Object(map)) = serde_json::to_value(state) else {
        return ("Unknown".to_owned(), None);
    };
    match map.into_iter().next() {
        Some((kind, body)) => (kind, body.get("pid").and_then(Value::as_u64)),
        None => ("Unknown".to_owned(), None),
    }
}

fn core_pid(status: &CoreStatusProjection) -> Option<u32> {
    core_state(status).1.and_then(|pid| u32::try_from(pid).ok())
}

fn status_json(status: &CoreStatusProjection) -> Value {
    let (state, pid) = core_state(status);
    json!({
        "state": state,
        "pid": pid,
        "connectivity": to_json(&status.connectivity),
        "healthy": status.snapshot.as_ref().and_then(|snapshot| snapshot.healthy),
    })
}

async fn current_profile(client: &NyanpasuClient) -> Option<ProfileId> {
    client
        .get_profiles()
        .await
        .ok()
        .and_then(|profiles| profiles.current.clone())
}

async fn profiles_summary(client: &NyanpasuClient) -> Value {
    let profiles = match client.get_profiles().await {
        Ok(profiles) => profiles,
        Err(error) => return error_json(&error),
    };
    let items: Vec<Value> = profiles
        .items
        .values()
        .map(|item| {
            let transforms = match &item.definition {
                ProfileDefinition::Config { config } => Some(config.transforms().to_vec()),
                ProfileDefinition::Transform { .. } => None,
            };
            json!({
                "uid": item.uid,
                "name": item.metadata.name,
                "category": item.definition.category(),
                "transforms": transforms,
            })
        })
        .collect();
    json!({
        "current": profiles.current,
        "global_transforms": profiles.global_transforms,
        "items": items,
    })
}

fn config_definition() -> ProfileDefinition {
    ProfileDefinition::Config {
        config: ConfigDefinition::File(FileConfig {
            source: managed_source("pending.yaml"),
            transforms: Vec::new(),
        }),
    }
}

fn transform_definition(runtime: ScriptRuntime, file: &str) -> ProfileDefinition {
    ProfileDefinition::Transform {
        transform: TransformDefinition::Script(ScriptTransform {
            source: managed_source(file),
            runtime,
        }),
    }
}

/// The client rewrites the placeholder path to `<uid>.<ext>` when it adds the profile.
fn managed_source(file: &str) -> ProfileSource {
    ProfileSource::Local {
        binding: LocalBinding::Managed {
            materialized: MaterializedFile {
                file: ManagedProfilePath::new(file)
                    .expect("a literal relative file name is a valid managed path"),
                updated_at: None,
            },
        },
    }
}

/// `VmHWM` and `VmRSS` in kB of `/proc/<pid>/status`; `null` where `/proc` is absent.
fn memory(pid: &str) -> Value {
    let Ok(status) = std::fs::read_to_string(format!("/proc/{pid}/status")) else {
        return Value::Null;
    };
    let field = |name: &str| {
        status
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .and_then(|rest| {
                rest.trim()
                    .trim_end_matches("kB")
                    .trim()
                    .parse::<u64>()
                    .ok()
            })
    };
    json!({ "vm_hwm_kb": field("VmHWM:"), "vm_rss_kb": field("VmRSS:") })
}

#[cfg(target_os = "linux")]
fn pid_alive(pid: u32) -> Option<bool> {
    Some(std::path::Path::new(&format!("/proc/{pid}")).exists())
}

#[cfg(not(target_os = "linux"))]
fn pid_alive(_pid: u32) -> Option<bool> {
    None
}

/// `--fresh` empties both directories, so it only acts on directories that are
/// empty (apart from the kept core binary) or that an earlier spike run marked.
fn reset_dirs(dirs: &ServerDirs) -> anyhow::Result<()> {
    for dir in [&dirs.config_dir, &dirs.data_dir] {
        let dir = dir.as_std_path();
        if dir.exists() {
            let marked = dir.join(DIRS_MARKER).exists();
            let empty = std::fs::read_dir(dir)
                .with_context(|| format!("read {}", dir.display()))?
                .all(|entry| {
                    entry
                        .is_ok_and(|entry| KEPT_FILES.iter().any(|kept| entry.file_name() == *kept))
                });
            if !marked && !empty {
                bail!(
                    "refusing to reset {}: it is not empty and has no {DIRS_MARKER} marker",
                    dir.display()
                );
            }
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                if KEPT_FILES.iter().any(|kept| entry.file_name() == *kept) {
                    continue;
                }
                let path = entry.path();
                if entry.file_type()?.is_dir() {
                    std::fs::remove_dir_all(&path)?;
                } else {
                    std::fs::remove_file(&path)?;
                }
            }
        }
    }
    mark_dirs(dirs)
}

fn mark_dirs(dirs: &ServerDirs) -> anyhow::Result<()> {
    for dir in [&dirs.config_dir, &dirs.data_dir] {
        std::fs::create_dir_all(dir)?;
        std::fs::write(dir.join(DIRS_MARKER), "")?;
    }
    Ok(())
}

fn to_json(value: &impl Serialize) -> Value {
    serde_json::to_value(value)
        .unwrap_or_else(|error| json!({ "unserializable": error.to_string() }))
}

/// A structured error: the `Display` chain and the `Debug` form that names the variants.
fn error_json(error: &(impl std::error::Error + 'static)) -> Value {
    let debug = format!("{error:?}");
    json!({
        "error": snafu::Report::from_error(error).to_string(),
        "debug": debug.chars().take(1500).collect::<String>(),
    })
}

struct Reporter {
    run: &'static str,
    failures: Cell<u32>,
}

impl Reporter {
    fn new(run: &'static str) -> Self {
        Self {
            run,
            failures: Cell::new(0),
        }
    }

    fn failures(&self) -> u32 {
        self.failures.get()
    }

    fn emit(&self, step: &str, ok: bool, detail: Value) {
        if !ok {
            self.failures.set(self.failures.get() + 1);
        }
        println!(
            "{}",
            json!({ "run": self.run, "step": step, "ok": ok, "detail": detail })
        );
    }

    fn ok(&self, step: &str, detail: Value) {
        self.emit(step, true, detail);
    }

    fn fail(&self, step: &str, detail: Value) {
        self.emit(step, false, detail);
    }

    fn check(&self, step: &str, ok: bool, detail: Value) {
        self.emit(step, ok, detail);
    }

    fn result<T: std::fmt::Debug, E: std::error::Error + 'static>(
        &self,
        step: &str,
        result: &Result<T, E>,
    ) {
        match result {
            Ok(value) => self.ok(step, json!({ "value": format!("{value:?}") })),
            Err(error) => self.fail(step, error_json(error)),
        }
    }

    /// A mutation meets its expectation only when it commits without degradation.
    fn mutation<T: Serialize, E: std::error::Error + 'static>(
        &self,
        step: &str,
        result: &Result<MutationOutcome<T>, E>,
    ) {
        match result {
            Ok(outcome @ MutationOutcome::Committed { .. }) => self.ok(step, to_json(outcome)),
            Ok(outcome @ MutationOutcome::CommittedDegraded { .. }) => {
                self.fail(step, to_json(outcome));
            }
            Err(error) => self.fail(step, error_json(error)),
        }
    }
}
