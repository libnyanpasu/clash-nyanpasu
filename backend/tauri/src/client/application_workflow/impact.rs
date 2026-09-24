//! Pure impact classification for one configuration mutation.
//!
//! Two questions that must not be collapsed into one:
//!
//! - does the candidate move a *runtime build input*, so the critical part of
//!   the mutation has to build, check and try it ([`RuntimeImpact`]);
//! - which *peripheral owners* were handed new inputs, so only those may be
//!   given a new desired target ([`ChangedOwnerInputs`]).
//!
//! Every input is a parameter and every output is data: nothing here reads
//! state, spawns work, or touches Tauri. The workflow composes the results.

use std::collections::{BTreeSet, HashSet};

use indexmap::IndexSet;
use nyanpasu_config::{
    application::{NyanpasuAppConfig, NyanpasuAppConfigPatch},
    clash::config::{
        ClashConfig, ClashConfigPatch, ClashControlChannel,
        clash_strategy::port::{ExternalControllerStrategy, PortStrategy},
        overrides::{ClashGuardOverrides, ClashGuardOverridesPatch},
        tun_stack::TunStack,
    },
    profile::{ManagedProfilePath, ProfileDefinition, ProfileId, Profiles},
};

use crate::{
    client::effects::plan::{
        ApplicationEffect, ApplicationEffectInputs, ApplicationEffectPlan, EffectKind,
    },
    state::profiles::ProfilesActor,
};

/// How much of the running core a candidate forces to change.
///
/// The variants are ordered by how much they disturb, so the verdict of a
/// mutation that spans domains is the `max` of theirs: a host switch restarts
/// the core with the freshly built config, which subsumes replacing the binary,
/// which subsumes reconciling it.
///
/// A rebuild and a control-channel change share one variant on purpose. Both
/// come out of the same candidate and go to one reconcile, which picks reload
/// or restart below the application layer (roadmap §6.2); this is where the
/// classification stops copying [`runtime_apply_kind`], whose split exists so
/// the facade can call one of two legacy entry points.
///
/// [`runtime_apply_kind`]: crate::client::effects::plan::runtime_apply_kind
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::client) enum RuntimeImpact {
    /// No build input and no control-channel input moved. There is nothing
    /// critical to try, and the mutation is a plain source-config save.
    None,
    /// A runtime build input and/or a control-channel input moved, so the
    /// generated config the core runs is no longer the committed one.
    Reconcile,
    /// The selected core binary changed: the running instance is replaced
    /// rather than reloaded.
    CoreSwap,
    /// The execution host changed: the core moves between the local process and
    /// the service host.
    HostSwitch,
}

/// What the request asked of the current profile selection.
///
/// Carried rather than derived: a pure document diff cannot tell "the user did
/// not touch the selection" from "the user asked for the profile that is
/// already selected", and the second still owes a one-shot action.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::client) enum ActivationIntent {
    /// The request does not address the current selection.
    #[default]
    None,
    /// `SetCurrent` / `SetCurrentIfNone`: run this profile.
    // Requested by the profiles actor, which moves onto the participant in T6.
    #[allow(dead_code)]
    Activate(ProfileId),
    /// Clear the current selection.
    #[allow(dead_code)]
    Deactivate,
}

/// Digest of the bytes a request stages for one managed path.
///
/// Opaque here: the classification only needs it to travel with the path it
/// belongs to, and the producer (the profiles domain) owns the algorithm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::client) struct ContentDigest(String);

// Produced by the profiles actor when it stages bytes for a managed path (T6).
#[allow(dead_code)]
impl ContentDigest {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A staged resource, addressed by the mutation that created it.
///
/// A config version cannot address it: two mutations of the same version can
/// each stage their own bytes for one path, so the handle is bound to the
/// request's `OperationId` (roadmap §4.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::client) struct StagedResourceToken(String);

// Produced by the profiles actor when it stages bytes for a managed path (T6).
#[allow(dead_code)]
impl StagedResourceToken {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Managed content a request replaces.
///
/// File bytes live outside the profiles document, so comparing two documents
/// cannot see that the same path now holds different content. A subscription
/// refresh that rewrites the selected profile's file is exactly that case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::client) struct TouchedContent {
    /// The managed path whose bytes this request replaces.
    pub path: ManagedProfilePath,
    /// Digest of the staged bytes, when the producer computed one.
    ///
    /// The classification does not read it: a touched path inside the current
    /// closure counts as changed either way, which over-counts an identical
    /// rewrite and never under-counts a real one. It travels with the path so
    /// the Try can pin the content set it built against.
    pub content_digest: Option<ContentDigest>,
    /// The staged bytes themselves, when the request pre-staged them.
    pub resource: Option<StagedResourceToken>,
}

/// The runtime-relevant fields one request explicitly named.
///
/// "Named" is struct-patch presence and never a value comparison: a `Some`
/// field of a patch DTO is a field the request addressed, and it stays
/// evidence of that when the value it carries is the one already committed.
/// That is the point — resubmitting an outstanding runtime field unchanged
/// *is* a request for that runtime, and no comparison of the two documents can
/// see it (R15, §9.2).
///
/// The converse matters as much. A request that named no runtime-relevant
/// field asked the runtime for nothing, even when the document it produces is
/// byte-identical to the committed one. Reading the answer out of document
/// equality instead would re-evaluate an outstanding target on an unrelated
/// save, and would skip a genuine resubmission that happened to move an
/// unrelated field alongside it. Manual re-evaluation does not spend the
/// automatic apply budget.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::client) struct RequestedRuntimeFields {
    named: bool,
}

// The domain actors begin producing this intent in T6.
#[allow(dead_code)]
impl RequestedRuntimeFields {
    /// A request that replaced the whole document rather than patching it.
    pub fn whole_document() -> Self {
        Self { named: true }
    }

    pub fn runtime() -> Self {
        Self { named: true }
    }

    /// The runtime-relevant fields an application-config patch carries.
    pub fn of_application(patch: &NyanpasuAppConfigPatch) -> Self {
        Self {
            named: patch.enable_service_mode.is_some()
                || patch.core.is_some()
                || patch.enable_builtin_enhanced.is_some(),
        }
    }

    /// The runtime-relevant fields a clash-config patch carries.
    pub fn of_clash(patch: &ClashConfigPatch) -> Self {
        Self {
            named: patch.overrides.is_some()
                || patch.enable_clash_fields.is_some()
                || patch.enable_tun_mode.is_some()
                || patch.tun_stack.is_some()
                || patch.mixed_port.is_some()
                || patch.socks_port.is_some()
                || patch.http_port.is_some()
                || patch.external_controller.is_some()
                || patch.clash_control_channel.is_some()
                || patch.clash_ipc_disable_http_controller.is_some(),
        }
    }

