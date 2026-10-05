//! Pure projection + diff: which side effects a committed configuration change
//! implies.

use std::time::Duration;

use nyanpasu_config::{
    application::{
        ClashCore, CoreLogSettings, I18nLanguage, LoggingLevel, NetworkStatisticWidgetConfig,
        NyanpasuAppConfig, ProxiesSelectorMode, TrayMenuMode,
    },
    clash::config::{
        ClashConfig,
        overrides::{LogLevel, Mode},
    },
    runtime::executor::ResolvedPortBindings,
};
use struct_patch::Patch;

/// The only configuration an effect may depend on.
///
/// Effects diff this projection rather than the config structs: the resolved
/// mixed port lives in neither config, the configs carry platform-gated fields
/// no effect reads, and a field absent here provably cannot trigger an effect.
#[derive(Debug, Clone, PartialEq)]
pub struct ApplicationEffectInputs {
    pub app: ApplicationEffectFields,
    pub clash: ClashEffectFields,
    /// Ports resolved for this session; `None` until the first resolve.
    pub ports: Option<ResolvedPortBindings>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ApplicationEffectFields {
    pub enable_system_proxy: bool,
    pub system_proxy_bypass: String,
    pub enable_proxy_guard: bool,
    pub proxy_guard_interval: u64,
    pub pac_url: Option<url::Url>,
    pub enable_auto_launch: bool,
    pub hotkeys: Vec<String>,
    pub language: I18nLanguage,
    pub app_log_level: LoggingLevel,
    pub max_log_files: usize,
    pub max_log_file_size: u64,
    pub core_logs: CoreLogSettings,
    pub tray_selector_mode: ProxiesSelectorMode,
    pub tray_menu_mode: TrayMenuMode,
    pub enable_tray_text: bool,
    pub enable_tray_traffic: bool,
    pub network_statistic_widget: NetworkStatisticWidgetConfig,
    pub core: ClashCore,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClashEffectFields {
    pub mode: Mode,
    pub enable_tun_mode: bool,
    pub log_level: LogLevel,
}

/// The application owner's slice: what it hands the effects owner after a
/// commit.
impl From<&NyanpasuAppConfig> for ApplicationEffectFields {
    fn from(app: &NyanpasuAppConfig) -> Self {
        Self {
            enable_system_proxy: app.enable_system_proxy,
            system_proxy_bypass: app.system_proxy_bypass.clone(),
            enable_proxy_guard: app.enable_proxy_guard,
            proxy_guard_interval: app.proxy_guard_interval,
            pac_url: app.pac_url.clone(),
            enable_auto_launch: app.enable_auto_launch,
            hotkeys: app.hotkeys.clone(),
            language: app.language,
            app_log_level: app.app_log_level.clone(),
            max_log_files: app.max_log_files,
            max_log_file_size: app.max_log_file_size,
            core_logs: app.core_logs,
            tray_selector_mode: app.tray_selector_mode,
            tray_menu_mode: app.tray_menu_mode,
            enable_tray_text: app.enable_tray_text,
            enable_tray_traffic: app.enable_tray_traffic,
            network_statistic_widget: app.network_statistic_widget,
            core: app.core,
        }
    }
}

/// The clash config owner's slice.
impl From<&ClashConfig> for ClashEffectFields {
    fn from(clash: &ClashConfig) -> Self {
        Self {
            mode: clash.overrides.mode(),
            enable_tun_mode: clash.enable_tun_mode,
            log_level: clash.overrides.log_level(),
        }
    }
}

impl ApplicationEffectInputs {
    /// All three slices at once, for a reader that is not one of their owners.
    /// Ports come from the session resolver rather than the config, so this is
    /// a projection with three sources and not a `From` impl.
    pub fn project(
        app: &NyanpasuAppConfig,
        clash: &ClashConfig,
        ports: Option<ResolvedPortBindings>,
    ) -> Self {
        Self {
            app: app.into(),
            clash: clash.into(),
            ports,
        }
    }

    /// The desired value of every effect, regrouped so that each group holds
    /// exactly the inputs of one effect. Both [`ApplicationEffectPlan::diff`]
    /// and [`ApplicationEffectPlan::full`] go through this single mapping.
    fn desired(&self) -> ApplicationDesired {
        let app = &self.app;
        ApplicationDesired {
            locale: app.language,
            logger: LoggerDesired {
                level: app.app_log_level.clone(),
                max_files: app.max_log_files,
                max_file_size: app.max_log_file_size,
            },
            core_log_level: self.clash.log_level,
            core_log_storage: app.core_logs,
            auto_launch: app.enable_auto_launch,
            system_proxy: SystemProxyDesired {
                enabled: app.enable_system_proxy,
                bypass: app.system_proxy_bypass.clone(),
                port: self.ports.as_ref().map(|ports| ports.mixed_port),
                pac_url: app.pac_url.clone(),
            },
            proxy_guard: ProxyGuardDesired {
                enabled: app.enable_proxy_guard,
                interval: Duration::from_secs(app.proxy_guard_interval),
            },
            hotkeys: app.hotkeys.clone(),
            widget: app.network_statistic_widget,
            tray_menu: TrayMenuDesired {
                menu_mode: app.tray_menu_mode,
                selector_mode: app.tray_selector_mode,
                core: app.core,
            },
            tray_part: TrayPartDesired {
                mode: self.clash.mode,
                system_proxy: app.enable_system_proxy,
                tun: self.clash.enable_tun_mode,
                text: app.enable_tray_text,
                traffic: app.enable_tray_traffic,
            },
        }
    }

    /// What the tray renders from this snapshot.
    pub fn tray_view(&self) -> TrayView {
        self.desired().tray_view()
    }
}

/// Execution order of a plan. The ordering is load-bearing: the tray menu is
/// rendered with the process-wide locale, and the proxy guard re-applies the
/// system proxy value that the `SystemProxy` effect just installed.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum EffectKind {
    Locale,
    Logger,
    CoreLogLevel,
    CoreLogStorage,
    AutoLaunch,
    SystemProxy,
    ProxyGuard,
    Hotkeys,
    Widget,
    Tray,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayRefresh {
    Full,
    Part,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SystemProxyDesired {
    pub enabled: bool,
    pub bypass: String,
    /// Resolved mixed port; `None` while the session has not resolved ports.
    pub port: Option<u16>,
    pub pac_url: Option<url::Url>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProxyGuardDesired {
    pub enabled: bool,
    pub interval: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoggerDesired {
    pub level: LoggingLevel,
    pub max_files: usize,
    pub max_file_size: u64,
}

/// What a [`TrayRefresh::Full`] rebuild renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrayMenuDesired {
    pub menu_mode: TrayMenuMode,
    pub selector_mode: ProxiesSelectorMode,
    /// Only a Premium core offers the script mode item.
    pub core: ClashCore,
}

/// What a [`TrayRefresh::Part`] redraw reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrayPartDesired {
    pub mode: Mode,
    pub system_proxy: bool,
    pub tun: bool,
    pub text: bool,
    pub traffic: bool,
}

/// Everything the tray renders, whichever of the two groups changed.
///
/// A part redraw still carries the menu inputs: the tray keeps the latest view
/// and renders every later rebuild from it, including the ones it triggers
/// itself when the proxy list changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrayView {
    pub menu: TrayMenuDesired,
    pub part: TrayPartDesired,
}

/// One field per trigger group: a group whose inputs changed produces the one
/// effect it belongs to, and the two tray groups collapse into a single `Tray`
/// effect.
///
/// `into_patch_by_diff` yields `Some(whole desired value)` for every group whose
/// inputs changed and `into_patch` yields all of them, so the incremental and
/// the full plan share one group-to-effect mapping. Execution order comes from
/// the order that mapping emits effects in, not from the order of the fields
/// below, which are merely kept in [`EffectKind`] order to read alongside it.
#[derive(Debug, Clone, PartialEq, Patch)]
#[patch(name = "ApplicationDesiredChanges")]
#[patch(attribute(derive(Debug, Clone, Default, PartialEq)))]
struct ApplicationDesired {
    locale: I18nLanguage,
    logger: LoggerDesired,
    /// The level the Core logs are captured at.
    core_log_level: LogLevel,
    /// Rotation and compression of the stored Core logs.
    core_log_storage: CoreLogSettings,
    auto_launch: bool,
    system_proxy: SystemProxyDesired,
    /// Split from `system_proxy` on purpose: changing only the interval must
    /// not re-apply the OS proxy, and changing only the bypass must not
    /// restart the guard timer.
    proxy_guard: ProxyGuardDesired,
    hotkeys: Vec<String>,
    widget: NetworkStatisticWidgetConfig,
    /// `locale` is deliberately not repeated here; "a locale change implies a
    /// full tray refresh" is written out in the mapping instead.
    tray_menu: TrayMenuDesired,
    tray_part: TrayPartDesired,
}

impl ApplicationDesired {
    fn tray_view(&self) -> TrayView {
        TrayView {
            menu: self.tray_menu,
            part: self.tray_part,
        }
    }
}

/// Every variant carries the full desired value, never a delta. Stale-effect
/// protection depends on it: a newer revision may overwrite an older one
/// wholesale, and a dropped effect loses nothing permanently.
#[derive(Debug, Clone, PartialEq)]
pub enum ApplicationEffect {
    Locale(I18nLanguage),
    Logger(LoggerDesired),
    CoreLogLevel(LogLevel),
    CoreLogStorage(CoreLogSettings),
    AutoLaunch(bool),
    SystemProxy(SystemProxyDesired),
    ProxyGuard(ProxyGuardDesired),
    Hotkeys(Vec<String>),
    Widget(NetworkStatisticWidgetConfig),
    Tray(TrayRefresh, TrayView),
}

impl ApplicationEffect {
    pub fn kind(&self) -> EffectKind {
        match self {
            Self::Locale(_) => EffectKind::Locale,
            Self::Logger(_) => EffectKind::Logger,
            Self::CoreLogLevel(_) => EffectKind::CoreLogLevel,
            Self::CoreLogStorage(_) => EffectKind::CoreLogStorage,
            Self::AutoLaunch(_) => EffectKind::AutoLaunch,
            Self::SystemProxy(_) => EffectKind::SystemProxy,
            Self::ProxyGuard(_) => EffectKind::ProxyGuard,
            Self::Hotkeys(_) => EffectKind::Hotkeys,
            Self::Widget(_) => EffectKind::Widget,
            Self::Tray(..) => EffectKind::Tray,
        }
    }
}

/// Effects to run after a commit, at most one per [`EffectKind`], ordered by it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ApplicationEffectPlan {
    effects: Vec<ApplicationEffect>,
}

impl ApplicationEffectPlan {
    pub(crate) fn from_effects(effects: Vec<ApplicationEffect>) -> Self {
        Self { effects }
    }

