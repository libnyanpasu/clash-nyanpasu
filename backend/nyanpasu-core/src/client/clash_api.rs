use std::time::Duration;

use anyhow::Result;
use clash_api::{
    Delay, DelayQuery, ExpectedStatus, IndexMap, ProviderName, ProxyName, ProxyProvider,
};
use nyanpasu_config::application::NyanpasuAppConfig;

use crate::clash::api::{
    ClashConfig, ClashRule, ClashVersion, ProvidersRulesRes, RuleProviderItem, RulesRes,
};

use super::NyanpasuClient;

/// Used only when the app config's default test URL is empty too.
const FALLBACK_LATENCY_TEST_URL: &str = "http://www.gstatic.com/generate_204";

/// A test's URL is the caller's, else the app's default; its timeout is
/// always the app's.
fn delay_query(
    url: Option<String>,
    expected: Option<String>,
    app: &NyanpasuAppConfig,
) -> Result<DelayQuery> {
    let url = url
        .filter(|url| !url.is_empty())
        .or_else(|| Some(app.default_latency_test.clone()).filter(|url| !url.is_empty()))
        .unwrap_or_else(|| FALLBACK_LATENCY_TEST_URL.into());
    let query = DelayQuery::new(
        url.parse()?,
        Duration::from_millis(app.default_latency_timeout_ms),
    )?;
    Ok(match expected.filter(|expected| !expected.is_empty()) {
        Some(expected) => query.with_expected(ExpectedStatus::new(expected)?),
        None => query,
    })
}

impl NyanpasuClient {
    pub async fn get_proxies(&self) -> Result<crate::clash::proxies::Proxies> {
        let proxies = self.inner.proxies.get(false).await?;
        Ok(proxies.trim_for_frontend(&self.default_latency_test_url().await?))
    }
    pub async fn refresh_proxies(&self) -> Result<crate::clash::proxies::Proxies> {
        let proxies = self.inner.proxies.get(true).await?;
        Ok(proxies.trim_for_frontend(&self.default_latency_test_url().await?))
    }
    /// The URL a test without an explicit one uses, as in `delay_query`.
    async fn default_latency_test_url(&self) -> Result<String> {
        let url = self.get_app_config().await?.default_latency_test;
        Ok(if url.is_empty() {
            FALLBACK_LATENCY_TEST_URL.into()
        } else {
            url
        })
    }
    pub async fn proxy_providers(&self) -> Result<IndexMap<ProviderName, ProxyProvider>> {
        self.inner.proxies.providers().await
    }
    pub async fn select_proxy(
        &self,
        group: String,
        name: String,
    ) -> Result<super::runtime::MutationOutcome<()>> {
        let strategy = self
            .get_clash_config()
            .await?
            .break_connection
            .on_proxy_change;
        self.inner.proxies.select(group, name, strategy).await
    }
    pub async fn clear_proxy_fixed(
        &self,
        group: String,
    ) -> Result<super::runtime::MutationOutcome<()>> {
        let strategy = self
            .get_clash_config()
            .await?
            .break_connection
            .on_proxy_change;
        self.inner.proxies.clear_fixed(group, strategy).await
    }
    pub async fn update_proxy_provider(&self, name: String) -> Result<()> {
        self.inner.proxies.update_provider(name).await
    }

    pub async fn healthcheck_proxy_provider(&self, name: String) -> Result<()> {
        self.inner.proxies.healthcheck_provider(name).await
    }
    pub fn request_proxy_refresh(&self) {
        self.inner.proxies.request_refresh();
    }
    pub fn proxies_snapshot(&self) -> crate::clash::proxies::Proxies {
        self.inner.proxies.snapshot()
    }
    pub fn subscribe_proxy_changes(&self) -> tokio::sync::watch::Receiver<()> {
        self.inner.proxies.subscribe()
    }

    pub async fn clash_configs(&self) -> Result<ClashConfig> {
        config_item(self.inner.core_api.api_client().await?.configs().await?)
    }