    /// The clash overrides are patched through an entry point of their own, so
    /// a request that carries one names runtime exactly
    /// when that patch addresses a field.
    pub fn of_clash_overrides(patch: &ClashGuardOverridesPatch) -> Self {
        use struct_patch::Status as _;
        Self {
            named: !patch.is_empty(),
        }
    }

    /// Whether the request named a runtime-relevant field at all.
    pub fn names_any(&self) -> bool {
        self.named
    }
}

/// Typed request intent that the committed candidate cannot express.
///
/// A pure diff of the source config loses the difference between "the user did
/// not ask" and "the user asked for the value that is already stored". The
/// second still carries a command, and dropping it would silently change
/// behaviour that users depend on (roadmap C7/D6).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::client) struct MutationHints {
    /// The request carried a `mode` field, whether or not the value moved.
    ///
    /// This is the repeated-mode command semantics: re-submitting the current
    /// mode is a request for the one-shot connection interruption, not for a
    /// rebuild. It therefore feeds the action, never [`RuntimeImpact`].
    pub mode_requested: bool,
    /// What the request asked of the current profile selection.
    pub activation: ActivationIntent,
    /// Managed content this request replaces.
    pub touched: Vec<TouchedContent>,
    /// Which runtime-relevant fields the request explicitly named.
    pub requested: RequestedRuntimeFields,
}

impl MutationHints {
    /// Whether this request explicitly asked the runtime for something.
    ///
    /// This is the third re-evaluation trigger of §9.2: while a committed
    /// document is still not applied, a request that names one of its runtime
    /// fields again owes it a fresh attempt even though the diff is empty,
    /// and a request that names none of them must leave that target and its
    /// convergence budget exactly as it found them (R15).
    ///
    /// The profiles selection is read off [`MutationHints::activation`], which
    /// is the typed form a request expresses it in: "run this profile" names
    /// the field the build reads whether or not the profile is already the
    /// current one. Replaced managed bytes are deliberately *not* read here —
    /// content inside the current closure already moves
    /// [`classify_profiles`]'s own verdict, and content outside it is not a
    /// runtime input at all.
    pub fn names_runtime_field(&self) -> bool {
        self.requested.names_any() || self.activation != ActivationIntent::None
    }
}

/// Which application-config fields the runtime build and the core lifecycle
/// read.
///
/// - `enable_service_mode` is the execution host the lifecycle follows
///   (`core_lifecycle/workflow.rs`);
/// - `core` picks the builtin transform table, the tun flavor, and the binary
///   that gets started (`enhance/runtime_builder.rs`, `RuntimeBuildPort::core_spec`);
/// - `enable_builtin_enhanced` gates that transform table.
///
/// Every other field of [`NyanpasuAppConfig`] drives a peripheral owner or
/// nothing at all, which is what [`ChangedOwnerInputs`] reports instead.
#[derive(Debug, PartialEq, serde::Serialize)]
struct ApplicationRuntimeInputs<'a> {
    enable_service_mode: bool,
    core: &'a nyanpasu_config::application::ClashCore,
    enable_builtin_enhanced: bool,
}

impl<'a> ApplicationRuntimeInputs<'a> {
    fn of(app: &'a NyanpasuAppConfig) -> Self {
        Self {
            enable_service_mode: app.enable_service_mode,
            core: &app.core,
            enable_builtin_enhanced: app.enable_builtin_enhanced,
        }
    }
}

pub(in crate::client) fn classify_application(
    previous: &NyanpasuAppConfig,
    candidate: &NyanpasuAppConfig,
) -> RuntimeImpact {
    // Widest verdict first: a host switch restarts the core with the freshly
    // built config, so it subsumes both of the others.
    if previous.enable_service_mode != candidate.enable_service_mode {
        RuntimeImpact::HostSwitch
    } else if previous.core != candidate.core {
        RuntimeImpact::CoreSwap
    } else if previous.enable_builtin_enhanced != candidate.enable_builtin_enhanced {
        RuntimeImpact::Reconcile
    } else {
        RuntimeImpact::None
    }
}

/// Every clash-config field the runtime build or the control channel reads.
///
/// Enumerated from the real consumers rather than from a field whitelist:
/// `RuntimeBuilder::build` reads `overrides`, `enable_clash_fields`,
/// `enable_tun_mode` and `tun_stack`; `SessionPortResolver::resolve` turns the
/// four port strategies into the bindings written into the generated config;
/// and `RuntimePreparation::prepare` turns the two channel fields into the
/// core's local-IPC settings.
///
/// Deliberately absent: `web_ui_list` is a dashboard list no build stage reads,
/// and `break_connection` decides whether connections are interrupted *after* an
/// apply, so it is a property of the action and not an input of the config that
/// gets built.
#[derive(Debug, PartialEq, serde::Serialize)]
struct ClashRuntimeInputs<'a> {
    overrides: &'a ClashGuardOverrides,
    enable_clash_fields: bool,
    enable_tun_mode: bool,
    tun_stack: TunStack,
    mixed_port: &'a PortStrategy,
    socks_port: Option<&'a PortStrategy>,
    http_port: Option<&'a PortStrategy>,
    external_controller: &'a ExternalControllerStrategy,
    control_channel: ClashControlChannel,
    disable_http_controller: bool,
}

impl<'a> ClashRuntimeInputs<'a> {
    fn of(clash: &'a ClashConfig) -> Self {
        Self {
            overrides: &clash.overrides,
            enable_clash_fields: clash.enable_clash_fields,
            enable_tun_mode: clash.enable_tun_mode,
            tun_stack: clash.tun_stack,
            mixed_port: &clash.mixed_port,
            socks_port: clash.socks_port.as_ref(),
            http_port: clash.http_port.as_ref(),
            external_controller: &clash.external_controller,
            control_channel: clash.clash_control_channel,
            disable_http_controller: clash.clash_ipc_disable_http_controller,
        }
    }
}

/// Runtime verdict for a clash-config candidate, overrides included.
///
/// One verdict for build inputs and control-channel inputs alike: they are
/// decided by the same candidate and settled by one reconcile (roadmap §6.2).
pub(in crate::client) fn classify_clash(
    previous: &ClashConfig,
    candidate: &ClashConfig,
) -> RuntimeImpact {
    if ClashRuntimeInputs::of(previous) == ClashRuntimeInputs::of(candidate) {
        RuntimeImpact::None
    } else {
        RuntimeImpact::Reconcile
    }
}

