use crate::{
    config::{
        profile::item_type::{ProfileItemType, ProfileUid},
        Config,
    },
    utils::{config::NyanpasuReqwestProxyExt, help},
};

use super::{ProfileFileOps, ProfileShared, ProfileSharedBuilder};
use crate::utils::dirs::APP_VERSION;
use anyhow::Context;
use backon::Retryable;
use derive_builder::Builder;
use indexmap::IndexMap;
use nyanpasu_macro::BuilderUpdate;
use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;
use sysproxy::Sysproxy;
use url::Url;

#[async_trait::async_trait]
pub trait RemoteProfileSubscription {
    async fn subscribe(&mut self) -> anyhow::Result<()>;
}

#[derive(Default, Debug, Clone, Deserialize, Serialize, Builder, BuilderUpdate)]
#[builder(derive(Serialize, Deserialize))]
#[builder(build_fn(skip))]
#[builder_update(patch_fn = "apply")]
pub struct RemoteProfile {
    #[serde(flatten)]
    #[builder(field(
        ty = "ProfileSharedBuilder",
        build = "self.shared.build().map_err(Into::into)?"
    ))]
    #[builder_field_attr(serde(flatten))]
    #[builder_update(nested)]
    pub shared: ProfileShared,
    /// subscription urls, the first one is the main url, others proxies should be merged
    pub url: Vec<Url>,
    /// subscription user info
    #[builder(default)]
    pub extra: IndexMap<Url, SubscriptionInfo>,
    /// remote profile options
    #[builder(field(
        ty = "RemoteProfileOptionsBuilder",
        build = "self.option.build().map_err(Into::into)?"
    ))]
    #[builder_update(nested)]
    pub option: RemoteProfileOptions,
    /// process chains
    #[builder(default)]
    pub chains: Vec<ProfileUid>,
}

struct Subscription {
    pub filename: Option<String>,
    pub data: Mapping,
    pub info: SubscriptionInfo,
}

/// perform a subscription
async fn subscribe_url(url: &Url, options: &RemoteProfileOptions) -> anyhow::Result<Subscription> {
    let options = options.apply_default();
    let mut builder = reqwest::ClientBuilder::new().use_rustls_tls().no_proxy();

    // TODO: 添加一个代理测试环节？
    let proxy_url: Option<String> = if options.self_proxy.unwrap() {
        // 使用软件自己的代理
        let port = Config::verge()
            .latest()
            .verge_mixed_port
            .unwrap_or(Config::clash().data().get_mixed_port());

        Some(format!("http://127.0.0.1:{port}"))
    } else if options.with_proxy.unwrap() {
        // 使用系统代理
        if let Ok(p @ Sysproxy { enable: true, .. }) = Sysproxy::get_system_proxy() {
            Some(format!("http://{}:{}", p.host, p.port))
        } else {
            None
        }
    } else {
        None
    };
    if let Some(proxy_url) = proxy_url {
        builder = builder.swift_set_proxy(&proxy_url);
    }

    builder = builder.user_agent(options.user_agent.unwrap());

    let client = builder.build()?;
    let perform_req = || async { Ok(client.get(url.as_str()).send().await?.error_for_status()?) };
    let resp = perform_req
        .retry(backon::ExponentialBuilder::default())
        .await?;

    let header = resp.headers();
    tracing::debug!("headers: {:#?}", header);

    // parse the Subscription UserInfo
    let extra = match header
        .get("subscription-userinfo")
        .or(header.get("Subscription-Userinfo"))
    {
        Some(value) => {
            tracing::debug!("Subscription-Userinfo: {:?}", value);
            let sub_info = value.to_str().unwrap_or("");

            Some(SubscriptionInfo {
                upload: help::parse_str(sub_info, "upload").unwrap_or(0),
                download: help::parse_str(sub_info, "download").unwrap_or(0),
                total: help::parse_str(sub_info, "total").unwrap_or(0),
                expire: help::parse_str(sub_info, "expire").unwrap_or(0),
            })
        }
        None => None,
    };

    // parse the Content-Disposition
    let filename = match header
        .get("content-disposition")
        .or(header.get("Content-Disposition"))
    {
        Some(value) => {
            tracing::debug!("Content-Disposition: {:?}", value);

            let filename = format!("{value:?}");
            let filename = filename.trim_matches('"');
            match help::parse_str::<String>(filename, "filename*") {
                Some(filename) => {
                    let iter = percent_encoding::percent_decode(filename.as_bytes());
                    let filename = iter.decode_utf8().unwrap_or_default();
                    filename
                        .split("''")
                        .last()
                        .map(|s| s.trim_matches('"').to_string())
                }
                None => match help::parse_str::<String>(filename, "filename") {
                    Some(filename) => {
                        let filename = filename.trim_matches('"');
                        Some(filename.to_string())
                    }
                    None => None,
                },
            }
        }
        None => None,
    };

    // parse the profile-update-interval
    let option = match header
        .get("profile-update-interval")
        .or(header.get("Profile-Update-Interval"))
    {
        Some(value) => {
            tracing::debug!("profile-update-interval: {:?}", value);
            match value.to_str().unwrap_or("").parse::<u64>() {
                Ok(val) => Some(RemoteProfileOptions {
                    update_interval: val * 60, // hour -> min
                    ..RemoteProfileOptions::default()
                }),
                Err(_) => None,
            }
        }
        None => None,
    };

    let data = resp.text_with_charset("utf-8").await?;

    // process the charset "UTF-8 with BOM"
    let data = data.trim_start_matches('\u{feff}');

    // check the data whether the valid yaml format
    let yaml =
        serde_yaml::from_str::<Mapping>(data).context("the remote profile data is invalid yaml")?;

    if !yaml.contains_key("proxies") && !yaml.contains_key("proxy-providers") {
        anyhow::bail!("profile does not contain `proxies` or `proxy-providers`");
    }

    Ok(Subscription {
        filename,
        data: yaml,
        info: extra.unwrap_or_default(),
    })
}

