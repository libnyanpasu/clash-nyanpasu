//! Supplies the application mixed port to subscription fetchers.

/// Where the fetcher looks up the app's own mixed port when `self_proxy` is
/// requested. Wired at the composition root (T07); tests inject a constant.
#[cfg_attr(test, mockall::automock)]
pub trait SelfProxyPortSource: Send + Sync + 'static {
    fn mixed_port(&self) -> Option<u16>;
}