/// Runtime verdict for a profiles candidate.
///
/// Three sources decide it, and all three are needed: the dependency closure of
/// the current selection, the definitions inside that closure, and the content
/// the request touched. Comparing `ProfileItem`s alone would miss a file whose
/// bytes changed under an unchanged path, which is what a subscription refresh
/// does to the running profile.
///
/// Over-counting is deliberate where it is cheap: a touched closure path counts
/// as changed without consulting its digest, and a definition that only carries
/// a new materialization timestamp counts as changed too. Missing a real
/// content change of a current dependency is the failure that is not allowed.
///
/// [`MutationHints::activation`] is not read here. Re-selecting the profile that
/// is already current leaves the built config identical, so it owes a one-shot
/// action rather than a rebuild, exactly as it does today.
pub(in crate::client) fn classify_profiles(
    previous: &Profiles,
    candidate: &Profiles,
    hints: &MutationHints,
) -> RuntimeImpact {
    let before = ProfilesActor::current_closure(previous);
    let after = ProfilesActor::current_closure(candidate);

    // `global_transforms` is compared as a list, not through the closure: the
    // closure is a set, and reordering the global transforms leaves it equal
    // while changing the order they run in.
    if previous.current != candidate.current
        || previous.global_transforms != candidate.global_transforms
        || previous.valid != candidate.valid
        || before != after
    {
        return RuntimeImpact::Reconcile;
    }

    for uid in &after {
        match (definition_of(previous, uid), definition_of(candidate, uid)) {
            // Dangling in both documents: the executor resolves neither, so
            // nothing about the build changed.
            (None, None) => {}
            (Some(previous), Some(candidate)) => {
                let (Ok(previous), Ok(candidate)) =
                    (serde_json::to_vec(previous), serde_json::to_vec(candidate))
                else {
                    // Unable to prove them equal, so assume they are not.
                    return RuntimeImpact::Reconcile;
                };
                if previous != candidate {
                    return RuntimeImpact::Reconcile;
                }
            }
            // The member appeared in or disappeared from the closure.
            _ => return RuntimeImpact::Reconcile,
        }
    }

    let closure_files = closure_files(candidate, &after);
    if hints
        .touched
        .iter()
        .any(|touched| closure_files.contains(&touched.path))
    {
        return RuntimeImpact::Reconcile;
    }

    RuntimeImpact::None
}

/// The runtime target a candidate asks for, as a stable identity.
///
/// This is the same projection the classification above reads, and that is the
/// point: two candidates that ask the core for the same thing are the same
/// target, however much of the rest of the document moved between them. A
/// digest of the whole source document would make an unrelated save — a
/// dashboard URL, a language — look like a different target, which silently
/// resets a convergence budget and makes a re-save of an unconverged value look
/// like a document that changed nothing (roadmap §9.2, D11/V22).
///
/// `None` when the projection cannot be serialized. A target with no identity
/// is never treated as equal to another one.
pub(in crate::client) fn application_target(candidate: &NyanpasuAppConfig) -> Option<String> {
    digest(&ApplicationRuntimeInputs::of(candidate))
}

pub(in crate::client) fn clash_target(candidate: &ClashConfig) -> Option<String> {
    digest(&ClashRuntimeInputs::of(candidate))
}

/// The profiles projection is the runtime dependency closure of the candidate
/// document: the current selection, the global transforms, `valid`, and every
/// closure member with the definition the build would resolve for it.
///
/// [`MutationHints::touched`] is deliberately not part of it. Those hints
/// describe what *this request* changes, and identity has to be a property of
/// the desired runtime rather than of the request that asked for it: a deferred
/// content update hashing `touched = [(path, digest)]` and the later save of the
/// same committed document — which carries no hints at all — ask the core for
/// exactly the same thing, and a key that moved between them would lose the
/// outstanding target and open a fresh convergence budget for it (§9.2,
/// D11/V22, R16).
///
/// This is the structural projection. `RuntimeInputs` combines it with the
/// captured file contents so content changes remain distinct even when a
/// producer has not advanced a materialization stamp.
pub(in crate::client) fn profiles_target(candidate: &Profiles) -> Option<String> {
    let closure = ProfilesActor::current_closure(candidate);
    digest(&ProfilesRuntimeInputs {
        current: candidate.current.as_ref(),
        global_transforms: &candidate.global_transforms,
        valid: &candidate.valid,
        definitions: closure
            .iter()
            .map(|uid| (uid, definition_of(candidate, uid)))
            .collect(),
    })
}

#[derive(Debug, serde::Serialize)]
struct ProfilesRuntimeInputs<'a> {
    current: Option<&'a ProfileId>,
    global_transforms: &'a [ProfileId],
    valid: &'a [String],
    /// The current closure in order, each member with the definition the build
    /// would resolve for it. A member that dangles in this document resolves to
    /// nothing, exactly as the classification reads it.
    definitions: Vec<(&'a ProfileId, Option<&'a ProfileDefinition>)>,
}

fn digest<T: serde::Serialize>(inputs: &T) -> Option<String> {
    serde_json::to_vec(inputs)
        .ok()
        .map(|bytes| nyanpasu_core_manager::payload_digest(&bytes))
}

fn definition_of<'a>(profiles: &'a Profiles, uid: &ProfileId) -> Option<&'a ProfileDefinition> {
    profiles.items.get(uid).map(|item| &item.definition)
}

/// The managed files the closure reads. A composition owns no file of its own,
/// so only its members contribute one.
fn closure_files<'a>(
    profiles: &'a Profiles,
    closure: &IndexSet<ProfileId>,
) -> HashSet<&'a ManagedProfilePath> {
    closure
        .iter()
        .filter_map(|uid| definition_of(profiles, uid)?.source())
        .map(|source| &source.materialized().file)
        .collect()
}

/// The peripheral owners whose *inputs* a candidate moves.
///
/// Only an owner listed here may be handed a new desired target (roadmap §9.1).
/// A language change must not raise the system proxy's target generation: doing
/// so would let any unrelated save reset that owner's retry budget.
///
/// This is not "the owner has converged". An owner absent here can still be
/// holding a target it never managed to apply. That fact lives in the owner's
/// own `EffectStatus`, where `applied_revision` trails `desired_revision`, and
/// the scheduler combines the two — it is never folded into this projection,
/// because then "nothing changed for you" and "you are behind" would be one
/// indistinguishable signal.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::client) struct ChangedOwnerInputs(BTreeSet<EffectKind>);