    pub async fn clash_rule_providers(&self) -> Result<ProvidersRulesRes> {
        let providers = self
            .inner
            .core_api
            .api_client()
            .await?
            .rule_providers()
            .await?;
        Ok(ProvidersRulesRes {
            providers: providers
                .into_iter()
                .map(|(name, provider)| {
                    Ok((name.as_str().to_owned(), rule_provider_item(provider)?))
                })
                .collect::<Result<_>>()?,
        })
    }

    pub async fn clash_rules(&self) -> Result<RulesRes> {
        let rules = self.inner.core_api.api_client().await?.rules().await?;
        Ok(RulesRes {
            rules: rules
                .into_iter()
                .map(|rule| ClashRule {
                    r#type: rule.rule_type,
                    payload: rule.payload,
                    proxy: rule.proxy,
                })
                .collect(),
        })
    }

    pub async fn update_clash_rule_provider(&self, name: String) -> Result<()> {
        self.inner
            .core_api
            .api_client()
            .await?
            .update_rule_provider(&clash_api::RuleProviderName::new(name))
            .await?;
        Ok(())
    }

    pub async fn clash_version(&self) -> Result<ClashVersion> {
        let version = self.inner.core_api.api_client().await?.version().await?;
        Ok(ClashVersion {
            version: version.version,
            premium: version.premium,
            meta: Some(version.meta),
        })
    }

    pub async fn proxy_delay(
        &self,
        name: String,
        provider: Option<String>,
        url: Option<String>,
        expected: Option<String>,
    ) -> Result<Delay> {
        let query = delay_query(url, expected, &self.get_app_config().await?)?;
        let provider = provider.map(ProviderName::new);
        Ok(self
            .inner
            .core_api
            .api_client()
            .await?
            .proxy_delay(&ProxyName::new(name), provider.as_ref(), &query)
            .await?)
    }

    pub async fn group_delay(
        &self,
        group: String,
        url: Option<String>,
        expected: Option<String>,
    ) -> Result<IndexMap<ProxyName, u16>> {
        let query = delay_query(url, expected, &self.get_app_config().await?)?;
        Ok(self
            .inner
            .core_api
            .api_client()
            .await?
            .group_delay(&ProxyName::new(group), &query)
            .await?)
    }

    pub async fn close_clash_connections(&self, id: Option<String>) -> Result<()> {
        let id = id.map(|id| id.parse::<uuid::Uuid>()).transpose()?;
        let api = self.inner.core_api.api_client().await?;
        match id {
            Some(id) => api.close_connection(id).await?,
            None => api.close_all_connections().await?,
        }
        Ok(())
    }
}

fn rule_provider_item(provider: clash_api::RuleProvider) -> Result<RuleProviderItem> {
    Ok(RuleProviderItem {
        name: provider.name.as_str().to_owned(),
        behavior: provider.behavior.map(|value| value.as_str().to_owned()),
        format: provider.format.map(|value| value.as_str().to_owned()),
        rule_count: provider.rule_count.map(u32::try_from).transpose()?,
        r#type: provider
            .provider_type
            .map(|value| value.as_str().to_owned()),
        vehicle_type: provider.vehicle_type.map(|value| value.as_str().to_owned()),
        updated_at: provider.updated_at.map(|value| value.to_rfc3339()),
    })
}

fn config_item(config: clash_api::RuntimeConfig) -> Result<ClashConfig> {
    Ok(ClashConfig {
        port: config.port.map(u16::try_from).transpose()?,
        socket_port: config.socket_port.map(u16::try_from).transpose()?,
        socks_port: config.socks_port.map(u16::try_from).transpose()?,
        mixed_port: config.mixed_port.map(u16::try_from).transpose()?,
        redir_port: config.redir_port.map(u16::try_from).transpose()?,
        tproxy_port: config.tproxy_port.map(u16::try_from).transpose()?,
        mode: config.mode.map(|value| value.as_str().to_owned()),
        log_level: config.log_level.map(|value| value.as_str().to_owned()),
        allow_lan: config.allow_lan,
        ipv6: config.ipv6,
        external_controller: config.external_controller,
        secret: config.secret,
    })
}

