use std::time::Duration;

use anyhow::Result;
use clash_api::{DelayQuery, ProviderName, ProxyName};
use indexmap::IndexMap;

use crate::core::clash::api::{
    ClashRule, ClashVersion, DelayRes, ProvidersRulesRes, RuleProviderItem, RulesRes,
};

use super::NyanpasuClient;

fn delay_query(url: Option<String>) -> Result<DelayQuery> {
    let url = url
        .filter(|url| !url.is_empty())
        .unwrap_or_else(|| "http://www.gstatic.com/generate_204".into());
    Ok(DelayQuery::new(url.parse()?, Duration::from_secs(10))?)
}

impl NyanpasuClient {
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
    ) -> Result<DelayRes> {
        let query = delay_query(url)?;
        let provider = provider.map(ProviderName::new);
        let delay = self
            .inner
            .core_api
            .api_client()
            .await?
            .proxy_delay(&ProxyName::new(name), provider.as_ref(), &query)
            .await?;
        Ok(DelayRes {
            delay: u64::from(delay.delay),
        })
    }

    pub async fn group_delay(
        &self,
        group: String,
        url: Option<String>,
    ) -> Result<IndexMap<String, u32>> {
        let query = delay_query(url)?;
        let delays = self
            .inner
            .core_api
            .api_client()
            .await?
            .group_delay(&ProxyName::new(group), &query)
            .await?;
        Ok(delays
            .into_iter()
            .map(|(name, delay)| (name.as_str().to_owned(), u32::from(delay)))
            .collect())
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

#[cfg(test)]
mod tests {
    use super::rule_provider_item;

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
