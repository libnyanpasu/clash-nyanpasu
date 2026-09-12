//! Pure projection + diff: which side effects a committed configuration change
//! implies, and whether the runtime config has to be rebuilt.

use std::time::Duration;

use nyanpasu_config::{
    application::{
        I18nLanguage, LoggingLevel, NetworkStatisticWidgetConfig, NyanpasuAppConfig,
        ProxiesSelectorMode, TrayMenuMode,
    },
    clash::config::{
        ClashConfig, ClashControlChannel,
        clash_strategy::port::{ExternalControllerStrategy, PortStrategy},
        tun_stack::TunStack,
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
    pub tray_selector_mode: ProxiesSelectorMode,
    pub tray_menu_mode: TrayMenuMode,
    pub enable_tray_text: bool,
    pub enable_tray_traffic: bool,
    pub network_statistic_widget: NetworkStatisticWidgetConfig,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClashEffectFields {
    pub enable_tun_mode: bool,
    pub tun_stack: TunStack,
    pub mixed_port: PortStrategy,
    pub socks_port: Option<PortStrategy>,
    pub http_port: Option<PortStrategy>,
    pub external_controller: ExternalControllerStrategy,
    pub enable_clash_fields: bool,
    pub clash_control_channel: ClashControlChannel,
    pub clash_ipc_disable_http_controller: bool,
}

impl ApplicationEffectInputs {
    /// Ports come from the session resolver rather than the config, so this is
    /// a projection with three sources and not a `From` impl.
    pub fn project(
        app: &NyanpasuAppConfig,
        clash: &ClashConfig,
        ports: Option<ResolvedPortBindings>,
    ) -> Self {
        Self {
            app: ApplicationEffectFields {
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
                tray_selector_mode: app.tray_selector_mode,
                tray_menu_mode: app.tray_menu_mode,
                enable_tray_text: app.enable_tray_text,
                enable_tray_traffic: app.enable_tray_traffic,
                network_statistic_widget: app.network_statistic_widget,
            },
            clash: ClashEffectFields {
                enable_tun_mode: clash.enable_tun_mode,
                tun_stack: clash.tun_stack,
                mixed_port: clash.mixed_port.clone(),
                socks_port: clash.socks_port.clone(),
                http_port: clash.http_port.clone(),
                external_controller: clash.external_controller.clone(),
                enable_clash_fields: clash.enable_clash_fields,
                clash_control_channel: clash.clash_control_channel,
                clash_ipc_disable_http_controller: clash.clash_ipc_disable_http_controller,
            },
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
            },
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
            },
            tray_part: TrayPartDesired {
                system_proxy: app.enable_system_proxy,
                tun: self.clash.enable_tun_mode,
                text: app.enable_tray_text,
                traffic: app.enable_tray_traffic,
            },
        }
    }
}

/// Execution order of a plan. The ordering is load-bearing: the tray menu is
/// rendered with the process-wide locale, and the proxy guard re-applies the
/// system proxy value that the `SystemProxy` effect just installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EffectKind {
    Locale,
    Logger,
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
}

/// What a [`TrayRefresh::Full`] rebuild renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TrayMenuDesired {
    menu_mode: TrayMenuMode,
    selector_mode: ProxiesSelectorMode,
}

/// What a [`TrayRefresh::Part`] redraw reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TrayPartDesired {
    system_proxy: bool,
    tun: bool,
    text: bool,
    traffic: bool,
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

/// Every variant carries the full desired value, never a delta. Stale-effect
/// protection depends on it: a newer revision may overwrite an older one
/// wholesale, and a dropped effect loses nothing permanently.
#[derive(Debug, Clone, PartialEq)]
pub enum ApplicationEffect {
    Locale(I18nLanguage),
    Logger(LoggerDesired),
    AutoLaunch(bool),
    SystemProxy(SystemProxyDesired),
    ProxyGuard(ProxyGuardDesired),
    Hotkeys(Vec<String>),
    Widget(NetworkStatisticWidgetConfig),
    Tray(TrayRefresh),
}

impl ApplicationEffect {
    pub fn kind(&self) -> EffectKind {
        match self {
            Self::Locale(_) => EffectKind::Locale,
            Self::Logger(_) => EffectKind::Logger,
            Self::AutoLaunch(_) => EffectKind::AutoLaunch,
            Self::SystemProxy(_) => EffectKind::SystemProxy,
            Self::ProxyGuard(_) => EffectKind::ProxyGuard,
            Self::Hotkeys(_) => EffectKind::Hotkeys,
            Self::Widget(_) => EffectKind::Widget,
            Self::Tray(_) => EffectKind::Tray,
        }
    }
}