#[cfg(test)]
mod tests {
    use super::rule_provider_item;

    #[test]
    fn a_delay_query_falls_back_to_the_app_default_then_gstatic() {
        use nyanpasu_config::application::NyanpasuAppConfig;
        let mut app = NyanpasuAppConfig {
            default_latency_test: "https://cp.cloudflare.com/generate_204".into(),
            default_latency_timeout_ms: 3000,
            ..Default::default()
        };

        let query = super::delay_query(Some("https://example.com/".into()), None, &app).unwrap();
        assert_eq!(query.url.as_str(), "https://example.com/");
        assert_eq!(query.timeout, std::time::Duration::from_millis(3000));
        assert!(query.expected.is_none());

        let query = super::delay_query(Some(String::new()), None, &app).unwrap();
        assert_eq!(query.url.as_str(), "https://cp.cloudflare.com/generate_204");

        app.default_latency_test.clear();
        let query = super::delay_query(None, None, &app).unwrap();
        assert_eq!(query.url.as_str(), "http://www.gstatic.com/generate_204");
    }

    #[test]
    fn a_delay_query_validates_the_expected_status() {
        let app = nyanpasu_config::application::NyanpasuAppConfig::default();
        let query = super::delay_query(None, Some("200-299".into()), &app).unwrap();
        assert_eq!(query.expected.unwrap().as_str(), "200-299");
        assert!(
            super::delay_query(None, Some(String::new()), &app)
                .unwrap()
                .expected
                .is_none()
        );
        assert!(super::delay_query(None, Some("not a status".into()), &app).is_err());
    }

    #[test]
    fn config_dto_preserves_absence_unknown_modes_and_valid_port_bounds() {
        let config = serde_json::from_value(serde_json::json!({
            "mode":"future-mode", "log-level":"trace", "mixed-port":0,
            "port":65535, "ipv6":false, "external-controller":"127.0.0.1:9090",
            "secret":"fixture-secret", "socket-port":100
        }))
        .unwrap();
        let dto = super::config_item(config).unwrap();
        assert_eq!(dto.mode.as_deref(), Some("future-mode"));
        assert_eq!(dto.log_level.as_deref(), Some("trace"));
        assert_eq!(dto.mixed_port, Some(0));
        assert_eq!(dto.port, Some(65535));
        assert_eq!(dto.ipv6, Some(false));
        assert_eq!(dto.socket_port, Some(100));
        assert_eq!(dto.secret.as_deref(), Some("fixture-secret"));
        assert!(dto.socks_port.is_none());
        assert!(dto.allow_lan.is_none());
        for field in [
            "port",
            "socket-port",
            "socks-port",
            "mixed-port",
            "redir-port",
            "tproxy-port",
        ] {
            for port in [-1, 65536] {
                let mut value = serde_json::json!({});
                value[field] = port.into();
                assert!(super::config_item(serde_json::from_value(value).unwrap()).is_err());
            }
        }
    }

    #[test]
    fn provider_dto_preserves_unknown_values_and_absence() {
        let provider = serde_json::from_value(serde_json::json!({
            "name":"provider", "behavior":"future", "format":"format-v2",
            "type":"remote-v2", "vehicleType":"transport-v2",
            "updatedAt":"2026-09-07T10:00:00+08:00"
        }))
        .unwrap();
        let item = serde_json::to_value(rule_provider_item(provider).unwrap()).unwrap();
        assert_eq!(
            item,
            serde_json::json!({
                "name":"provider", "behavior":"future", "format":"format-v2",
                "type":"remote-v2", "vehicleType":"transport-v2", "ruleCount":null,
                "updatedAt":"2026-09-07T10:00:00+08:00"
            })
        );
    }

    #[test]
    fn provider_dto_rejects_counts_outside_the_ui_contract() {
        for count in [-1, i64::from(u32::MAX) + 1] {
            let provider =
                serde_json::from_value(serde_json::json!({"name":"p","ruleCount":count})).unwrap();
            assert!(rule_provider_item(provider).is_err());
        }
    }
}