    /// Incremental: fields that are equal in both snapshots produce nothing.
    pub fn diff(before: &ApplicationEffectInputs, after: &ApplicationEffectInputs) -> Self {
        let desired = after.desired();
        let tray = desired.tray_view();
        Self::from_changes(desired.into_patch_by_diff(before.desired()), tray)
    }

    /// Full reconcile: the desired value of every effect, used at startup where
    /// no trustworthy "before" snapshot exists.
    pub fn full(after: &ApplicationEffectInputs) -> Self {
        let desired = after.desired();
        let tray = desired.tray_view();
        Self::from_changes(desired.into_patch(), tray)
    }

    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }

    pub fn effects(&self) -> &[ApplicationEffect] {
        &self.effects
    }
}

impl ApplicationEffectPlan {
    /// `tray` is the whole view of the same snapshot: a change set only holds
    /// the tray group that moved, and the tray effect carries both.
    fn from_changes(changes: ApplicationDesiredChanges, tray: TrayView) -> Self {
        // Destructured without `..` on purpose: a new effect cannot be added to
        // `ApplicationDesired` without being mapped here.
        let ApplicationDesiredChanges {
            locale,
            logger,
            core_log_level,
            core_log_storage,
            auto_launch,
            system_proxy,
            proxy_guard,
            hotkeys,
            widget,
            tray_menu,
            tray_part,
        } = changes;

        // Pushed in `EffectKind` order, which is the plan's ordering invariant.
        let mut effects = Vec::new();
        effects.extend(locale.map(ApplicationEffect::Locale));
        effects.extend(logger.map(ApplicationEffect::Logger));
        effects.extend(core_log_level.map(ApplicationEffect::CoreLogLevel));
        effects.extend(core_log_storage.map(ApplicationEffect::CoreLogStorage));
        effects.extend(auto_launch.map(ApplicationEffect::AutoLaunch));
        effects.extend(system_proxy.map(ApplicationEffect::SystemProxy));
        effects.extend(proxy_guard.map(ApplicationEffect::ProxyGuard));
        effects.extend(hotkeys.map(ApplicationEffect::Hotkeys));
        effects.extend(widget.map(ApplicationEffect::Widget));
        // The menu is rendered with the locale, so a locale change is a menu
        // change. A full refresh rebuilds the menu and refreshes the parts on
        // its way out, so it subsumes a part refresh.
        let refresh = match (locale.is_some() || tray_menu.is_some(), tray_part.is_some()) {
            (true, _) => Some(TrayRefresh::Full),
            (false, true) => Some(TrayRefresh::Part),
            (false, false) => None,
        };
        effects.extend(refresh.map(|refresh| ApplicationEffect::Tray(refresh, tray)));

        Self { effects }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nyanpasu_config::{
        application::{
            ClashCore, I18nLanguage, LoggingLevel, NetworkStatisticWidgetConfig, NyanpasuAppConfig,
            ProxiesSelectorMode, TrayMenuMode,
        },
        clash::config::{
            ClashConfig,
            overrides::{ClashGuardOverridesPatch, Mode},
        },
        runtime::executor::ResolvedPortBindings,
    };
    use nyanpasu_helper::StatisticWidgetVariant;

    fn inputs() -> ApplicationEffectInputs {
        ApplicationEffectInputs {
            app: ApplicationEffectFields {
                enable_system_proxy: false,
                system_proxy_bypass: "localhost".to_owned(),
                enable_proxy_guard: false,
                proxy_guard_interval: 30,
                pac_url: None,
                enable_auto_launch: false,
                hotkeys: vec!["open_or_close_dashboard,Ctrl+Q".to_owned()],
                language: I18nLanguage::English,
                app_log_level: LoggingLevel::Info,
                max_log_files: 7,
                max_log_file_size: 10,
                core_logs: CoreLogSettings::default(),
                tray_selector_mode: ProxiesSelectorMode::Normal,
                tray_menu_mode: TrayMenuMode::Native,
                enable_tray_text: false,
                enable_tray_traffic: false,
                network_statistic_widget: NetworkStatisticWidgetConfig::Disabled,
                core: ClashCore::Mihomo,
            },
            clash: ClashEffectFields {
                mode: Mode::Rule,
                enable_tun_mode: false,
                log_level: LogLevel::Info,
            },
            ports: Some(ResolvedPortBindings {
                mixed_port: 7890,
                ..ResolvedPortBindings::default()
            }),
        }
    }

    fn kinds(plan: &ApplicationEffectPlan) -> Vec<EffectKind> {
        plan.effects().iter().map(ApplicationEffect::kind).collect()
    }

    /// A tray effect always carries the whole view of the snapshot it was
    /// planned from.
    fn tray(refresh: TrayRefresh, inputs: &ApplicationEffectInputs) -> ApplicationEffect {
        ApplicationEffect::Tray(refresh, inputs.tray_view())
    }

    #[test]
    fn empty_diff_produces_no_effects() {
        let plan = ApplicationEffectPlan::diff(&inputs(), &inputs());
        assert!(plan.is_empty(), "unchanged inputs must plan nothing");
        assert!(plan.effects().is_empty());
    }

    #[test]
    fn system_proxy_toggle_produces_system_proxy_effect() {
        let before = inputs();
        let mut after = inputs();
        after.app.enable_system_proxy = true;

        let plan = ApplicationEffectPlan::diff(&before, &after);

        assert_eq!(
            plan.effects()[0],
            ApplicationEffect::SystemProxy(SystemProxyDesired {
                enabled: true,
                bypass: "localhost".to_owned(),
                port: Some(7890),
                pac_url: None,
            })
        );
    }

    #[test]
    fn bypass_change_produces_system_proxy_effect() {
        let before = inputs();
        let mut after = inputs();
        after.app.system_proxy_bypass = "127.0.0.1,<local>".to_owned();

        let plan = ApplicationEffectPlan::diff(&before, &after);

        assert_eq!(kinds(&plan), vec![EffectKind::SystemProxy]);
    }

    #[test]
    fn port_change_alone_produces_system_proxy_effect() {
        let before = inputs();
        let mut after = inputs();
        after.ports = Some(ResolvedPortBindings {
            mixed_port: 7891,
            ..ResolvedPortBindings::default()
        });

        let plan = ApplicationEffectPlan::diff(&before, &after);

        assert_eq!(
            plan.effects()[0],
            ApplicationEffect::SystemProxy(SystemProxyDesired {
                enabled: false,
                bypass: "localhost".to_owned(),
                port: Some(7891),
                pac_url: None,
            })
        );
    }

    #[test]
    fn pac_url_change_produces_system_proxy_effect() {
        let pac = url::Url::parse("http://127.0.0.1:11233/commands/pac").unwrap();
        let before = inputs();
        let mut after = inputs();
        after.app.pac_url = Some(pac.clone());

        let plan = ApplicationEffectPlan::diff(&before, &after);

        assert_eq!(
            plan.effects()[0],
            ApplicationEffect::SystemProxy(SystemProxyDesired {
                enabled: false,
                bypass: "localhost".to_owned(),
                port: Some(7890),
                pac_url: Some(pac),
            })
        );
    }

    #[test]
    fn pac_url_removal_produces_system_proxy_effect() {
        let pac = url::Url::parse("http://127.0.0.1:11233/commands/pac").unwrap();
        let mut before = inputs();
        before.app.pac_url = Some(pac);
        let after = inputs();

        let plan = ApplicationEffectPlan::diff(&before, &after);

        assert_eq!(kinds(&plan), vec![EffectKind::SystemProxy]);
        assert_eq!(
            plan.effects()[0],
            ApplicationEffect::SystemProxy(SystemProxyDesired {
                enabled: false,
                bypass: "localhost".to_owned(),
                port: Some(7890),
                pac_url: None,
            })
        );
    }

    #[test]
    fn guard_interval_change_produces_only_proxy_guard() {
        let before = inputs();
        let mut after = inputs();
        after.app.proxy_guard_interval = 60;

        let plan = ApplicationEffectPlan::diff(&before, &after);

        assert_eq!(kinds(&plan), vec![EffectKind::ProxyGuard]);
        assert_eq!(
            plan.effects()[0],
            ApplicationEffect::ProxyGuard(ProxyGuardDesired {
                enabled: false,
                interval: Duration::from_secs(60),
            })
        );
    }

    #[test]
    fn language_change_produces_locale_and_full_tray() {
        let before = inputs();
        let mut after = inputs();
        after.app.language = I18nLanguage::SimplifiedChinese;

        let plan = ApplicationEffectPlan::diff(&before, &after);

        assert_eq!(kinds(&plan), vec![EffectKind::Locale, EffectKind::Tray]);
        assert_eq!(
            plan.effects()[0],
            ApplicationEffect::Locale(I18nLanguage::SimplifiedChinese)
        );
        assert_eq!(plan.effects()[1], tray(TrayRefresh::Full, &after));
    }

    #[test]
    fn system_proxy_change_alone_produces_part_tray() {
        let before = inputs();
        let mut after = inputs();
        after.app.enable_system_proxy = true;

        let plan = ApplicationEffectPlan::diff(&before, &after);

        assert_eq!(
            kinds(&plan),
            vec![EffectKind::SystemProxy, EffectKind::Tray]
        );
        assert_eq!(plan.effects()[1], tray(TrayRefresh::Part, &after));
    }

    #[test]
    fn language_and_system_proxy_change_produces_full_tray_only() {
        let before = inputs();
        let mut after = inputs();
        after.app.language = I18nLanguage::Korean;
        after.app.enable_system_proxy = true;

        let plan = ApplicationEffectPlan::diff(&before, &after);

        let trays: Vec<_> = plan
            .effects()
            .iter()
            .filter(|effect| effect.kind() == EffectKind::Tray)
            .collect();
        assert_eq!(trays, vec![&tray(TrayRefresh::Full, &after)]);
    }

    #[test]
    fn log_level_change_produces_logger_effect() {
        let before = inputs();
        let mut after = inputs();
        after.app.app_log_level = LoggingLevel::Debug;

        let plan = ApplicationEffectPlan::diff(&before, &after);

        assert_eq!(kinds(&plan), vec![EffectKind::Logger]);
        assert_eq!(
            plan.effects()[0],
            ApplicationEffect::Logger(LoggerDesired {
                level: LoggingLevel::Debug,
                max_files: 7,
                max_file_size: 10,
            })
        );
    }

    #[test]
    fn core_log_level_change_produces_only_its_effect() {
        let before = inputs();
        let mut after = inputs();
        after.clash.log_level = LogLevel::Silent;

        let plan = ApplicationEffectPlan::diff(&before, &after);

        assert_eq!(
            plan.effects(),
            [ApplicationEffect::CoreLogLevel(LogLevel::Silent)]
        );
    }

    #[test]
    fn core_log_storage_change_produces_only_its_effect() {
        let before = inputs();
        let mut after = inputs();
        after.app.core_logs.compression = nyanpasu_config::application::CoreLogCompression::None;

        let plan = ApplicationEffectPlan::diff(&before, &after);

        assert_eq!(
            plan.effects(),
            [ApplicationEffect::CoreLogStorage(after.app.core_logs)]
        );
    }

    #[test]
    fn widget_change_produces_widget_effect() {
        let before = inputs();
        let mut after = inputs();
        after.app.network_statistic_widget =
            NetworkStatisticWidgetConfig::Enabled(StatisticWidgetVariant::Small);

        let plan = ApplicationEffectPlan::diff(&before, &after);

        assert_eq!(
            plan.effects(),
            [ApplicationEffect::Widget(
                NetworkStatisticWidgetConfig::Enabled(StatisticWidgetVariant::Small)
            )]
        );
    }

    #[test]
    fn hotkeys_change_produces_hotkeys_effect() {
        let before = inputs();
        let mut after = inputs();
        after.app.hotkeys = vec!["clash_mode_rule,Ctrl+R".to_owned()];

        let plan = ApplicationEffectPlan::diff(&before, &after);

        assert_eq!(
            plan.effects(),
            [ApplicationEffect::Hotkeys(vec![
                "clash_mode_rule,Ctrl+R".to_owned()
            ])]
        );
    }

    #[test]
    fn effects_are_sorted_by_kind() {
        let before = inputs();
        let mut after = inputs();
        after.app.language = I18nLanguage::Russian;
        after.app.app_log_level = LoggingLevel::Trace;
        after.app.enable_auto_launch = true;
        after.app.enable_system_proxy = true;
        after.app.enable_proxy_guard = true;
        after.app.hotkeys = vec!["clash_mode_global,Ctrl+G".to_owned()];
        after.app.network_statistic_widget =
            NetworkStatisticWidgetConfig::Enabled(StatisticWidgetVariant::Large);

        let plan = ApplicationEffectPlan::diff(&before, &after);

        assert_eq!(
            kinds(&plan),
            vec![
                EffectKind::Locale,
                EffectKind::Logger,
                EffectKind::AutoLaunch,
                EffectKind::SystemProxy,
                EffectKind::ProxyGuard,
                EffectKind::Hotkeys,
                EffectKind::Widget,
                EffectKind::Tray,
            ]
        );
        assert!(
            kinds(&plan).is_sorted(),
            "plan order must follow EffectKind"
        );
    }

    #[test]
    fn full_plan_contains_every_kind_with_full_tray() {
        let inputs = inputs();
        let plan = ApplicationEffectPlan::full(&inputs);

        assert_eq!(
            kinds(&plan),
            vec![
                EffectKind::Locale,
                EffectKind::Logger,
                EffectKind::CoreLogLevel,
                EffectKind::CoreLogStorage,
                EffectKind::AutoLaunch,
                EffectKind::SystemProxy,
                EffectKind::ProxyGuard,
                EffectKind::Hotkeys,
                EffectKind::Widget,
                EffectKind::Tray,
            ]
        );
        assert_eq!(
            plan.effects().last(),
            Some(&tray(TrayRefresh::Full, &inputs))
        );
    }

    /// A group holds several fields, and dropping one from its desired struct
    /// still compiles while silently disarming the effect. One case per field
    /// that no other test moves on its own.
    #[test]
    fn every_group_field_triggers_its_effect_on_its_own() {
        fn assert_case(
            label: &str,
            mutate: impl FnOnce(&mut ApplicationEffectInputs),
            expected: impl FnOnce(&ApplicationEffectInputs) -> Vec<ApplicationEffect>,
        ) {
            let before = inputs();
            let mut after = inputs();
            mutate(&mut after);

            let plan = ApplicationEffectPlan::diff(&before, &after);

            assert_eq!(
                plan.effects(),
                expected(&after),
                "{label} alone must plan exactly its own effect"
            );
        }

        assert_case(
            "max_log_files",
            |after| after.app.max_log_files = 14,
            |_| {
                vec![ApplicationEffect::Logger(LoggerDesired {
                    level: LoggingLevel::Info,
                    max_files: 14,
                    max_file_size: 10,
                })]
            },
        );
        assert_case(
            "max_log_file_size",
            |after| after.app.max_log_file_size = 20,
            |_| {
                vec![ApplicationEffect::Logger(LoggerDesired {
                    level: LoggingLevel::Info,
                    max_files: 7,
                    max_file_size: 20,
                })]
            },
        );
        assert_case(
            "tray_selector_mode",
            |after| after.app.tray_selector_mode = ProxiesSelectorMode::Submenu,
            |after| vec![tray(TrayRefresh::Full, after)],
        );
        assert_case(
            "core",
            |after| after.app.core = ClashCore::ClashPremium,
            |after| vec![tray(TrayRefresh::Full, after)],
        );
        assert_case(
            "mode",
            |after| after.clash.mode = Mode::Global,
            |after| vec![tray(TrayRefresh::Part, after)],
        );
        assert_case(
            "enable_tray_text",
            |after| after.app.enable_tray_text = true,
            |after| vec![tray(TrayRefresh::Part, after)],
        );
        assert_case(
            "enable_tray_traffic",
            |after| after.app.enable_tray_traffic = true,
            |after| vec![tray(TrayRefresh::Part, after)],
        );
        assert_case(
            "enable_proxy_guard",
            |after| after.app.enable_proxy_guard = true,
            |_| {
                vec![ApplicationEffect::ProxyGuard(ProxyGuardDesired {
                    enabled: true,
                    interval: Duration::from_secs(30),
                })]
            },
        );
        assert_case(
            "enable_auto_launch",
            |after| after.app.enable_auto_launch = true,
            |_| vec![ApplicationEffect::AutoLaunch(true)],
        );
    }

    #[test]
    fn unchanged_inputs_have_empty_changes() {
        use struct_patch::Status;

        assert!(
            inputs()
                .desired()
                .into_patch_by_diff(inputs().desired())
                .is_empty(),
            "no group may report a change for identical snapshots"
        );
    }

    #[test]
    fn tray_menu_mode_change_produces_full_tray_only() {
        let before = inputs();
        let mut after = inputs();
        after.app.tray_menu_mode = TrayMenuMode::Webview;

        let plan = ApplicationEffectPlan::diff(&before, &after);

        assert_eq!(kinds(&plan), vec![EffectKind::Tray]);
        assert_eq!(plan.effects()[0], tray(TrayRefresh::Full, &after));
    }

    #[test]
    fn tun_toggle_alone_produces_part_tray() {
        let before = inputs();
        let mut after = inputs();
        after.clash.enable_tun_mode = true;

        let plan = ApplicationEffectPlan::diff(&before, &after);

        assert_eq!(kinds(&plan), vec![EffectKind::Tray]);
        assert_eq!(plan.effects()[0], tray(TrayRefresh::Part, &after));
    }

    #[test]
    fn service_mode_is_not_an_effect_input_and_core_only_shapes_the_tray() {
        let app = NyanpasuAppConfig::default();
        let clash = ClashConfig::default();
        let before = ApplicationEffectInputs::project(&app, &clash, None);

        let host_switched = NyanpasuAppConfig {
            enable_service_mode: !app.enable_service_mode,
            ..app.clone()
        };
        let after = ApplicationEffectInputs::project(&host_switched, &clash, None);
        assert_eq!(
            before, after,
            "service mode may not enter the effect inputs"
        );

        // The core reaches the effects only through the tray menu it shapes.
        let core_switched = NyanpasuAppConfig {
            core: match app.core {
                ClashCore::Mihomo => ClashCore::ClashRs,
                _ => ClashCore::Mihomo,
            },
            ..app.clone()
        };
        let after = ApplicationEffectInputs::project(&core_switched, &clash, None);
        assert_eq!(
            ApplicationEffectPlan::diff(&before, &after).effects(),
            [tray(TrayRefresh::Full, &after)]
        );
    }

    #[test]
    fn the_tray_view_reads_core_and_mode_from_the_typed_configs() {
        let app = NyanpasuAppConfig {
            core: ClashCore::ClashPremium,
            tray_menu_mode: TrayMenuMode::Webview,
            tray_selector_mode: ProxiesSelectorMode::Submenu,
            enable_system_proxy: true,
            enable_tray_text: true,
            enable_tray_traffic: false,
            ..NyanpasuAppConfig::default()
        };
        let mut clash = ClashConfig {
            enable_tun_mode: true,
            ..ClashConfig::default()
        };
        clash.overrides.apply(ClashGuardOverridesPatch {
            mode: Some(Mode::Script),
            ..ClashGuardOverridesPatch::default()
        });

        let view = ApplicationEffectInputs::project(&app, &clash, None).tray_view();

        assert_eq!(
            view,
            TrayView {
                menu: TrayMenuDesired {
                    menu_mode: TrayMenuMode::Webview,
                    selector_mode: ProxiesSelectorMode::Submenu,
                    core: ClashCore::ClashPremium,
                },
                part: TrayPartDesired {
                    mode: Mode::Script,
                    system_proxy: true,
                    tun: true,
                    text: true,
                    traffic: false,
                },
            }
        );
    }
}

/// Named owner inputs matter even when a user saves the same failed value.
pub(crate) fn requested_owners(
    patch: &nyanpasu_config::application::NyanpasuAppConfigPatch,
) -> Vec<EffectKind> {
    let mut kinds = Vec::new();
    if patch.enable_system_proxy.is_some()
        || patch.system_proxy_bypass.is_some()
        || patch.pac_url.is_some()
    {
        kinds.push(EffectKind::SystemProxy);
    }
    if patch.enable_proxy_guard.is_some() || patch.proxy_guard_interval.is_some() {
        kinds.push(EffectKind::ProxyGuard);
    }
    if patch.hotkeys.is_some() {
        kinds.push(EffectKind::Hotkeys);
    }
    if patch.enable_auto_launch.is_some() {
        kinds.push(EffectKind::AutoLaunch);
    }
    if patch.language.is_some() {
        kinds.push(EffectKind::Locale);
    }
    if patch.app_log_level.is_some()
        || patch.max_log_files.is_some()
        || patch.max_log_file_size.is_some()
    {
        kinds.push(EffectKind::Logger);
    }
    if patch.core_logs.is_some() {
        kinds.push(EffectKind::CoreLogStorage);
    }
    if patch.network_statistic_widget.is_some() {
        kinds.push(EffectKind::Widget);
    }
    if patch.language.is_some()
        || patch.tray_menu_mode.is_some()
        || patch.tray_selector_mode.is_some()
        || patch.enable_tray_text.is_some()
        || patch.enable_tray_traffic.is_some()
        || patch.enable_system_proxy.is_some()
    {
        kinds.push(EffectKind::Tray);
    }
    kinds
}
