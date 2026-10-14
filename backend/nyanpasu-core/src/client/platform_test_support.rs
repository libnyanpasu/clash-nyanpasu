use crate::{
    system_dns::{SystemDnsCache, SystemDnsError},
    system_proxy::ports::{
        AutoLaunchError, AutoLaunchPort, OsProxyConfig, OsProxyError, OsProxyPort, PacError,
        PacPort,
    },
};
use tokio_util::sync::CancellationToken;

mockall::mock! {
    pub(super) OsProxyPort {}

    impl OsProxyPort for OsProxyPort {
        fn get(&self) -> Result<OsProxyConfig, OsProxyError>;
        fn set(&self, config: &OsProxyConfig) -> Result<(), OsProxyError>;
        fn default_bypass(&self) -> &'static str;
    }
}

mockall::mock! {
    pub(super) AutoLaunchPort {}

    impl AutoLaunchPort for AutoLaunchPort {
        fn is_enabled(&self) -> Result<bool, AutoLaunchError>;
        fn set_enabled(&self, enabled: bool) -> Result<(), AutoLaunchError>;
    }
}

mockall::mock! {
    pub(super) PacPort {}

    #[async_trait::async_trait]
    impl PacPort for PacPort {
        fn is_supported(&self) -> bool;
        async fn apply(&self, url: &url::Url, cancel: CancellationToken) -> Result<(), PacError>;
        fn disable(&self) -> Result<(), PacError>;
    }
}

mockall::mock! {
    pub(super) SystemDnsCache {}

    impl SystemDnsCache for SystemDnsCache {
        fn flush(&self) -> Result<(), SystemDnsError>;
    }
}

#[cfg(test)]
#[derive(Debug, Default)]
pub struct NoopSystemDnsCache;

#[cfg(test)]
impl SystemDnsCache for NoopSystemDnsCache {
    fn flush(&self) -> Result<(), SystemDnsError> {
        Ok(())
    }
}
