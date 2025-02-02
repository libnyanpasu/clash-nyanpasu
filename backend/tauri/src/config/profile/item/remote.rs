use super::{
    ambassador_impl_ProfileFileIo, ambassador_impl_ProfileSharedGetter,
    ambassador_impl_ProfileSharedSetter, ProfileCleanup, ProfileFileIo, ProfileHelper,
    ProfileShared, ProfileSharedBuilder, ProfileSharedGetter, ProfileSharedSetter,
};
use crate::{
    config::{
        profile::item_type::{ProfileItemType, ProfileUid},
        Config,
    },
    utils::{config::NyanpasuReqwestProxyExt, dirs::APP_VERSION, help},
};
use ambassador::Delegate;
use backon::Retryable;
use derive_builder::Builder;
use indexmap::IndexMap;
use itertools::Itertools;
use nyanpasu_macro::BuilderUpdate;
use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;
use specta::Type;
use std::time::Duration;
use sysproxy::Sysproxy;
use url::Url;

pub trait RemoteProfileSubscription {
    async fn subscribe(&mut self, opts: Option<RemoteProfileOptionsBuilder>) -> anyhow::Result<()>;
}

#[derive(Delegate, Debug, Clone, Deserialize, Serialize, Builder, BuilderUpdate, specta::Type)]
#[builder(derive(Serialize, Deserialize, Debug, specta::Type))]
#[builder(build_fn(skip, error = "RemoteProfileBuilderError"))]
#[builder_update(patch_fn = "apply")]
#[delegate(ProfileSharedSetter, target = "shared")]
#[delegate(ProfileSharedGetter, target = "shared")]
#[delegate(ProfileFileIo, target = "shared")]
pub struct RemoteProfile {
    #[serde(flatten)]
    #[builder(field(
        ty = "ProfileSharedBuilder",
        build = "self.shared.build().map_err(Into::into)?"
    ))]
    #[builder_field_attr(serde(flatten))]
    #[builder_update(nested)]
    pub shared: ProfileShared,
    /// subscription url
    pub url: Url,
    /// subscription user info
    #[builder(default)]
    #[serde(default)]
    pub extra: SubscriptionInfo,
    /// remote profile options
    #[builder(field(
        ty = "RemoteProfileOptionsBuilder",
        build = "self.option.build().map_err(Into::into)?"
    ))]
    #[builder_update(nested)]
    #[builder_field_attr(serde(default))]
    #[serde(default)]
    pub option: RemoteProfileOptions,
    /// process chain
    #[builder(default)]
    #[serde(alias = "chains", default)]
    #[builder_field_attr(serde(alias = "chains", default))]
    pub chain: Vec<ProfileUid>,
}

impl ProfileHelper for RemoteProfile {}
impl ProfileCleanup for RemoteProfile {}

impl RemoteProfileSubscription for RemoteProfile {
    #[tracing::instrument]
    async fn subscribe(
        &mut self,
        partial: Option<RemoteProfileOptionsBuilder>,
    ) -> anyhow::Result<()> {
        let mut opts = self.option.clone();
        if let Some(partial) = partial {
            opts.apply(partial);
        }
        let subscription = subscribe_url(&self.url, &opts).await?;
        self.extra = subscription.info;

        let content = serde_yaml::to_string(&subscription.data)?;
        self.write_file(content).await?;
        self.set_updated(chrono::Local::now().timestamp() as usize);
        Ok(())
    }
}

#[derive(Debug)]
struct Subscription {
    pub url: Url,
    pub filename: Option<String>,
    pub data: Mapping,
    pub info: SubscriptionInfo,
    pub opts: Option<RemoteProfileOptions>,
}

/// perform a subscription
#[tracing::instrument]
async fn subscribe_url(
    url: &Url,
    options: &RemoteProfileOptions,
) -> Result<Subscription, SubscribeError> {
    let options = options.apply_default();
    let mut builder = reqwest::ClientBuilder::new()
        .use_rustls_tls()
        .no_proxy()
        .timeout(Duration::from_secs(30));

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

    let client = builder.build().map_err(|e| SubscribeError::Network {
        url: url.to_string(),
        source: e,
    })?;
    let perform_req = || async { client.get(url.as_str()).send().await?.error_for_status() };
    let resp = perform_req
        .retry(backon::ExponentialBuilder::default())
        // Only retry on network errors or server errors
        .when(|result| {
            !result.is_status()
                || result.status().is_some_and(|status_code| {
                    !matches!(
                        status_code,
                        reqwest::StatusCode::FORBIDDEN
                            | reqwest::StatusCode::NOT_FOUND
                            | reqwest::StatusCode::UNAUTHORIZED
                    )
                })
        })
        .await
        .map_err(|e| SubscribeError::Network {
            url: url.to_string(),
            source: e,
        })?;

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
    let opts = match header
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

    let data = resp
        .text_with_charset("utf-8")
        .await
        .map_err(|e| SubscribeError::Network {
            url: url.to_string(),
            source: e,
        })?;

    // process the charset "UTF-8 with BOM"
    let data = data.trim_start_matches('\u{feff}');

    // check the data whether the valid yaml format
    let yaml = serde_yaml::from_str::<Mapping>(data).map_err(|e| SubscribeError::Parse {
        url: url.to_string(),
        source: e,
    })?;

    if !yaml.contains_key("proxies") && !yaml.contains_key("proxy-providers") {
        return Err(SubscribeError::ValidationFailed {
            url: url.to_string(),
            reason: "profile does not contain `proxies` or `proxy-providers`".to_string(),
        });
    }

    Ok(Subscription {
        url: url.clone(),
        filename,
        data: yaml,
        info: extra.unwrap_or_default(),
        opts,
    })
}

