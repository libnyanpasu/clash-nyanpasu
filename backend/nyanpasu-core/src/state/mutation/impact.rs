//! Runtime input projections, target identities, and source-change detection.

use std::collections::HashSet;

use indexmap::IndexSet;
use nyanpasu_config::{
    application::NyanpasuAppConfig,
    clash::config::{
        ClashConfig, ClashControlChannel,
        clash_strategy::port::{ExternalControllerStrategy, PortStrategy},
        overrides::{ClashGuardOverrides, TunOverrides},
        tun_stack::SelectedTunStack,
    },
    profile::{ManagedProfilePath, ProfileDefinition, ProfileId, Profiles},
};

use crate::state::profiles::ProfilesActor;

/// Runtime projections supplied by each source domain.
pub(crate) trait RuntimeDomain {
    fn reaches_runtime(previous: &Self, candidate: &Self, touched: &[ManagedProfilePath]) -> bool;
}

impl RuntimeDomain for NyanpasuAppConfig {
    fn reaches_runtime(previous: &Self, candidate: &Self, _: &[ManagedProfilePath]) -> bool {
        application_runtime_inputs(previous) != application_runtime_inputs(candidate)
    }
}
impl RuntimeDomain for ClashConfig {
    fn reaches_runtime(previous: &Self, candidate: &Self, _: &[ManagedProfilePath]) -> bool {
        clash_runtime_inputs(previous) != clash_runtime_inputs(candidate)
    }
}
impl RuntimeDomain for Profiles {
    fn reaches_runtime(previous: &Self, candidate: &Self, touched: &[ManagedProfilePath]) -> bool {
        let before = profiles_target(previous);
        let after = profiles_target(candidate);
        selection_changed(previous, candidate)
            || before.is_none()
            || before != after
            || touched.iter().any(|path| {
                closure_files(candidate, &ProfilesActor::current_closure(candidate)).contains(path)
            })
    }
}

/// Whether committed build inputs or content in the current closure changes.
pub(crate) fn reaches_runtime<T: RuntimeDomain>(
    previous: &T,
    candidate: &T,
    touched: &[ManagedProfilePath],
) -> bool {
    T::reaches_runtime(previous, candidate, touched)
}

/// Which application-config fields the runtime build and the core lifecycle
/// read.
///
/// - `enable_service_mode` is the execution host the lifecycle follows
///   (`core_lifecycle/workflow.rs`);
/// - `core` picks the builtin transform table, the tun flavor, and the binary
///   that gets started (`nyanpasu-profile-transform/src/runtime_builder.rs`,
///   `RuntimeBuildPort::core_spec`);
/// - `enable_builtin_enhanced` gates that transform table.
///
/// Every other field of [`NyanpasuAppConfig`] drives a peripheral owner or
/// nothing at all.
#[derive(Debug, PartialEq, serde::Serialize)]
struct ApplicationRuntimeInputs<'a> {
    enable_service_mode: bool,
    core: &'a nyanpasu_config::application::ClashCore,
    enable_builtin_enhanced: bool,
}

/// The fields of `app` the runtime reads, and nothing else.
fn application_runtime_inputs(app: &NyanpasuAppConfig) -> ApplicationRuntimeInputs<'_> {
    ApplicationRuntimeInputs {
        enable_service_mode: app.enable_service_mode,
        core: &app.core,
        enable_builtin_enhanced: app.enable_builtin_enhanced,
    }
}

/// Every clash-config field the runtime build or the control channel reads.
///
/// Enumerated from the real consumers rather than from a field whitelist:
/// `RuntimeBuilder::build` reads `overrides`, `enable_clash_fields`,
/// `expand_include_all`, `enable_tun_mode`, `tun` and `tun_stack`;
/// `SessionPortResolver::resolve` turns the four port strategies into the runtime bindings;
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
    expand_include_all: bool,
    enable_tun_mode: bool,
    tun: TunOverrides,
    tun_stack: SelectedTunStack<'a>,
    mixed_port: &'a PortStrategy,
    socks_port: Option<&'a PortStrategy>,
    http_port: Option<&'a PortStrategy>,
    redir_port: Option<&'a PortStrategy>,
    tproxy_port: Option<&'a PortStrategy>,
    transparent_proxy: &'a nyanpasu_config::clash::config::TransparentProxyConfig,
    external_controller: &'a ExternalControllerStrategy,
    control_channel: ClashControlChannel,
    disable_http_controller: bool,
}

