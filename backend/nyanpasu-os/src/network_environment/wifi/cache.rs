//! A short-lived memory of the last Wi-Fi name read.
//!
//! Reading the name on macOS and Windows counts as using location, which the
//! system shows to the user. The observer polls every few seconds, so a name
//! that was just read for the same network is reused instead of asked again.

use std::time::{Duration, Instant};

/// The network a name was read for. A different interface or gateway is a
/// different network and is read afresh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CacheKey {
    interface: String,
    gateway_mac: Option<String>,
}

impl CacheKey {
    pub(super) fn new(interface: &str, gateway_mac: Option<&str>) -> Self {
        Self {
            interface: interface.to_owned(),
            gateway_mac: gateway_mac.map(str::to_owned),
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct NameCache {
    entry: Option<Entry>,
}

#[derive(Debug)]
struct Entry {
    key: CacheKey,
    ssid: String,
    read_at: Instant,
}

impl NameCache {
    /// The name read for `key` less than `ttl` before `now`. A zero `ttl`
    /// never hits.
    pub(super) fn get(&self, key: &CacheKey, now: Instant, ttl: Duration) -> Option<&str> {
        self.entry
            .as_ref()
            .filter(|entry| entry.key == *key && now.saturating_duration_since(entry.read_at) < ttl)
            .map(|entry| entry.ssid.as_str())
    }

    pub(super) fn put(&mut self, key: CacheKey, ssid: String, now: Instant) {
        self.entry = Some(Entry {
            key,
            ssid,
            read_at: now,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TTL: Duration = Duration::from_secs(30);

    fn home() -> CacheKey {
        CacheKey::new("wlan0", Some("aa:bb:cc:dd:ee:ff"))
    }

    #[test]
    fn a_name_is_reused_for_the_same_network_within_the_ttl() {
        let start = Instant::now();
        let mut cache = NameCache::default();
        cache.put(home(), "Home".into(), start);

        assert_eq!(
            cache.get(&home(), start + Duration::from_secs(29), TTL),
            Some("Home")
        );
        assert_eq!(cache.get(&home(), start + TTL, TTL), None);
    }

    #[test]
    fn another_network_is_read_afresh() {
        let start = Instant::now();
        let mut cache = NameCache::default();
        cache.put(home(), "Home".into(), start);

        assert_eq!(
            cache.get(
                &CacheKey::new("wlan0", Some("11:22:33:44:55:66")),
                start,
                TTL
            ),
            None
        );
        assert_eq!(cache.get(&CacheKey::new("wlan1", None), start, TTL), None);
    }

    #[test]
    fn a_zero_ttl_never_hits() {
        let start = Instant::now();
        let mut cache = NameCache::default();
        cache.put(home(), "Home".into(), start);

        assert_eq!(cache.get(&home(), start, Duration::ZERO), None);
    }
}
