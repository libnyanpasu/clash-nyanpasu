//! Explicit application-boundary conversions for clash.

use nyanpasu_core::clash::{
    api::{
        ClashConfig as DomainClashConfig, ClashRule as DomainClashRule,
        ClashVersion as DomainClashVersion, ProvidersRulesRes as DomainProvidersRulesRes,
        RuleProviderItem as DomainRuleProviderItem, RulesRes as DomainRulesRes,
    },
    proxies::{
        Proxies as DomainProxies, ProxyGroup as DomainProxyGroup,
        ProxyGroupCapabilities as DomainProxyGroupCapabilities,
        ProxyGroupKind as DomainProxyGroupKind, ProxyProviderSummary as DomainProxyProviderSummary,
    },
};
use nyanpasu_rpc_protocol::wire;

use super::IntoWire;

impl IntoWire for DomainClashConfig {
    type Wire = wire::clash::ClashConfig;

    fn into_wire(self) -> Self::Wire {
        wire::clash::ClashConfig {
            port: self.port,
            mode: self.mode,
            ipv6: self.ipv6,
            socket_port: self.socket_port,
            allow_lan: self.allow_lan,
            log_level: self.log_level,
            mixed_port: self.mixed_port,
            redir_port: self.redir_port,
            socks_port: self.socks_port,
            tproxy_port: self.tproxy_port,
            external_controller: self.external_controller,
            secret: self.secret,
        }
    }
}

impl IntoWire for DomainClashVersion {
    type Wire = wire::clash::ClashVersion;

    fn into_wire(self) -> Self::Wire {
        wire::clash::ClashVersion {
            version: self.version,
            premium: self.premium,
            meta: self.meta,
        }
    }
}

impl IntoWire for DomainClashRule {
    type Wire = wire::clash::ClashRule;

    fn into_wire(self) -> Self::Wire {
        wire::clash::ClashRule {
            r#type: self.r#type,
            payload: self.payload,
            proxy: self.proxy,
        }
    }
}

impl IntoWire for DomainRulesRes {
    type Wire = wire::clash::RulesRes;

    fn into_wire(self) -> Self::Wire {
        wire::clash::RulesRes {
            rules: self
                .rules
                .into_iter()
                .map(|value| value.into_wire())
                .collect(),
        }
    }
}

impl IntoWire for DomainRuleProviderItem {
    type Wire = wire::clash::RuleProviderItem;

    fn into_wire(self) -> Self::Wire {
        wire::clash::RuleProviderItem {
            behavior: self.behavior,
            format: self.format,
            name: self.name,
            rule_count: self.rule_count,
            r#type: self.r#type,
            updated_at: self.updated_at,
            vehicle_type: self.vehicle_type,
        }
    }
}

impl IntoWire for DomainProvidersRulesRes {
    type Wire = wire::clash::ProvidersRulesRes;

    fn into_wire(self) -> Self::Wire {
        wire::clash::ProvidersRulesRes {
            providers: self
                .providers
                .into_iter()
                .map(|(key, value)| (key, value.into_wire()))
                .collect(),
        }
    }
}

impl IntoWire for DomainProxyProviderSummary {
    type Wire = wire::clash::ProxyProviderSummary;

    fn into_wire(self) -> Self::Wire {
        wire::clash::ProxyProviderSummary {
            name: self.name,
            provider_type: self.provider_type,
            vehicle_type: self.vehicle_type,
            updated_at: self.updated_at,
            subscription_info: self.subscription_info,
            proxy_count: self.proxy_count,
        }
    }
}

impl IntoWire for DomainProxyGroupKind {
    type Wire = wire::clash::ProxyGroupKind;

    fn into_wire(self) -> Self::Wire {
        match self {
            DomainProxyGroupKind::Selector => wire::clash::ProxyGroupKind::Selector,
            DomainProxyGroupKind::UrlTest => wire::clash::ProxyGroupKind::UrlTest,
            DomainProxyGroupKind::Fallback => wire::clash::ProxyGroupKind::Fallback,
            DomainProxyGroupKind::LoadBalance => wire::clash::ProxyGroupKind::LoadBalance,
            DomainProxyGroupKind::Relay => wire::clash::ProxyGroupKind::Relay,
            DomainProxyGroupKind::Smart => wire::clash::ProxyGroupKind::Smart,
            DomainProxyGroupKind::Unknown(value0) => wire::clash::ProxyGroupKind::Unknown(value0),
        }
    }
}

impl IntoWire for DomainProxyGroupCapabilities {
    type Wire = wire::clash::ProxyGroupCapabilities;

    fn into_wire(self) -> Self::Wire {
        wire::clash::ProxyGroupCapabilities {
            select: self.select,
            clear_fixed: self.clear_fixed,
        }
    }
}

impl IntoWire for DomainProxyGroup {
    type Wire = wire::clash::ProxyGroup;

    fn into_wire(self) -> Self::Wire {
        wire::clash::ProxyGroup {
            name: self.name,
            kind: self.kind.into_wire(),
            all: self.all,
            now: self.now,
            fixed: self.fixed,
            test_url: self.test_url,
            expected_status: self.expected_status,
            hidden: self.hidden,
            icon: self.icon,
            capabilities: self.capabilities.into_wire(),
        }
    }
}

impl IntoWire for DomainProxies {
    type Wire = wire::clash::Proxies;

    fn into_wire(self) -> Self::Wire {
        wire::clash::Proxies {
            global: self.global.map(|value| value.into_wire()),
            groups: self
                .groups
                .into_iter()
                .map(|value| value.into_wire())
                .collect(),
            nodes: self.nodes,
        }
    }
}