/// The fields of `clash` the runtime reads, and nothing else.
fn clash_runtime_inputs(clash: &ClashConfig) -> ClashRuntimeInputs<'_> {
    ClashRuntimeInputs {
        overrides: &clash.overrides,
        enable_clash_fields: clash.enable_clash_fields,
        expand_include_all: clash.expand_include_all,
        enable_tun_mode: clash.enable_tun_mode,
        tun: clash.tun,
        tun_stack: clash.selected_tun_stack(),
        mixed_port: &clash.mixed_port,
        socks_port: clash.socks_port.as_ref(),
        http_port: clash.http_port.as_ref(),
        redir_port: clash.redir_port.as_ref(),
        tproxy_port: clash.tproxy_port.as_ref(),
        transparent_proxy: &clash.transparent_proxy,
        external_controller: &clash.external_controller,
        control_channel: clash.clash_control_channel,
        disable_http_controller: clash.clash_ipc_disable_http_controller,
    }
}

/// Whether the candidate selects a different current profile, which is what
/// interrupts connections on a profile change.
pub(crate) fn selection_changed(previous: &Profiles, candidate: &Profiles) -> bool {
    previous.current != candidate.current
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
pub(crate) fn application_target(candidate: &NyanpasuAppConfig) -> Option<String> {
    digest(&application_runtime_inputs(candidate))
}

pub(crate) fn clash_target(candidate: &ClashConfig) -> Option<String> {
    digest(&clash_runtime_inputs(candidate))
}

/// The profiles projection is the runtime dependency closure of the candidate
/// document: the current selection, the global transforms, `valid`, and every
/// closure member with the definition the build would resolve for it.
///
/// Captured content is combined with this structural projection by the runtime inputs.
pub(crate) fn profiles_target(candidate: &Profiles) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use nyanpasu_config::{
        application::{
            ClashCore, I18nLanguage, LoggingLevel, NetworkStatisticWidgetConfig,
            ProxiesSelectorMode, ReleaseChannel, StatisticWidgetVariant, ThemeMode, TrayMenuMode,
            WindowCloseBehavior, WindowCloseSettings,
        },
        clash::config::{
            ClashConfigPatch,
            clash_strategy::{
                break_connection::{BreakConnectionStrategyPatch, ProxyChangeBreakMode},
                port::{
                    ExternalControllerStrategyPatch, PortStrategy, PortStrategyKind,
                    PortStrategyPatch,
                },
            },
            overrides::{ClashGuardOverridesPatch, LogLevel, ManageableField, Mode},
            tun_stack::{MipsConfig, TunCongestionController, TunStack},
        },
        profile::{
            ConfigDefinition, ProfileId, ProfileMetadata, ProfileSource, TransformDefinition,
        },
        runtime::executor::ResolvedPortBindings,
    };
    use struct_patch::Patch as _;

    use crate::client::effects::{
        plan::{ApplicationEffect, ApplicationEffectInputs, ApplicationEffectPlan},
        status::{EffectHealth, EffectRevision, EffectStatus},
    };
    use nyanpasu_profile_transform::test_support;
    use nyanpasu_runtime_values::configuration::{EffectFailureCode, EffectKind};

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

    /// The owners the effect plan hands a new target, in kind order.
    fn owners(
        previous: &ApplicationEffectInputs,
        candidate: &ApplicationEffectInputs,
    ) -> Vec<EffectKind> {
        let mut kinds: Vec<EffectKind> = ApplicationEffectPlan::diff(previous, candidate)
            .effects()
            .iter()
            .map(ApplicationEffect::kind)
            .collect();
        kinds.sort();
        kinds.dedup();
        kinds
    }

    fn overrides_patch(patch: ClashGuardOverridesPatch) -> ClashConfig {
        let mut clash = base_clash();
        clash.overrides.apply(patch);
        clash
    }

    struct AppCase {
        field: &'static str,
        mutate: fn(&mut NyanpasuAppConfig),
        impact: bool,
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
                impact: true,
                owners: &[EffectKind::Tray],
            },
            AppCase {
                field: "core",
                mutate: |app| app.core = ClashCore::ClashRs,
                impact: true,
                // The core determines whether the transparent-proxy item is available.
                owners: &[EffectKind::Tray],
            },
            AppCase {
                field: "enable_builtin_enhanced",
                mutate: |app| app.enable_builtin_enhanced = false,
                impact: true,
                owners: &[],
            },
            AppCase {
                field: "core_logs",
                mutate: |app| app.core_logs.max_size_mib = 128,
                impact: false,
                owners: &[EffectKind::CoreLogStorage],
            },
            // Peripheral owners.
            AppCase {
                field: "language",
                mutate: |app| app.language = I18nLanguage::SimplifiedChinese,
                impact: false,
                // The tray menu is rendered with the process locale.
                owners: &[EffectKind::Locale, EffectKind::Tray],
            },
            AppCase {
                field: "app_log_level",
                mutate: |app| app.app_log_level = LoggingLevel::Debug,
                impact: false,
                owners: &[EffectKind::Logger],
            },
            AppCase {
                field: "max_log_files",
                mutate: |app| app.max_log_files = 14,
                impact: false,
                owners: &[EffectKind::Logger],
            },
            AppCase {
                field: "max_log_file_size",
                mutate: |app| app.max_log_file_size = 20,
                impact: false,
                owners: &[EffectKind::Logger],
            },
            AppCase {
                field: "enable_auto_launch",
                mutate: |app| app.enable_auto_launch = true,
                impact: false,
                owners: &[EffectKind::AutoLaunch],
            },
            AppCase {
                field: "enable_system_proxy",
                mutate: |app| app.enable_system_proxy = true,
                impact: false,
                owners: &[EffectKind::SystemProxy, EffectKind::Tray],
            },
            AppCase {
                field: "system_proxy_bypass",
                mutate: |app| app.system_proxy_bypass = "localhost".to_owned(),
                impact: false,
                owners: &[EffectKind::SystemProxy],
            },
            AppCase {
                field: "pac_url",
                mutate: |app| {
                    app.pac_url =
                        Some(url::Url::parse("http://127.0.0.1:1/p.pac").expect("pac url"))
                },
                impact: false,
                owners: &[EffectKind::SystemProxy],
            },
            AppCase {
                field: "enable_proxy_guard",
                mutate: |app| app.enable_proxy_guard = true,
                impact: false,
                owners: &[EffectKind::ProxyGuard],
            },
            AppCase {
                field: "proxy_guard_interval",
                mutate: |app| app.proxy_guard_interval = 60,
                impact: false,
                owners: &[EffectKind::ProxyGuard],
            },
            AppCase {
                field: "hotkeys",
                mutate: |app| app.hotkeys = vec!["open_or_close_dashboard,Ctrl+Q".to_owned()],
                impact: false,
                owners: &[EffectKind::Hotkeys],
            },
            AppCase {
                field: "network_statistic_widget",
                mutate: |app| {
                    app.network_statistic_widget =
                        NetworkStatisticWidgetConfig::Enabled(StatisticWidgetVariant::Small)
                },
                impact: false,
                owners: &[EffectKind::Widget],
            },
            AppCase {
                field: "network_statistic_widget_logo",
                mutate: |app| {
                    app.network_statistic_widget_logo.source =
                        nyanpasu_config::application::NetworkStatisticWidgetLogoSource::Widget
                },
                impact: false,
                owners: &[EffectKind::WidgetLogo],
            },
            AppCase {
                field: "tray_selector_mode",
                mutate: |app| app.tray_selector_mode = ProxiesSelectorMode::Hidden,
                impact: false,
                owners: &[EffectKind::Tray],
            },
            AppCase {
                field: "tray_menu_mode",
                mutate: |app| app.tray_menu_mode = TrayMenuMode::Webview,
                impact: false,
                owners: &[EffectKind::Tray],
            },
            AppCase {
                field: "enable_tray_text",
                mutate: |app| app.enable_tray_text = true,
                impact: false,
                owners: &[EffectKind::Tray],
            },
            AppCase {
                field: "enable_tray_traffic",
                mutate: |app| app.enable_tray_traffic = true,
                impact: false,
                owners: &[EffectKind::Tray],
            },
            // Fields no runtime build and no effect owner reads.
            AppCase {
                field: "update_sources",
                mutate: |app| {
                    app.update_sources = vec![nyanpasu_config::application::UpdateSource::Github]
                },
                impact: false,
                owners: &[],
            },
            AppCase {
                field: "enable_local_ip_probe",
                mutate: |app| app.enable_local_ip_probe = true,
                impact: false,
                owners: &[],
            },
            AppCase {
                field: "stun_servers",
                mutate: |app| app.stun_servers.clear(),
                impact: false,
                owners: &[],
            },
            AppCase {
                field: "ip_lookup_providers",
                mutate: |app| {
                    app.ip_lookup_providers =
                        vec![nyanpasu_config::application::IpLookupProvider::Ipify]
                },
                impact: false,
                owners: &[],
            },
            AppCase {
                field: "traffic_retention",
                mutate: |app| {
                    app.traffic_retention = nyanpasu_config::application::TrafficRetention::Forever
                },
                impact: false,
                owners: &[],
            },
            AppCase {
                field: "release_channel",
                mutate: |app| app.release_channel = Some(ReleaseChannel::Beta),
                impact: false,
                owners: &[],
            },
            AppCase {
                field: "app_singleton_port",
                mutate: |app| app.app_singleton_port = 33333,
                impact: false,
                owners: &[],
            },
            AppCase {
                field: "theme_mode",
                mutate: |app| app.theme_mode = ThemeMode::Dark,
                impact: false,
                owners: &[],
            },
            AppCase {
                field: "traffic_graph",
                mutate: |app| app.traffic_graph = false,
                impact: false,
                owners: &[],
            },
            AppCase {
                field: "enable_memory_usage",
                mutate: |app| app.enable_memory_usage = false,
                impact: false,
                owners: &[],
            },
            AppCase {
                field: "lighten_animation_effects",
                mutate: |app| app.lighten_animation_effects = true,
                impact: false,
                owners: &[],
            },
            AppCase {
                field: "enable_silent_start",
                mutate: |app| app.enable_silent_start = true,
                impact: false,
                owners: &[],
            },
            AppCase {
                field: "default_latency_test",
                mutate: |app| app.default_latency_test = "http://example.invalid".to_owned(),
                impact: false,
                owners: &[],
            },
            AppCase {
                field: "default_latency_timeout_ms",
                mutate: |app| app.default_latency_timeout_ms = 10000,
                impact: false,
                owners: &[],
            },
            AppCase {
                field: "proxy_layout_column",
                mutate: |app| app.proxy_layout_column = 3,
                impact: false,
                owners: &[],
            },
            AppCase {
                field: "enable_auto_check_update",
                mutate: |app| app.enable_auto_check_update = false,
                impact: false,
                owners: &[],
            },
            AppCase {
                field: "enable_auto_download_update",
                mutate: |app| app.enable_auto_download_update = true,
                impact: false,
                owners: &[],
            },
            AppCase {
                field: "always_on_top",
                mutate: |app| app.always_on_top = true,
                impact: false,
                owners: &[],
            },
            AppCase {
                field: "window_close",
                mutate: |app| {
                    app.window_close = WindowCloseSettings {
                        global: WindowCloseBehavior::Hide,
                        ..WindowCloseSettings::default()
                    }
                },
                impact: false,
                owners: &[],
            },
            AppCase {
                field: "use_legacy_ui",
                mutate: |app| app.use_legacy_ui = true,
                impact: false,
                owners: &[],
            },
            AppCase {
                field: "theme_color",
                // Mutated in place: the color type is re-exported by neither
                // crate, so a case cannot name it to build one.
                mutate: |app| app.theme_color.r = 1.0,
                impact: false,
                owners: &[],
            },
        ];
        #[cfg(target_os = "macos")]
        cases.push(AppCase {
            field: "enable_macos_colored_icons",
            mutate: |app| app.enable_macos_colored_icons = true,
            impact: false,
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
                reaches_runtime(&previous, &candidate, &[]),
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
        assert!(!(reaches_runtime(&base_app(), &base_app(), &[])));
        assert!(
            owners(
                &effect_inputs(&base_app(), &clash),
                &effect_inputs(&base_app(), &clash)
            )
            .is_empty()
        );
    }

    struct ClashCase {
        field: &'static str,
        candidate: fn() -> ClashConfig,
        impact: bool,
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
            impact: true,
        },
        ClashCase {
            field: "overrides.allow_lan",
            candidate: || {
                overrides_patch(ClashGuardOverridesPatch {
                    allow_lan: Some(true),
                    ..ClashGuardOverridesPatch::default()
                })
            },
            impact: true,
        },
        ClashCase {
            field: "overrides.mode",
            candidate: || {
                overrides_patch(ClashGuardOverridesPatch {
                    mode: Some(Mode::Global),
                    ..ClashGuardOverridesPatch::default()
                })
            },
            impact: true,
        },
        ClashCase {
            field: "overrides.secret",
            candidate: || {
                overrides_patch(ClashGuardOverridesPatch {
                    secret: Some("a-new-secret".to_owned()),
                    ..ClashGuardOverridesPatch::default()
                })
            },
            impact: true,
        },
        ClashCase {
            field: "overrides.unified_delay",
            candidate: || {
                overrides_patch(ClashGuardOverridesPatch {
                    unified_delay: Some(ManageableField::Managed(false)),
                    ..ClashGuardOverridesPatch::default()
                })
            },
            impact: true,
        },
        ClashCase {
            field: "overrides.tcp_concurrent",
            candidate: || {
                overrides_patch(ClashGuardOverridesPatch {
                    tcp_concurrent: Some(ManageableField::Unmanaged),
                    ..ClashGuardOverridesPatch::default()
                })
            },
            impact: true,
        },
        ClashCase {
            field: "overrides.ipv6",
            candidate: || {
                overrides_patch(ClashGuardOverridesPatch {
                    ipv6: Some(true),
                    ..ClashGuardOverridesPatch::default()
                })
            },
            impact: true,
        },
        ClashCase {
            field: "enable_tun_mode",
            candidate: || ClashConfig {
                enable_tun_mode: true,
                ..base_clash()
            },
            impact: true,
        },
        ClashCase {
            field: "tun",
            candidate: || ClashConfig {
                tun: TunOverrides {
                    auto_route: ManageableField::Managed(false),
                },
                ..base_clash()
            },
            impact: true,
        },
        ClashCase {
            field: "tun_stack",
            candidate: || ClashConfig {
                tun_stack: TunStack::System,
                ..base_clash()
            },
            impact: true,
        },
        // The options of a stack that is not selected are not built.
        ClashCase {
            field: "tun_mips",
            candidate: || ClashConfig {
                tun_mips: MipsConfig {
                    congestion_controller: Some(TunCongestionController::Bbr),
                },
                ..base_clash()
            },
            impact: false,
        },
        ClashCase {
            field: "enable_clash_fields",
            candidate: || ClashConfig {
                enable_clash_fields: !base_clash().enable_clash_fields,
                ..base_clash()
            },
            impact: true,
        },
        ClashCase {
            field: "expand_include_all",
            candidate: || ClashConfig {
                expand_include_all: !base_clash().expand_include_all,
                ..base_clash()
            },
            impact: true,
        },
        ClashCase {
            field: "mixed_port",
            candidate: || ClashConfig {
                mixed_port: PortStrategy::new_allow_fallback(7999),
                ..base_clash()
            },
            impact: true,
        },
        ClashCase {
            field: "socks_port",
            candidate: || ClashConfig {
                socks_port: Some(PortStrategy::new_allow_fallback(1080)),
                ..base_clash()
            },
            impact: true,
        },
        ClashCase {
            field: "http_port",
            candidate: || ClashConfig {
                http_port: Some(PortStrategy::new_allow_fallback(1081)),
                ..base_clash()
            },
            impact: true,
        },
        ClashCase {
            field: "redir_port",
            candidate: || ClashConfig {
                redir_port: Some(PortStrategy::new_allow_fallback(12345)),
                ..base_clash()
            },
            impact: true,
        },
        ClashCase {
            field: "tproxy_port",
            candidate: || ClashConfig {
                tproxy_port: Some(PortStrategy::new_allow_fallback(12346)),
                ..base_clash()
            },
            impact: true,
        },
        ClashCase {
            field: "transparent_proxy",
            candidate: || ClashConfig {
                transparent_proxy: nyanpasu_config::clash::config::TransparentProxyConfig {
                    mode: nyanpasu_config::clash::config::TransparentProxyMode::Tproxy,
                    ..Default::default()
                },
                ..base_clash()
            },
            impact: true,
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
            impact: true,
        },
        ClashCase {
            field: "clash_control_channel",
            candidate: || ClashConfig {
                clash_control_channel: ClashControlChannel::HttpOnly,
                ..base_clash()
            },
            impact: true,
        },
        ClashCase {
            field: "clash_ipc_disable_http_controller",
            candidate: || ClashConfig {
                clash_ipc_disable_http_controller: true,
                ..base_clash()
            },
            impact: true,
        },
        // Read by the dashboard, never by a build stage.
        ClashCase {
            field: "web_ui_list",
            candidate: || ClashConfig {
                web_ui_list: vec!["http://example.invalid/ui".to_owned()],
                ..base_clash()
            },
            impact: false,
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
            impact: false,
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
                reaches_runtime(&previous, &candidate, &[]),
                case.impact,
                "{} runtime verdict",
                case.field
            );
        }
    }

    #[test]
    fn mips_options_are_built_while_mips_is_selected() {
        let previous = ClashConfig {
            tun_stack: TunStack::Mips,
            ..base_clash()
        };
        let candidate = ClashConfig {
            tun_mips: MipsConfig {
                congestion_controller: Some(TunCongestionController::Bbr),
            },
            ..previous.clone()
        };
        assert!(reaches_runtime(&previous, &candidate, &[]));
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
    fn each_auto_route_policy_changes_the_runtime_target() {
        let mut previous = base_clash();
        for auto_route in [
            ManageableField::Managed(false),
            ManageableField::Unmanaged,
            ManageableField::Managed(true),
        ] {
            let candidate = ClashConfig {
                tun: TunOverrides { auto_route },
                ..previous.clone()
            };
            assert!(reaches_runtime(&previous, &candidate, &[]));
            assert_ne!(
                clash_target(&previous).unwrap(),
                clash_target(&candidate).unwrap()
            );
            previous = candidate;
        }
    }

    #[test]
    fn an_unchanged_clash_config_has_no_runtime_impact() {
        assert!(!(reaches_runtime(&base_clash(), &base_clash(), &[])));
    }

    #[test]
    fn a_mode_change_is_both_a_rebuild_and_a_tray_input() {
        let previous = base_clash();
        let candidate = overrides_patch(ClashGuardOverridesPatch {
            mode: Some(Mode::Direct),
            ..ClashGuardOverridesPatch::default()
        });
        let app = base_app();

        assert!(reaches_runtime(&previous, &candidate, &[]));
        assert_eq!(
            owners(
                &effect_inputs(&app, &previous),
                &effect_inputs(&app, &candidate)
            ),
            vec![EffectKind::Tray],
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

        assert!(reaches_runtime(&previous, &candidate, &[]));
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
        profiles.append_item(test_support::file_config("sel", "sel.yaml", &["scoped"]));
        profiles.append_item(test_support::overlay("scoped", "scoped.yaml"));
        profiles.append_item(test_support::overlay("global", "global.yaml"));
        profiles.append_item(test_support::file_config("spare", "spare.yaml", &[]));
        profiles.current = Some(ProfileId("sel".into()));
        profiles.global_transforms = vec![ProfileId("global".into())];
        profiles
    }

    fn touching(file: &str) -> Vec<ManagedProfilePath> {
        vec![ManagedProfilePath::new(file).unwrap()]
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
                        materialized: test_support::managed(file),
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

        assert!(reaches_runtime(&previous, &candidate, &[]));
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
                        materialized: test_support::managed("spare-v2.yaml"),
                    },
                },
                transforms: Vec::new(),
            }),
        };

        assert!(!(reaches_runtime(&previous, &candidate, &[])));
    }

    #[test]
    fn touched_content_on_a_closure_path_affects_the_runtime_without_a_document_change() {
        let profiles = profiles();

        // The document is byte-identical: only the file behind `scoped` changed.
        assert!(reaches_runtime(
            &profiles,
            &profiles,
            &touching("scoped.yaml")
        ));
        assert!(reaches_runtime(&profiles, &profiles, &touching("sel.yaml")));
        assert!(reaches_runtime(
            &profiles,
            &profiles,
            &touching("global.yaml")
        ));
    }

    #[test]
    fn touched_content_outside_the_closure_does_not_affect_the_runtime() {
        let profiles = profiles();
        assert!(!(reaches_runtime(&profiles, &profiles, &touching("spare.yaml"))));
    }

    #[test]
    fn a_current_selection_change_affects_the_runtime() {
        let previous = profiles();
        let mut candidate = profiles();
        candidate.current = Some(ProfileId("spare".into()));

        assert!(reaches_runtime(&previous, &candidate, &[]));
    }

    /// The closure is a set, so reordering the global transforms leaves it
    /// equal while changing the order they run in.
    #[test]
    fn reordering_the_global_transforms_affects_the_runtime() {
        let mut previous = profiles();
        previous.append_item(test_support::overlay("global2", "global2.yaml"));
        previous.global_transforms = vec![ProfileId("global".into()), ProfileId("global2".into())];
        let mut candidate = previous.clone();
        candidate.global_transforms.reverse();

        assert_eq!(
            ProfilesActor::current_closure(&previous),
            ProfilesActor::current_closure(&candidate),
            "the closure must be set-equal for this test to mean anything"
        );
        assert!(reaches_runtime(&previous, &candidate, &[]));
    }

    #[test]
    fn a_retained_field_change_affects_the_runtime() {
        let previous = profiles();
        let mut candidate = profiles();
        candidate.valid.push("new-clash-field".to_owned());

        assert!(reaches_runtime(&previous, &candidate, &[]));
    }

    /// Metadata is a plain save: no build stage reads it, not even for the
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

        assert!(!(reaches_runtime(&previous, &candidate, &[])));
    }

    /// Folders and the display order only arrange the list: moving the running
    /// profile into a folder, or reordering the items, builds the same config.
    #[test]
    fn folders_and_display_order_have_no_runtime_impact() {
        use nyanpasu_config::profile::{FolderId, ProfileFolder, ProfileLayout};

        let previous = profiles();
        let mut candidate = profiles();
        candidate
            .apply_layout(ProfileLayout {
                folders: vec![ProfileFolder {
                    id: FolderId("f1".into()),
                    name: "Work".to_owned(),
                    members: vec![ProfileId("spare".into()), ProfileId("sel".into())],
                }],
                ungrouped: vec![],
            })
            .expect("the layout places every config");
        assert_ne!(
            previous.items.keys().collect::<Vec<_>>(),
            candidate.items.keys().collect::<Vec<_>>(),
            "the layout reordered the items"
        );

        assert!(!(reaches_runtime(&previous, &candidate, &[])));
    }

    /// A proxy group arrangement is read by the sidebar and the tray only: it
    /// never reaches the config the core runs.
    #[test]
    fn a_proxy_group_layout_has_no_runtime_impact() {
        use nyanpasu_config::profile::{FolderId, ProxyGroupFolder, ProxyGroupLayout};

        let previous = profiles();
        let mut candidate = profiles();
        candidate.proxy_group_layouts.insert(
            ProfileId("sel".into()),
            ProxyGroupLayout {
                folders: vec![ProxyGroupFolder {
                    id: FolderId("f1".into()),
                    name: "Streaming".to_owned(),
                    members: vec!["Netflix".to_owned()],
                }],
                ungrouped: vec!["Proxy".to_owned()],
            },
        );

        assert!(!(reaches_runtime(&previous, &candidate, &[])));
    }

    #[test]
    fn an_unchanged_profiles_document_has_no_runtime_impact() {
        let profiles = profiles();
        assert!(!(reaches_runtime(&profiles, &profiles, &[])));
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

        let changed = owners(
            &effect_inputs(&previous, &clash),
            &effect_inputs(&candidate, &clash),
        );
        assert!(
            !changed.contains(&EffectKind::SystemProxy),
            "a language change must not raise the system proxy's target"
        );

        // Independently of that, the system proxy may be behind: the fact lives
        // in its own status, not in the projection above.
        let unconverged = EffectStatus {
            kind: EffectKind::SystemProxy,
            desired_revision: EffectRevision::new(4),
            applied_revision: EffectRevision::new(3),
            health: EffectHealth::Degraded {
                code: EffectFailureCode::SystemProxyApplyFailed,
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

        assert!(!(reaches_runtime(&previous, &candidate, &[])));
        assert!(
            owners(
                &effect_inputs(&app, &previous),
                &effect_inputs(&app, &candidate)
            )
            .is_empty()
        );
    }

    /// A composite clash field is patched one sub-field at a time. Carrying any
    /// sub-field names the field as carrying the whole value did, and the
    /// verdict still comes from the committed candidate alone.
    #[test]
    fn a_nested_clash_patch_classifies_like_the_whole_value() {
        let cases = [
            (
                "mixed_port.start_port",
                ClashConfigPatch {
                    mixed_port: PortStrategyPatch {
                        start_port: Some(7891),
                        ..PortStrategyPatch::default()
                    },
                    ..ClashConfigPatch::default()
                },
                true,
            ),
            (
                "external_controller.port.kind",
                ClashConfigPatch {
                    external_controller: ExternalControllerStrategyPatch {
                        port: PortStrategyPatch {
                            kind: Some(PortStrategyKind::Fixed),
                            ..PortStrategyPatch::default()
                        },
                        ..ExternalControllerStrategyPatch::default()
                    },
                    ..ClashConfigPatch::default()
                },
                true,
            ),
            (
                "break_connection.on_mode_change",
                ClashConfigPatch {
                    break_connection: BreakConnectionStrategyPatch {
                        on_mode_change: Some(false),
                        ..BreakConnectionStrategyPatch::default()
                    },
                    ..ClashConfigPatch::default()
                },
                false,
            ),
        ];
        for (field, patch, impact) in cases {
            let previous = base_clash();
            let mut candidate = base_clash();
            candidate.apply(patch);
            assert_eq!(
                reaches_runtime(&previous, &candidate, &[]),
                impact,
                "{field}"
            );
        }
    }
}