/// subscribe multiple urls
#[tracing::instrument]
async fn subscribe_urls(
    urls: &[Url],
    options: &RemoteProfileOptions,
) -> Result<Vec<Subscription>, SubscribeError> {
    if urls.is_empty() {
        return Err(SubscribeError::ValidationFailed {
            url: "".to_string(),
            reason: "urls should not be empty".to_string(),
        });
    }
    let futures = urls.iter().map(|url| subscribe_url(url, options));
    let results = futures::future::join_all(futures).await;
    let (successes, errors): (Vec<_>, Vec<_>) = results.into_iter().partition_map(|r| match r {
        Ok(val) => itertools::Either::Left(val),
        Err(err) => itertools::Either::Right(err),
    });

    if !errors.is_empty() {
        return Err(SubscribeError::MultipleErrors(errors));
    }

    Ok(successes)
}

/// merge the subscriptions
#[tracing::instrument]
fn merge_subscription(
    subscriptions: &[Subscription],
) -> (Mapping, IndexMap<Url, SubscriptionInfo>) {
    let mut data = Mapping::new();
    let mut extra = IndexMap::new();
    for (i, sub) in subscriptions.iter().enumerate() {
        if i == 0 {
            data.extend(sub.data.clone());
        } else {
            let proxies = data.get_mut("proxies").unwrap().as_sequence_mut().unwrap();
            let sub_proxies = sub.data.get("proxies").unwrap().as_sequence().unwrap();
            proxies.extend(sub_proxies.iter().cloned());
        }
        extra.insert(sub.url.clone(), sub.info);
    }
    (data, extra)
}

#[derive(thiserror::Error, Debug)]
pub enum SubscribeError {
    #[error("network issue at {url}: {source}")]
    Network {
        url: String,
        #[source]
        source: reqwest::Error,
    },

    #[error("yaml parse error at {url}: {source}")]
    Parse {
        url: String,
        #[source]
        source: serde_yaml::Error,
    },

    #[error("invalid profile at {url}: {reason}")]
    ValidationFailed { url: String, reason: String },

    #[error("multiple errors occurred: {0:?}")]
    MultipleErrors(Vec<SubscribeError>),
}

#[derive(thiserror::Error, Debug)]
pub enum RemoteProfileBuilderError {
    #[error("validation error: {0}")]
    Validation(String),
    #[error("error: {0}")]
    UninitializedField(#[from] derive_builder::UninitializedFieldError),
    #[error("subscribe failed: {0}")]
    SubscribeFailed(#[from] SubscribeError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl RemoteProfileBuilder {
    fn default_shared(&self) -> ProfileSharedBuilder {
        let mut builder = ProfileShared::builder();
        builder.r#type(ProfileItemType::Remote);
        builder
    }

    fn validate(&self) -> Result<(), RemoteProfileBuilderError> {
        if self.url.is_none() {
            return Err(RemoteProfileBuilderError::Validation(
                "url should not be null".into(),
            ));
        }

        Ok(())
    }

    pub async fn build_no_blocking(&mut self) -> Result<RemoteProfile, RemoteProfileBuilderError> {
        self.validate()?;
        if self.shared.get_uid().is_none() {
            self.shared
                .uid(super::utils::generate_uid(&ProfileItemType::Remote));
        }
        self.shared.r#type(ProfileItemType::Remote);
        let url = self.url.take().unwrap();
        let options = self
            .option
            .build()
            .map_err(|e| RemoteProfileBuilderError::Validation(e.to_string()))?;
        let mut subscription = subscribe_url(&url, &options).await?;
        let extra = subscription.info;

        if self.shared.get_name().is_none() && subscription.filename.is_some() {
            self.shared.name(subscription.filename.take().unwrap());
        }
        if self.option.get_update_interval().is_none() && subscription.opts.is_some() {
            self.option
                .update_interval(subscription.opts.take().unwrap().update_interval);
        }

        let profile = RemoteProfile {
            shared: self
                .shared
                .build()
                .map_err(|e| RemoteProfileBuilderError::Validation(e.to_string()))?,
            url,
            extra,
            option: self.option.build().unwrap(),
            chain: self.chain.take().unwrap_or_default(),
        };
        // write the profile to the file
        profile
            .shared
            .write_file(
                serde_yaml::to_string(&subscription.data)
                    .map_err(|e| RemoteProfileBuilderError::Validation(e.to_string()))?,
            )
            .await?;
        Ok(profile)
    }

    pub fn build(&mut self) -> Result<RemoteProfile, RemoteProfileBuilderError> {
        nyanpasu_utils::runtime::block_on_current_thread(self.build_no_blocking())
            .map_err(|e| RemoteProfileBuilderError::Validation(e.to_string()))
    }
}

#[derive(Default, Debug, Clone, Copy, Deserialize, Serialize, Type)]
pub struct SubscriptionInfo {
    pub upload: usize,
    pub download: usize,
    pub total: usize,
    pub expire: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Builder, BuilderUpdate, Type)]
#[builder(derive(Serialize, Deserialize, Debug, Type))]
#[builder_update(patch_fn = "apply", getter)]
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
