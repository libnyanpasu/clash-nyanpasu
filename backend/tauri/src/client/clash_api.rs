use std::time::Duration;

use anyhow::Result;
use clash_api::{DelayQuery, ProviderName, ProxyName};
use indexmap::IndexMap;

use crate::core::clash::api::{ClashRule, ClashVersion, DelayRes, RulesRes};

use super::NyanpasuClient;

fn delay_query(url: Option<String>) -> Result<DelayQuery> {
    let url = url
        .filter(|url| !url.is_empty())
        .unwrap_or_else(|| "http://www.gstatic.com/generate_204".into());
    Ok(DelayQuery::new(url.parse()?, Duration::from_secs(10))?)
}

impl NyanpasuClient {
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