impl ChangedOwnerInputs {
    /// Reuses the effect plan's struct-patch diff, so an owner appears here on
    /// exactly the inputs that would produce its effect.
    pub fn diff(previous: &ApplicationEffectInputs, candidate: &ApplicationEffectInputs) -> Self {
        Self(
            ApplicationEffectPlan::diff(previous, candidate)
                .effects()
                .iter()
                .map(ApplicationEffect::kind)
                .collect(),
        )
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Read by the post-commit dispatch that hands each owner its new target
    /// (T7); the workflow itself only asks whether any owner moved.
    #[allow(dead_code)]
    pub fn contains(&self, kind: EffectKind) -> bool {
        self.0.contains(&kind)
    }

    #[allow(dead_code)]
    pub fn kinds(&self) -> &BTreeSet<EffectKind> {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nyanpasu_config::{
        application::{
            ClashCore, I18nLanguage, LoggingLevel, NetworkStatisticWidgetConfig,
            ProxiesSelectorMode, ThemeMode, TrayMenuCloseBehavior, TrayMenuMode,
        },
        clash::config::{
            ClashConfigPatch,
            clash_strategy::{break_connection::ProxyChangeBreakMode, port::PortStrategy},
            overrides::{ClashGuardOverridesPatch, LogLevel, Mode},
        },
        profile::{
            ConfigDefinition, ProfileId, ProfileMetadata, ProfileSource, TransformDefinition,
        },
        runtime::executor::ResolvedPortBindings,
    };
    use nyanpasu_egui::widget::StatisticWidgetVariant;
    use struct_patch::Patch as _;

    use crate::{
        client::effects::status::{EffectHealth, EffectRevision, EffectStatus},
        enhance::golden_support,
    };

    /// `NyanpasuAppConfig::default()` reads the system locale and the platform
    /// tray default, so the fields a case flips are pinned first.
    fn base_app() -> NyanpasuAppConfig {
        NyanpasuAppConfig {
            language: I18nLanguage::English,
            app_log_level: LoggingLevel::Info,
            tray_menu_mode: TrayMenuMode::Native,
            ..NyanpasuAppConfig::default()
        }
    }

    /// `ClashGuardOverrides::default()` mints a fresh secret on every call, so
    /// the base has to pin it or two "identical" configs would never compare
    /// equal.
    fn base_clash() -> ClashConfig {
        let mut clash = ClashConfig::default();
        clash.overrides.apply(ClashGuardOverridesPatch {
            secret: Some("fixed-test-secret".to_owned()),
            ..ClashGuardOverridesPatch::default()
        });
        clash
    }

    fn effect_inputs(app: &NyanpasuAppConfig, clash: &ClashConfig) -> ApplicationEffectInputs {
        ApplicationEffectInputs::project(
            app,
            clash,
            Some(ResolvedPortBindings {
                mixed_port: 7890,
                ..ResolvedPortBindings::default()
            }),
        )
    }

    fn owners(
        previous: &ApplicationEffectInputs,
        candidate: &ApplicationEffectInputs,
    ) -> Vec<EffectKind> {
        ChangedOwnerInputs::diff(previous, candidate)
            .kinds()
            .iter()
            .copied()
            .collect()
    }

    fn overrides_patch(patch: ClashGuardOverridesPatch) -> ClashConfig {
        let mut clash = base_clash();
        clash.overrides.apply(patch);
        clash
    }

    struct AppCase {
        field: &'static str,
        mutate: fn(&mut NyanpasuAppConfig),
        impact: RuntimeImpact,
        owners: &'static [EffectKind],
    }

    /// One case per application-config field: the three runtime build inputs
    /// with their verdict, and every other field with `None` plus exactly the
    /// peripheral owners it feeds.
    ///
    /// A function rather than a constant because one field is platform-gated.
    fn app_cases() -> Vec<AppCase> {
        let mut cases = vec![
            // Runtime build inputs.
            AppCase {
                field: "enable_service_mode",
                mutate: |app| app.enable_service_mode = true,
                impact: RuntimeImpact::HostSwitch,
                owners: &[],
            },
            AppCase {
                field: "core",
                mutate: |app| app.core = ClashCore::ClashRs,
                impact: RuntimeImpact::CoreSwap,
                owners: &[],
            },
            AppCase {
                field: "enable_builtin_enhanced",
                mutate: |app| app.enable_builtin_enhanced = false,
                impact: RuntimeImpact::Reconcile,
                owners: &[],
            },
            // Peripheral owners.
            AppCase {
                field: "language",
                mutate: |app| app.language = I18nLanguage::SimplifiedChinese,
                impact: RuntimeImpact::None,
                // The tray menu is rendered with the process locale.
                owners: &[EffectKind::Locale, EffectKind::Tray],
            },
            AppCase {
                field: "app_log_level",
                mutate: |app| app.app_log_level = LoggingLevel::Debug,
                impact: RuntimeImpact::None,
                owners: &[EffectKind::Logger],
            },
            AppCase {
                field: "max_log_files",
                mutate: |app| app.max_log_files = 14,
                impact: RuntimeImpact::None,
                owners: &[EffectKind::Logger],
            },
            AppCase {
                field: "enable_auto_launch",
                mutate: |app| app.enable_auto_launch = true,
                impact: RuntimeImpact::None,
                owners: &[EffectKind::AutoLaunch],
            },
            AppCase {
                field: "enable_system_proxy",
                mutate: |app| app.enable_system_proxy = true,
                impact: RuntimeImpact::None,
                owners: &[EffectKind::SystemProxy, EffectKind::Tray],
            },
            AppCase {
                field: "system_proxy_bypass",
                mutate: |app| app.system_proxy_bypass = "localhost".to_owned(),
                impact: RuntimeImpact::None,
                owners: &[EffectKind::SystemProxy],
            },
            AppCase {
                field: "pac_url",
                mutate: |app| {
                    app.pac_url =
                        Some(url::Url::parse("http://127.0.0.1:1/p.pac").expect("pac url"))
                },
                impact: RuntimeImpact::None,
                owners: &[EffectKind::SystemProxy],
            },
            AppCase {
                field: "enable_proxy_guard",
                mutate: |app| app.enable_proxy_guard = true,
                impact: RuntimeImpact::None,
                owners: &[EffectKind::ProxyGuard],
            },
            AppCase {
                field: "proxy_guard_interval",
                mutate: |app| app.proxy_guard_interval = 60,
                impact: RuntimeImpact::None,
                owners: &[EffectKind::ProxyGuard],
            },
            AppCase {
                field: "hotkeys",
                mutate: |app| app.hotkeys = vec!["open_or_close_dashboard,Ctrl+Q".to_owned()],
                impact: RuntimeImpact::None,
                owners: &[EffectKind::Hotkeys],
            },
            AppCase {
                field: "network_statistic_widget",
                mutate: |app| {
                    app.network_statistic_widget =
                        NetworkStatisticWidgetConfig::Enabled(StatisticWidgetVariant::Small)
                },
                impact: RuntimeImpact::None,
                owners: &[EffectKind::Widget],
            },
            AppCase {
                field: "tray_selector_mode",
                mutate: |app| app.tray_selector_mode = ProxiesSelectorMode::Hidden,
                impact: RuntimeImpact::None,
                owners: &[EffectKind::Tray],
            },
            AppCase {
                field: "tray_menu_mode",
                mutate: |app| app.tray_menu_mode = TrayMenuMode::Webview,
                impact: RuntimeImpact::None,
                owners: &[EffectKind::Tray],
            },
            AppCase {
                field: "enable_tray_text",
                mutate: |app| app.enable_tray_text = true,
                impact: RuntimeImpact::None,
                owners: &[EffectKind::Tray],
            },
            AppCase {
                field: "enable_tray_traffic",
                mutate: |app| app.enable_tray_traffic = true,
                impact: RuntimeImpact::None,
                owners: &[EffectKind::Tray],
            },
            // Fields no runtime build and no effect owner reads.
            AppCase {
                field: "release_channel",
                mutate: |app| app.release_channel = Some(crate::bundle::Channel::Beta),
                impact: RuntimeImpact::None,
                owners: &[],
            },
            AppCase {
                field: "app_singleton_port",
                mutate: |app| app.app_singleton_port = 33333,
                impact: RuntimeImpact::None,
                owners: &[],
            },
            AppCase {
                field: "theme_mode",
                mutate: |app| app.theme_mode = ThemeMode::Dark,
                impact: RuntimeImpact::None,
                owners: &[],
            },
            AppCase {
                field: "traffic_graph",
                mutate: |app| app.traffic_graph = false,
                impact: RuntimeImpact::None,
                owners: &[],
            },
            AppCase {
                field: "enable_memory_usage",
                mutate: |app| app.enable_memory_usage = false,
                impact: RuntimeImpact::None,
                owners: &[],
            },
            AppCase {
                field: "lighten_animation_effects",
                mutate: |app| app.lighten_animation_effects = true,
                impact: RuntimeImpact::None,
                owners: &[],
            },
            AppCase {
                field: "enable_silent_start",
                mutate: |app| app.enable_silent_start = true,
                impact: RuntimeImpact::None,
                owners: &[],
            },
            AppCase {
                field: "default_latency_test",
                mutate: |app| app.default_latency_test = "http://example.invalid".to_owned(),
                impact: RuntimeImpact::None,
                owners: &[],
            },
            AppCase {
                field: "proxy_layout_column",
                mutate: |app| app.proxy_layout_column = 3,
                impact: RuntimeImpact::None,
                owners: &[],
            },
            AppCase {
                field: "enable_auto_check_update",
                mutate: |app| app.enable_auto_check_update = false,
                impact: RuntimeImpact::None,
                owners: &[],
            },
            AppCase {
                field: "always_on_top",
                mutate: |app| app.always_on_top = true,
                impact: RuntimeImpact::None,
                owners: &[],
            },
            AppCase {
                field: "tray_menu_close_behavior",
                mutate: |app| app.tray_menu_close_behavior = TrayMenuCloseBehavior::Close,
                impact: RuntimeImpact::None,
                owners: &[],
            },
            AppCase {
                field: "use_legacy_ui",
                mutate: |app| app.use_legacy_ui = true,
                impact: RuntimeImpact::None,
                owners: &[],
            },
            AppCase {
                field: "theme_color",
                // Mutated in place: the color type is re-exported by neither
                // crate, so a case cannot name it to build one.
                mutate: |app| app.theme_color.r = 1.0,
                impact: RuntimeImpact::None,
                owners: &[],
            },
        ];
        #[cfg(target_os = "macos")]
        cases.push(AppCase {
            field: "enable_macos_colored_icons",
            mutate: |app| app.enable_macos_colored_icons = true,
            impact: RuntimeImpact::None,
            owners: &[],
        });
        cases
    }

    #[test]
    fn every_application_field_is_classified_on_its_own() {
        let clash = base_clash();
        for case in &app_cases() {
            let previous = base_app();
            let mut candidate = base_app();
            (case.mutate)(&mut candidate);
            assert!(
                serde_json::to_value(&previous).expect("previous app config")
                    != serde_json::to_value(&candidate).expect("candidate app config"),
                "{}: the case must change the field it names",
                case.field
            );

            assert_eq!(
                classify_application(&previous, &candidate),
                case.impact,
                "{} runtime verdict",
                case.field
            );
            assert_eq!(
                owners(
                    &effect_inputs(&previous, &clash),
                    &effect_inputs(&candidate, &clash)
                ),
                case.owners,
                "{} peripheral owners",
                case.field
            );
        }
    }

    /// The table above only proves "single-field coverage" while it covers
    /// every field. A field added to the config without a case here would
    /// otherwise be classified by nobody and noticed by nothing.
    #[test]
    fn the_application_case_table_covers_every_field() {
        // `pac_url` is skipped on serialize while it is `None`, so the fixture
        // carries a value purely to make the key appear.
        let mut app = base_app();
        app.pac_url = Some(url::Url::parse("http://127.0.0.1:1/p.pac").expect("pac url"));
        let value = serde_json::to_value(&app).expect("app config");

        let fields: BTreeSet<&str> = value
            .as_object()
            .expect("the app config serializes as a map")
            .keys()
            .map(String::as_str)
            .collect();
        let cases = app_cases();
        let covered: BTreeSet<&str> = cases.iter().map(|case| case.field).collect();

        assert_eq!(fields, covered);
    }

    #[test]
    fn an_unchanged_application_config_has_no_impact_anywhere() {
        let clash = base_clash();
        assert_eq!(
            classify_application(&base_app(), &base_app()),
            RuntimeImpact::None
        );
        assert!(
            ChangedOwnerInputs::diff(
                &effect_inputs(&base_app(), &clash),
                &effect_inputs(&base_app(), &clash)
            )
            .is_empty()
        );
    }

    struct ClashCase {
        field: &'static str,
        candidate: fn() -> ClashConfig,
        impact: RuntimeImpact,
    }

    /// One case per clash-config field, overrides members included. Everything
    /// the build or the control channel reads is `Reconcile`; the two fields
    /// neither reads are `None`.
    const CLASH_CASES: &[ClashCase] = &[
        ClashCase {
            field: "overrides.log_level",
            candidate: || {
                overrides_patch(ClashGuardOverridesPatch {
                    log_level: Some(LogLevel::Debug),
                    ..ClashGuardOverridesPatch::default()
                })
            },
            impact: RuntimeImpact::Reconcile,
        },
        ClashCase {
            field: "overrides.allow_lan",
            candidate: || {
                overrides_patch(ClashGuardOverridesPatch {
                    allow_lan: Some(true),
                    ..ClashGuardOverridesPatch::default()
                })
            },
            impact: RuntimeImpact::Reconcile,
        },
        ClashCase {
            field: "overrides.mode",
            candidate: || {
                overrides_patch(ClashGuardOverridesPatch {
                    mode: Some(Mode::Global),
                    ..ClashGuardOverridesPatch::default()
                })
            },
            impact: RuntimeImpact::Reconcile,
        },
        ClashCase {
            field: "overrides.secret",
            candidate: || {
                overrides_patch(ClashGuardOverridesPatch {
                    secret: Some("a-new-secret".to_owned()),
                    ..ClashGuardOverridesPatch::default()
                })
            },
            impact: RuntimeImpact::Reconcile,
        },
        ClashCase {
            field: "overrides.unified_delay",
            candidate: || {
                overrides_patch(ClashGuardOverridesPatch {
                    unified_delay: Some(false),
                    ..ClashGuardOverridesPatch::default()
                })
            },
            impact: RuntimeImpact::Reconcile,
        },
        ClashCase {
            field: "overrides.tcp_concurrent",
            candidate: || {
                overrides_patch(ClashGuardOverridesPatch {
                    tcp_concurrent: Some(false),
                    ..ClashGuardOverridesPatch::default()
                })
            },
            impact: RuntimeImpact::Reconcile,
        },
        ClashCase {
            field: "overrides.ipv6",
            candidate: || {
                overrides_patch(ClashGuardOverridesPatch {
                    ipv6: Some(true),
                    ..ClashGuardOverridesPatch::default()
                })
            },
            impact: RuntimeImpact::Reconcile,
        },
        ClashCase {
            field: "enable_tun_mode",
            candidate: || ClashConfig {
                enable_tun_mode: true,
                ..base_clash()
            },
            impact: RuntimeImpact::Reconcile,
        },
        ClashCase {
            field: "tun_stack",
            candidate: || ClashConfig {
                tun_stack: TunStack::System,
                ..base_clash()
            },
            impact: RuntimeImpact::Reconcile,
        },
        ClashCase {
            field: "enable_clash_fields",
            candidate: || ClashConfig {
                enable_clash_fields: !base_clash().enable_clash_fields,
                ..base_clash()
            },
            impact: RuntimeImpact::Reconcile,
        },
        ClashCase {
            field: "mixed_port",
            candidate: || ClashConfig {
                mixed_port: PortStrategy::new_allow_fallback(7999),
                ..base_clash()
            },
            impact: RuntimeImpact::Reconcile,
        },
        ClashCase {
            field: "socks_port",
            candidate: || ClashConfig {
                socks_port: Some(PortStrategy::new_allow_fallback(1080)),
                ..base_clash()
            },
            impact: RuntimeImpact::Reconcile,
        },
        ClashCase {
            field: "http_port",
            candidate: || ClashConfig {
                http_port: Some(PortStrategy::new_allow_fallback(1081)),
                ..base_clash()
            },
            impact: RuntimeImpact::Reconcile,
        },
        ClashCase {
            field: "external_controller",
            candidate: || ClashConfig {
                external_controller: ExternalControllerStrategy {
                    port: PortStrategy::new_allow_fallback(19999),
                    ..ExternalControllerStrategy::default()
                },
                ..base_clash()
            },
            impact: RuntimeImpact::Reconcile,
        },
        ClashCase {
            field: "clash_control_channel",
            candidate: || ClashConfig {
                clash_control_channel: ClashControlChannel::HttpOnly,
                ..base_clash()
            },
            impact: RuntimeImpact::Reconcile,
        },
        ClashCase {
            field: "clash_ipc_disable_http_controller",
            candidate: || ClashConfig {
                clash_ipc_disable_http_controller: true,
                ..base_clash()
            },
            impact: RuntimeImpact::Reconcile,
        },
        // Read by the dashboard, never by a build stage.
        ClashCase {
            field: "web_ui_list",
            candidate: || ClashConfig {
                web_ui_list: vec!["http://example.invalid/ui".to_owned()],
                ..base_clash()
            },
            impact: RuntimeImpact::None,
        },
        // Decides whether connections are cut after an apply, not what is built.
        ClashCase {
            field: "break_connection",
            candidate: || {
                let mut clash = base_clash();
                clash.break_connection.on_mode_change = !clash.break_connection.on_mode_change;
                clash.break_connection.on_profile_change =
                    !clash.break_connection.on_profile_change;
                clash.break_connection.on_proxy_change = ProxyChangeBreakMode::Off;
                clash
            },
            impact: RuntimeImpact::None,
        },
    ];

    #[test]
    fn every_clash_field_is_classified_on_its_own() {
        for case in CLASH_CASES {
            let previous = base_clash();
            let candidate = (case.candidate)();
            assert!(
                serde_json::to_value(&previous).expect("previous clash config")
                    != serde_json::to_value(&candidate).expect("candidate clash config"),
                "{}: the case must change the field it names",
                case.field
            );
            assert_eq!(
                classify_clash(&previous, &candidate),
                case.impact,
                "{} runtime verdict",
                case.field
            );
        }
    }

    /// Same guard for the clash table, overrides members included: the
    /// overrides struct is one build dependency, so each of its fields needs a
    /// case of its own.
    #[test]
    fn the_clash_case_table_covers_every_field() {
        fn keys(value: &serde_json::Value) -> BTreeSet<String> {
            value
                .as_object()
                .expect("a map")
                .keys()
                // The overrides go on the wire in kebab-case; the case names
                // spell the Rust fields.
                .map(|key| key.replace('-', "_"))
                .collect()
        }

        let clash = base_clash();
        let mut fields = keys(&serde_json::to_value(&clash).expect("clash config"));
        fields.remove("overrides");
        fields.extend(
            keys(&serde_json::to_value(&clash.overrides).expect("overrides"))
                .into_iter()
                .map(|field| format!("overrides.{field}")),
        );

        let covered: BTreeSet<String> = CLASH_CASES
            .iter()
            .map(|case| case.field.to_owned())
            .collect();

        assert_eq!(fields, covered);
    }

    #[test]
    fn an_unchanged_clash_config_has_no_runtime_impact() {
        assert_eq!(
            classify_clash(&base_clash(), &base_clash()),
            RuntimeImpact::None
        );
    }

    #[test]
    fn a_tun_mode_change_is_both_a_rebuild_and_a_tray_input() {
        let previous = base_clash();
        let candidate = ClashConfig {
            enable_tun_mode: true,
            ..base_clash()
        };
        let app = base_app();

        assert_eq!(
            classify_clash(&previous, &candidate),
            RuntimeImpact::Reconcile
        );
        assert_eq!(
            owners(
                &effect_inputs(&app, &previous),
                &effect_inputs(&app, &candidate)
            ),
            vec![EffectKind::Tray],
        );
    }

    /// current -> `sel`; `sel` carries `scoped` as a scoped transform; `global`
    /// is a document-level transform; `spare` is reachable from neither.
    fn profiles() -> Profiles {
        let mut profiles = Profiles::default();
        profiles.append_item(golden_support::file_config("sel", "sel.yaml", &["scoped"]));
        profiles.append_item(golden_support::overlay("scoped", "scoped.yaml"));
        profiles.append_item(golden_support::overlay("global", "global.yaml"));
        profiles.append_item(golden_support::file_config("spare", "spare.yaml", &[]));
        profiles.current = Some(ProfileId("sel".into()));
        profiles.global_transforms = vec![ProfileId("global".into())];
        profiles
    }

    fn touching(file: &str) -> MutationHints {
        MutationHints {
            touched: vec![TouchedContent {
                path: ManagedProfilePath::new(file).expect("managed path"),
                content_digest: Some(ContentDigest::new("sha256:new")),
                resource: Some(StagedResourceToken::new("staged-1")),
            }],
            ..MutationHints::default()
        }
    }

    fn replace_overlay_source(profiles: &mut Profiles, uid: &str, file: &str) {
        let item = profiles
            .items
            .get_mut(&ProfileId(uid.into()))
            .expect("item exists");
        item.definition = ProfileDefinition::Transform {
            transform: TransformDefinition::Overlay(nyanpasu_config::profile::OverlayTransform {
                source: ProfileSource::Local {
                    binding: nyanpasu_config::profile::LocalBinding::Managed {
                        materialized: golden_support::managed(file),
                    },
                },
            }),
        };
    }

    #[test]
    fn a_definition_change_inside_the_current_closure_affects_the_runtime() {
        let previous = profiles();
        let mut candidate = profiles();
        replace_overlay_source(&mut candidate, "scoped", "scoped-v2.yaml");

        assert_eq!(
            classify_profiles(&previous, &candidate, &MutationHints::default()),
            RuntimeImpact::Reconcile
        );
    }

    #[test]
    fn a_definition_change_outside_the_current_closure_does_not_affect_the_runtime() {
        let previous = profiles();
        let mut candidate = profiles();
        let item = candidate
            .items
            .get_mut(&ProfileId("spare".into()))
            .expect("spare exists");
        item.definition = ProfileDefinition::Config {
            config: ConfigDefinition::File(nyanpasu_config::profile::FileConfig {
                source: ProfileSource::Local {
                    binding: nyanpasu_config::profile::LocalBinding::Managed {
                        materialized: golden_support::managed("spare-v2.yaml"),
                    },
                },
                transforms: Vec::new(),
            }),
        };

        assert_eq!(
            classify_profiles(&previous, &candidate, &MutationHints::default()),
            RuntimeImpact::None
        );
    }

    #[test]
    fn touched_content_on_a_closure_path_affects_the_runtime_without_a_document_change() {
        let profiles = profiles();

        // The document is byte-identical: only the file behind `scoped` changed.
        assert_eq!(
            classify_profiles(&profiles, &profiles, &touching("scoped.yaml")),
            RuntimeImpact::Reconcile
        );
        assert_eq!(
            classify_profiles(&profiles, &profiles, &touching("sel.yaml")),
            RuntimeImpact::Reconcile
        );
        assert_eq!(
            classify_profiles(&profiles, &profiles, &touching("global.yaml")),
            RuntimeImpact::Reconcile
        );
    }

    #[test]
    fn touched_content_outside_the_closure_does_not_affect_the_runtime() {
        let profiles = profiles();
        assert_eq!(
            classify_profiles(&profiles, &profiles, &touching("spare.yaml")),
            RuntimeImpact::None
        );
    }

    #[test]
    fn a_current_selection_change_affects_the_runtime() {
        let previous = profiles();
        let mut candidate = profiles();
        candidate.current = Some(ProfileId("spare".into()));

        assert_eq!(
            classify_profiles(&previous, &candidate, &MutationHints::default()),
            RuntimeImpact::Reconcile
        );
    }

    /// The closure is a set, so reordering the global transforms leaves it
    /// equal while changing the order they run in.
    #[test]
    fn reordering_the_global_transforms_affects_the_runtime() {
        let mut previous = profiles();
        previous.append_item(golden_support::overlay("global2", "global2.yaml"));
        previous.global_transforms = vec![ProfileId("global".into()), ProfileId("global2".into())];
        let mut candidate = previous.clone();
        candidate.global_transforms.reverse();

        assert_eq!(
            ProfilesActor::current_closure(&previous),
            ProfilesActor::current_closure(&candidate),
            "the closure must be set-equal for this test to mean anything"
        );
        assert_eq!(
            classify_profiles(&previous, &candidate, &MutationHints::default()),
            RuntimeImpact::Reconcile
        );
    }

    #[test]
    fn a_retained_field_change_affects_the_runtime() {
        let previous = profiles();
        let mut candidate = profiles();
        candidate.valid.push("new-clash-field".to_owned());

        assert_eq!(
            classify_profiles(&previous, &candidate, &MutationHints::default()),
            RuntimeImpact::Reconcile
        );
    }

    /// Metadata is the SaveOnly case: no build stage reads it, not even for the
    /// profile that is running.
    #[test]
    fn profile_metadata_has_no_runtime_impact() {
        let previous = profiles();
        let mut candidate = profiles();
        candidate
            .items
            .get_mut(&ProfileId("sel".into()))
            .expect("sel exists")
            .metadata = ProfileMetadata {
            name: "renamed".to_owned(),
            desc: Some("a description".to_owned()),
            custom_name: true,
        };

        assert_eq!(
            classify_profiles(&previous, &candidate, &MutationHints::default()),
            RuntimeImpact::None
        );
    }

    #[test]
    fn an_unchanged_profiles_document_has_no_runtime_impact() {
        let profiles = profiles();
        assert_eq!(
            classify_profiles(&profiles, &profiles, &MutationHints::default()),
            RuntimeImpact::None
        );
    }

    /// The two signals the scheduler must not confuse: an owner with no input
    /// change gets no new target, which says nothing about whether the target
    /// it already holds ever applied.
    #[test]
    fn an_unrelated_change_gives_an_owner_no_new_target_while_it_stays_unconverged() {
        let clash = base_clash();
        let previous = base_app();
        let mut candidate = base_app();
        candidate.language = I18nLanguage::SimplifiedChinese;

        let changed = ChangedOwnerInputs::diff(
            &effect_inputs(&previous, &clash),
            &effect_inputs(&candidate, &clash),
        );
        assert!(
            !changed.contains(EffectKind::SystemProxy),
            "a language change must not raise the system proxy's target"
        );

        // Independently of that, the system proxy may be behind: the fact lives
        // in its own status, not in the projection above.
        let unconverged = EffectStatus {
            kind: EffectKind::SystemProxy,
            desired_revision: EffectRevision::new(4),
            applied_revision: EffectRevision::new(3),
            health: EffectHealth::Degraded {
                code: "system_proxy_apply_failed",
                message: "os refused".to_owned(),
                retryable: true,
            },
        };
        assert!(unconverged.applied_revision < unconverged.desired_revision);
    }

    /// An empty patch reaches every domain and must produce no target at all.
    #[test]
    fn an_empty_clash_patch_gives_no_owner_a_new_target() {
        let previous = base_clash();
        let mut candidate = base_clash();
        candidate.apply(ClashConfigPatch::default());
        let app = base_app();

        assert_eq!(classify_clash(&previous, &candidate), RuntimeImpact::None);
        assert!(
            ChangedOwnerInputs::diff(
                &effect_inputs(&app, &previous),
                &effect_inputs(&app, &candidate)
            )
            .is_empty()
        );
    }

    /// A patch names a field by carrying it, whatever value it carries. The
    /// value is exactly what must not decide: resubmitting the committed
    /// `core` unchanged is still a request for that runtime (R15).
    #[test]
    fn an_application_patch_names_the_fields_it_carries_whatever_their_values() {
        let base = base_app();
        let unchanged = RequestedRuntimeFields::of_application(&NyanpasuAppConfigPatch {
            core: Some(base.core),
            ..NyanpasuAppConfigPatch::default()
        });
        assert!(unchanged.names_any());
        assert_eq!(unchanged, RequestedRuntimeFields::runtime());
        assert_eq!(
            RequestedRuntimeFields::of_application(&NyanpasuAppConfigPatch {
                enable_service_mode: Some(true),
                enable_builtin_enhanced: Some(false),
                ..NyanpasuAppConfigPatch::default()
            }),
            RequestedRuntimeFields::runtime()
        );
    }

    /// The other direction: a patch that only carries fields no build stage
    /// reads asked the runtime for nothing, however large it is.
    #[test]
    fn a_patch_of_fields_no_build_stage_reads_names_nothing() {
        assert!(
            !RequestedRuntimeFields::of_application(&NyanpasuAppConfigPatch {
                language: Some(I18nLanguage::English),
                theme_mode: Some(ThemeMode::Dark),
                enable_system_proxy: Some(true),
                ..NyanpasuAppConfigPatch::default()
            })
            .names_any()
        );
        assert!(
            !RequestedRuntimeFields::of_clash(&ClashConfigPatch {
                web_ui_list: Some(vec!["http://127.0.0.1:9090/ui".into()]),
                break_connection: Some(Default::default()),
                ..ClashConfigPatch::default()
            })
            .names_any()
        );
        assert!(!RequestedRuntimeFields::default().names_any());
        assert!(RequestedRuntimeFields::whole_document().names_any());
    }

    #[test]
    fn a_clash_patch_names_its_runtime_fields_including_the_overrides() {
        assert_eq!(
            RequestedRuntimeFields::of_clash(&ClashConfigPatch {
                enable_tun_mode: Some(true),
                mixed_port: Some(PortStrategy::default()),
                web_ui_list: Some(Vec::new()),
                ..ClashConfigPatch::default()
            }),
            RequestedRuntimeFields::runtime()
        );
        assert_eq!(
            RequestedRuntimeFields::of_clash(&ClashConfigPatch {
                overrides: Some(base_clash().overrides),
                ..ClashConfigPatch::default()
            }),
            RequestedRuntimeFields::runtime()
        );
        // The overrides have an entry point of their own, and an empty patch
        // through it addressed nothing.
        assert_eq!(
            RequestedRuntimeFields::of_clash_overrides(&ClashGuardOverridesPatch {
                mode: Some(Mode::Direct),
                ..ClashGuardOverridesPatch::default()
            }),
            RequestedRuntimeFields::runtime()
        );
        assert!(
            !RequestedRuntimeFields::of_clash_overrides(&ClashGuardOverridesPatch::default())
                .names_any()
        );
    }

    /// The profiles selection has no patch field: a request expresses it as an
    /// activation, and re-selecting the profile that is already current is the
    /// resubmission the third trigger exists for.
    #[test]
    fn a_re_activation_of_the_current_profile_names_a_runtime_field() {
        assert!(!MutationHints::default().names_runtime_field());
        assert!(
            MutationHints {
                activation: ActivationIntent::Activate(ProfileId("sel".into())),
                ..MutationHints::default()
            }
            .names_runtime_field()
        );
        // Replaced managed bytes are not read here: content inside the current
        // closure already moves `classify_profiles`, and content outside it is
        // not a runtime input at all.
        assert!(
            !MutationHints {
                touched: vec![TouchedContent {
                    path: ManagedProfilePath::new("other.yaml").expect("managed path"),
                    content_digest: None,
                    resource: None,
                }],
                ..MutationHints::default()
            }
            .names_runtime_field()
        );
    }

    #[test]
    fn a_wider_runtime_impact_wins_when_domains_are_combined() {
        assert_eq!(
            RuntimeImpact::Reconcile.max(RuntimeImpact::HostSwitch),
            RuntimeImpact::HostSwitch
        );
        assert_eq!(
            RuntimeImpact::CoreSwap.max(RuntimeImpact::Reconcile),
            RuntimeImpact::CoreSwap
        );
        assert_eq!(
            RuntimeImpact::None.max(RuntimeImpact::None),
            RuntimeImpact::None
        );
    }
}