/// Effects to run after a commit, at most one per [`EffectKind`], ordered by it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ApplicationEffectPlan {
    effects: Vec<ApplicationEffect>,
}

impl ApplicationEffectPlan {
    /// Incremental: fields that are equal in both snapshots produce nothing.
    pub fn diff(before: &ApplicationEffectInputs, after: &ApplicationEffectInputs) -> Self {
        after.desired().into_patch_by_diff(before.desired()).into()
    }

    /// Full reconcile: the desired value of every effect, used at startup where
    /// no trustworthy "before" snapshot exists.
    pub fn full(after: &ApplicationEffectInputs) -> Self {
        after.desired().into_patch().into()
    }

    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }

    pub fn effects(&self) -> &[ApplicationEffect] {
        &self.effects
    }
}

impl From<ApplicationDesiredChanges> for ApplicationEffectPlan {
    fn from(changes: ApplicationDesiredChanges) -> Self {
        // Destructured without `..` on purpose: a new effect cannot be added to
        // `ApplicationDesired` without being mapped here.
        let ApplicationDesiredChanges {
            locale,
            logger,
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
        effects.extend(auto_launch.map(ApplicationEffect::AutoLaunch));
        effects.extend(system_proxy.map(ApplicationEffect::SystemProxy));
        effects.extend(proxy_guard.map(ApplicationEffect::ProxyGuard));
        effects.extend(hotkeys.map(ApplicationEffect::Hotkeys));
        effects.extend(widget.map(ApplicationEffect::Widget));
        // The menu is rendered with the locale, so a locale change is a menu
        // change. A full refresh rebuilds the menu and refreshes the parts on
        // its way out, so it subsumes a part refresh.
        let tray = match (locale.is_some() || tray_menu.is_some(), tray_part.is_some()) {
            (true, _) => Some(TrayRefresh::Full),
            (false, true) => Some(TrayRefresh::Part),
            (false, false) => None,
        };
        effects.extend(tray.map(ApplicationEffect::Tray));

        Self { effects }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeApplyKind {
    None,
    Rebuild,
    ControlChannel,
}

/// How the core is reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ControlChannelDesired {
    channel: ClashControlChannel,
    disable_http_controller: bool,
}

/// What the generated runtime config is built from.
#[derive(Debug, Clone, PartialEq)]
struct RuntimeRebuildDesired {
    enable_tun_mode: bool,
    tun_stack: TunStack,
    mixed_port: PortStrategy,
    socks_port: Option<PortStrategy>,
    http_port: Option<PortStrategy>,
    external_controller: ExternalControllerStrategy,
    enable_clash_fields: bool,
}

/// The clash fields a running core reacts to, grouped by how it must react.
#[derive(Debug, Clone, PartialEq, Patch)]
#[patch(name = "ClashRuntimeChanges")]
#[patch(attribute(derive(Debug, Clone, Default, PartialEq)))]
struct ClashRuntimeDesired {
    control_channel: ControlChannelDesired,
    rebuild: RuntimeRebuildDesired,
}

impl ClashEffectFields {
    fn runtime_desired(&self) -> ClashRuntimeDesired {
        ClashRuntimeDesired {
            control_channel: ControlChannelDesired {
                channel: self.clash_control_channel,
                disable_http_controller: self.clash_ipc_disable_http_controller,
            },
            rebuild: RuntimeRebuildDesired {
                enable_tun_mode: self.enable_tun_mode,
                tun_stack: self.tun_stack,
                mixed_port: self.mixed_port.clone(),
                socks_port: self.socks_port.clone(),
                http_port: self.http_port.clone(),
                external_controller: self.external_controller.clone(),
                enable_clash_fields: self.enable_clash_fields,
            },
        }
    }
}

/// How the running core must react to a committed clash-config change.
///
/// The core binary and the execution host are deliberately absent: both have
/// dedicated facade operations whose queues this would race.
pub fn runtime_apply_kind(
    before: &ApplicationEffectInputs,
    after: &ApplicationEffectInputs,
) -> RuntimeApplyKind {
    let ClashRuntimeChanges {
        control_channel,
        rebuild,
    } = after
        .clash
        .runtime_desired()
        .into_patch_by_diff(before.clash.runtime_desired());

    // A control-channel change outranks a rebuild, matching `bridge/verge.rs`,
    // which picks `apply_control_channel` over `rebuild_running_config`
    // whenever both would apply.
    if control_channel.is_some() {
        RuntimeApplyKind::ControlChannel
    } else if rebuild.is_some() {
        RuntimeApplyKind::Rebuild
    } else {
        RuntimeApplyKind::None
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
            ClashConfig, ClashControlChannel,
            clash_strategy::port::{ExternalControllerStrategy, PortStrategy},
            tun_stack::TunStack,
        },
        runtime::executor::ResolvedPortBindings,
    };
    use nyanpasu_egui::widget::StatisticWidgetVariant;

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
                tray_selector_mode: ProxiesSelectorMode::Normal,
                tray_menu_mode: TrayMenuMode::Native,
                enable_tray_text: false,
                enable_tray_traffic: false,
                network_statistic_widget: NetworkStatisticWidgetConfig::Disabled,
            },
            clash: ClashEffectFields {
                enable_tun_mode: false,
                tun_stack: TunStack::default(),
                mixed_port: PortStrategy::new_allow_fallback(7890),
                socks_port: None,
                http_port: None,
                external_controller: ExternalControllerStrategy::default(),
                enable_clash_fields: true,
                clash_control_channel: ClashControlChannel::PreferIpc,
                clash_ipc_disable_http_controller: false,
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
    fn socks_port_removal_reports_rebuild() {
        let mut before = inputs();
        before.clash.socks_port = Some(PortStrategy::new_allow_fallback(1080));
        let after = inputs();

        assert_eq!(
            runtime_apply_kind(&before, &after),
            RuntimeApplyKind::Rebuild
        );
        assert!(
            ApplicationEffectPlan::diff(&before, &after).is_empty(),
            "a clash port strategy is not an application effect input"
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
        assert_eq!(
            plan.effects()[1],
            ApplicationEffect::Tray(TrayRefresh::Full)
        );
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
        assert_eq!(
            plan.effects()[1],
            ApplicationEffect::Tray(TrayRefresh::Part)
        );
    }

    #[test]
    fn language_and_system_proxy_change_produces_full_tray_only() {
        let before = inputs();
        let mut after = inputs();
        after.app.language = I18nLanguage::Korean;
        after.app.enable_system_proxy = true;

        let plan = ApplicationEffectPlan::diff(&before, &after);

        let tray: Vec<_> = plan
            .effects()
            .iter()
            .filter(|effect| effect.kind() == EffectKind::Tray)
            .collect();
        assert_eq!(tray, vec![&ApplicationEffect::Tray(TrayRefresh::Full)]);
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
            })
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
        let plan = ApplicationEffectPlan::full(&inputs());

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
        assert_eq!(
            plan.effects().last(),
            Some(&ApplicationEffect::Tray(TrayRefresh::Full))
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
            expected: &[ApplicationEffect],
        ) {
            let before = inputs();
            let mut after = inputs();
            mutate(&mut after);

            let plan = ApplicationEffectPlan::diff(&before, &after);

            assert_eq!(
                plan.effects(),
                expected,
                "{label} alone must plan exactly its own effect"
            );
        }

        assert_case(
            "max_log_files",
            |after| after.app.max_log_files = 14,
            &[ApplicationEffect::Logger(LoggerDesired {
                level: LoggingLevel::Info,
                max_files: 14,
            })],
        );
        assert_case(
            "tray_selector_mode",
            |after| after.app.tray_selector_mode = ProxiesSelectorMode::Submenu,
            &[ApplicationEffect::Tray(TrayRefresh::Full)],
        );
        assert_case(
            "enable_tray_text",
            |after| after.app.enable_tray_text = true,
            &[ApplicationEffect::Tray(TrayRefresh::Part)],
        );
        assert_case(
            "enable_tray_traffic",
            |after| after.app.enable_tray_traffic = true,
            &[ApplicationEffect::Tray(TrayRefresh::Part)],
        );
        assert_case(
            "enable_proxy_guard",
            |after| after.app.enable_proxy_guard = true,
            &[ApplicationEffect::ProxyGuard(ProxyGuardDesired {
                enabled: true,
                interval: Duration::from_secs(30),
            })],
        );
        assert_case(
            "enable_auto_launch",
            |after| after.app.enable_auto_launch = true,
            &[ApplicationEffect::AutoLaunch(true)],
        );
    }

