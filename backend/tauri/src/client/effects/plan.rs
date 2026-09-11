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

/// The only configuration an effect may depend on.
///
/// `NyanpasuAppConfig` and `ClashConfig` are not `PartialEq` and carry
/// platform-gated fields, so effects diff this projection rather than the
/// config structs. A field absent here provably cannot trigger an effect.
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

    fn mixed_port(&self) -> Option<u16> {
        self.ports.as_ref().map(|ports| ports.mixed_port)
    }

    fn logger_desired(&self) -> LoggerDesired {
        LoggerDesired {
            level: self.app.app_log_level.clone(),
            max_files: self.app.max_log_files,
        }
    }

    fn system_proxy_desired(&self) -> SystemProxyDesired {
        SystemProxyDesired {
            enabled: self.app.enable_system_proxy,
            bypass: self.app.system_proxy_bypass.clone(),
            port: self.mixed_port(),
            pac_url: self.app.pac_url.clone(),
        }
    }

    fn proxy_guard_desired(&self) -> ProxyGuardDesired {
        ProxyGuardDesired {
            enabled: self.app.enable_proxy_guard,
            interval: Duration::from_secs(self.app.proxy_guard_interval),
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
        let app_before = &before.app;
        let app_after = &after.app;
        let mut effects = Vec::new();

        // Pushed in `EffectKind` order, which is the plan's ordering invariant.
        if app_before.language != app_after.language {
            effects.push(ApplicationEffect::Locale(app_after.language));
        }
        if app_before.app_log_level != app_after.app_log_level
            || app_before.max_log_files != app_after.max_log_files
        {
            effects.push(ApplicationEffect::Logger(after.logger_desired()));
        }
        if app_before.enable_auto_launch != app_after.enable_auto_launch {
            effects.push(ApplicationEffect::AutoLaunch(app_after.enable_auto_launch));
        }
        if app_before.enable_system_proxy != app_after.enable_system_proxy
            || app_before.system_proxy_bypass != app_after.system_proxy_bypass
            || app_before.pac_url != app_after.pac_url
            || before.mixed_port() != after.mixed_port()
        {
            effects.push(ApplicationEffect::SystemProxy(after.system_proxy_desired()));
        }
        // Split from `SystemProxy` on purpose: changing only the interval must
        // not re-apply the OS proxy, and changing only the bypass must not
        // restart the guard timer.
        if app_before.enable_proxy_guard != app_after.enable_proxy_guard
            || app_before.proxy_guard_interval != app_after.proxy_guard_interval
        {
            effects.push(ApplicationEffect::ProxyGuard(after.proxy_guard_desired()));
        }
        if app_before.hotkeys != app_after.hotkeys {
            effects.push(ApplicationEffect::Hotkeys(app_after.hotkeys.clone()));
        }
        if app_before.network_statistic_widget != app_after.network_statistic_widget {
            effects.push(ApplicationEffect::Widget(
                app_after.network_statistic_widget,
            ));
        }
        if let Some(tray) = tray_refresh(before, after) {
            effects.push(ApplicationEffect::Tray(tray));
        }

        Self { effects }
    }

    /// Full reconcile: the desired value of every effect, used at startup where
    /// no trustworthy "before" snapshot exists.
    pub fn full(after: &ApplicationEffectInputs) -> Self {
        Self {
            effects: vec![
                ApplicationEffect::Locale(after.app.language),
                ApplicationEffect::Logger(after.logger_desired()),
                ApplicationEffect::AutoLaunch(after.app.enable_auto_launch),
                ApplicationEffect::SystemProxy(after.system_proxy_desired()),
                ApplicationEffect::ProxyGuard(after.proxy_guard_desired()),
                ApplicationEffect::Hotkeys(after.app.hotkeys.clone()),
                ApplicationEffect::Widget(after.app.network_statistic_widget),
                ApplicationEffect::Tray(TrayRefresh::Full),
            ],
        }
    }

    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }

    pub fn effects(&self) -> &[ApplicationEffect] {
        &self.effects
    }
}

/// A full refresh rebuilds the menu and refreshes the parts on its way out, so
/// it subsumes a part refresh when both sets of fields changed at once.
fn tray_refresh(
    before: &ApplicationEffectInputs,
    after: &ApplicationEffectInputs,
) -> Option<TrayRefresh> {
    if before.app.language != after.app.language
        || before.app.tray_menu_mode != after.app.tray_menu_mode
        || before.app.tray_selector_mode != after.app.tray_selector_mode
    {
        return Some(TrayRefresh::Full);
    }

    let part = before.app.enable_system_proxy != after.app.enable_system_proxy
        || before.clash.enable_tun_mode != after.clash.enable_tun_mode
        || before.app.enable_tray_text != after.app.enable_tray_text
        || before.app.enable_tray_traffic != after.app.enable_tray_traffic;
    part.then_some(TrayRefresh::Part)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeApplyKind {
    None,
    Rebuild,
    ControlChannel,
}

/// How the running core must react to a committed clash-config change.
///
/// The core binary and the execution host are deliberately absent: both have
/// dedicated facade operations whose queues this would race.
pub fn runtime_apply_kind(
    before: &ApplicationEffectInputs,
    after: &ApplicationEffectInputs,
) -> RuntimeApplyKind {
    let before = &before.clash;
    let after = &after.clash;

    if before.clash_control_channel != after.clash_control_channel
        || before.clash_ipc_disable_http_controller != after.clash_ipc_disable_http_controller
    {
        return RuntimeApplyKind::ControlChannel;
    }

    let rebuild = before.enable_tun_mode != after.enable_tun_mode
        || before.tun_stack != after.tun_stack
        || before.mixed_port != after.mixed_port
        || before.socks_port != after.socks_port
        || before.http_port != after.http_port
        || before.external_controller != after.external_controller
        || before.enable_clash_fields != after.enable_clash_fields;
    if rebuild {
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