impl RemoteProfileBuilder {
    fn default_shared(&self) -> ProfileSharedBuilder {
        let mut builder = ProfileShared::builder();
        builder.r#type(ProfileItemType::Remote);
        builder
    }

    async fn import_urls(&mut self, urls: &[Url]) -> anyhow::Result<()> {
        if urls.is_empty() {
            anyhow::bail!("url should not be empty");
        }
        
        if self.shared.is_file_none() {
            anyhow::bail!("file should not be none");
        }

        let options = self.option.build()?;

        let futures = urls.iter().map(|url| subscribe_url(url, &options));
        let results = futures::future::join_all(futures).await;
        // filter all failed results, and combine them into one error
        let failed_jobs = results
            .iter()
            .filter_map(|r| r.as_ref().err().clone())
            .collect::<Vec<_>>();
        if !failed_jobs.is_empty() {
            let errors = failed_jobs
                .iter()
                .enumerate()
                // The results is a one to one correspondence with urls, so it is safe to get unchecked here
                .map(|(i, e)| format!("url: {}, error: {:?}", unsafe { urls.get_unchecked(i) }, e))
                .collect::<Vec<_>>()
                .join("\n");
            anyhow::bail!("failed to import urls:\n{}", errors);
        }

        self.url = Some(urls.to_vec());

        if self.extra.is_none() {
            self.extra = Some(IndexMap::new());
        }
        let extra = self.extra.as_mut().unwrap();

        let mut data = unsafe { results.get_unchecked(0).as_ref().unwrap().data.clone() };

        for (i, sub) in results.into_iter().filter_map(|r| r.ok()).enumerate() {
            let url = unsafe { urls.get_unchecked(i) };
            extra.insert(url.clone(), sub.info);
            if i > 0
                && let Some(proxies) = sub.data.get("proxies")
                && proxies.is_sequence()
                && !proxies.as_sequence().unwrap().is_empty()
            {
                let mut proxies = proxies.as_sequence().unwrap().clone();
                let main_proxies = data.get_mut("proxies").unwrap().as_sequence_mut().unwrap();
                main_proxies.append(&mut proxies);
            }
        }

        let shared = self.shared.build()?;
        shared.set_file(serde_yaml::to_string(&data)?).await?;
        Ok(())
    }

    pub async fn build_non_blocking(&mut self) -> Result<RemoteProfile, RemoteProfileBuilderError> {
        if self.url.is_none() || self.url.is_some_and(|v| v.is_empty()) {
            return Err(RemoteProfileBuilderError::ValidationError(
                "url should not be null".into(),
            ));
        }

        self.shared = self.default_shared();
        self.import_urls(self.url.as_ref().unwrap()).await?;
        Ok(self.build()?)
    }

    pub fn build(&mut self) -> Result<RemoteProfile, RemoteProfileBuilderError> {
        if self.url.is_none() || self.url.is_some_and(|v| v.is_empty()) {
            return Err(RemoteProfileBuilderError::ValidationError(
                "url should not be null".into(),
            ));
        }

        Ok(RemoteProfile {
            shared: self
                .shared
                .build()
                .map_err(|e| RemoteProfileBuilderError::from(e.to_string()))?,
            url: self.url.take().unwrap(),
            extra: self.extra.take().unwrap_or_default(),
            option: self.option.build().map_err(Into::into)?,
            chains: self.chains.take().unwrap_or_default(),
        })
    }
}

#[derive(Default, Debug, Clone, Copy, Deserialize, Serialize)]
pub struct SubscriptionInfo {
    pub upload: usize,
    pub download: usize,
    pub total: usize,
    pub expire: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Builder, BuilderUpdate)]
#[builder(derive(Serialize, Deserialize))]
#[builder_update(patch_fn = "apply")]
pub struct RemoteProfileOptions {
    /// see issue #13
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default, setter(strip_option))]
    pub user_agent: Option<String>,

    /// for `remote` profile
    /// use system proxy
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default, setter(strip_option))]
    pub with_proxy: Option<bool>,

    /// use self proxy
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default = "Some(true)", setter(strip_option))]
    pub self_proxy: Option<bool>,

    /// subscription update interval
    #[builder(default = "120")]
    pub update_interval: u64,
}

impl Default for RemoteProfileOptions {
    fn default() -> Self {
        Self {
            user_agent: None,
            with_proxy: None,
            self_proxy: Some(true),
            update_interval: 120, // 2 hours
        }
    }
}

impl RemoteProfileOptions {
    pub fn apply_default(&self) -> Self {
        let mut options = self.clone();
        if options.user_agent.is_none() {
            options.user_agent = Some(format!("clash-nyanpasu/v{APP_VERSION}"));
        }
        if options.with_proxy.is_none() {
            options.with_proxy = Some(false);
        }
        if options.self_proxy.is_none() {
            options.self_proxy = Some(false);
        }
        options
    }
}