    /// Same exposure on the rebuild group: none of these fields is an
    /// application effect input, so each must rebuild and plan nothing.
    #[test]
    fn every_rebuild_field_reports_rebuild_on_its_own() {
        fn assert_rebuild(label: &str, mutate: impl FnOnce(&mut ApplicationEffectInputs)) {
            let before = inputs();
            let mut after = inputs();
            mutate(&mut after);

            assert_eq!(
                runtime_apply_kind(&before, &after),
                RuntimeApplyKind::Rebuild,
                "{label} alone must rebuild the runtime config"
            );
            assert!(
                ApplicationEffectPlan::diff(&before, &after).is_empty(),
                "{label} is not an application effect input"
            );
        }

        assert_rebuild("tun_stack", |after| {
            after.clash.tun_stack = TunStack::System
        });
        assert_rebuild("http_port", |after| {
            after.clash.http_port = Some(PortStrategy::new_allow_fallback(7891));
        });
        assert_rebuild("external_controller", |after| {
            after.clash.external_controller = ExternalControllerStrategy {
                port: PortStrategy::new_allow_fallback(9999),
                ..ExternalControllerStrategy::default()
            };
        });
        assert_rebuild("enable_clash_fields", |after| {
            after.clash.enable_clash_fields = false;
        });
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
        assert_eq!(
            plan.effects()[0],
            ApplicationEffect::Tray(TrayRefresh::Full)
        );
    }

    #[test]
    fn tun_toggle_alone_produces_part_tray_and_rebuild() {
        let before = inputs();
        let mut after = inputs();
        after.clash.enable_tun_mode = true;

        let plan = ApplicationEffectPlan::diff(&before, &after);

        assert_eq!(kinds(&plan), vec![EffectKind::Tray]);
        assert_eq!(
            plan.effects()[0],
            ApplicationEffect::Tray(TrayRefresh::Part)
        );
        assert_eq!(
            runtime_apply_kind(&before, &after),
            RuntimeApplyKind::Rebuild
        );
    }

    #[test]
    fn runtime_apply_kind_prefers_control_channel_over_rebuild() {
        let before = inputs();
        let mut after = inputs();
        after.clash.clash_control_channel = ClashControlChannel::HttpOnly;
        after.clash.enable_tun_mode = true;

        assert_eq!(
            runtime_apply_kind(&before, &after),
            RuntimeApplyKind::ControlChannel
        );

        let mut after = inputs();
        after.clash.clash_ipc_disable_http_controller = true;
        assert_eq!(
            runtime_apply_kind(&before, &after),
            RuntimeApplyKind::ControlChannel
        );
    }

    #[test]
    fn runtime_apply_kind_reports_rebuild_for_port_and_tun_changes() {
        let before = inputs();

        let mut after = inputs();
        after.clash.mixed_port = PortStrategy::new_allow_fallback(7891);
        assert_eq!(
            runtime_apply_kind(&before, &after),
            RuntimeApplyKind::Rebuild,
            "mixed port strategy changes must rebuild the runtime config"
        );

        let mut after = inputs();
        after.clash.enable_tun_mode = true;
        assert_eq!(
            runtime_apply_kind(&before, &after),
            RuntimeApplyKind::Rebuild
        );

        let mut after = inputs();
        after.clash.socks_port = Some(PortStrategy::new_allow_fallback(1080));
        assert_eq!(
            runtime_apply_kind(&before, &after),
            RuntimeApplyKind::Rebuild
        );

        assert_eq!(
            runtime_apply_kind(&before, &inputs()),
            RuntimeApplyKind::None
        );
    }

    #[test]
    fn runtime_apply_kind_ignores_core_and_service_mode() {
        let app = NyanpasuAppConfig::default();
        let switched = NyanpasuAppConfig {
            core: match app.core {
                ClashCore::Mihomo => ClashCore::ClashRs,
                _ => ClashCore::Mihomo,
            },
            enable_service_mode: !app.enable_service_mode,
            ..NyanpasuAppConfig::default()
        };
        let clash = ClashConfig::default();

        let before = ApplicationEffectInputs::project(&app, &clash, None);
        let after = ApplicationEffectInputs::project(&switched, &clash, None);

        assert_eq!(before, after, "neither field may enter the effect inputs");
        assert_eq!(runtime_apply_kind(&before, &after), RuntimeApplyKind::None);
        assert!(ApplicationEffectPlan::diff(&before, &after).is_empty());
    }
}
