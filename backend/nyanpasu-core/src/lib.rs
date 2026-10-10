pub mod backup;
pub mod clash;
pub mod client;
pub use client::{ClientSetupArgs, ClientSetupOutput, NyanpasuClient};
pub(crate) mod enhance;
pub use enhance::PostProcessingOutput;
pub mod connections;
pub mod control;
pub mod device;
pub mod diagnostics;
pub mod download;
pub mod effects;
pub mod error;
pub mod format;
pub mod geo;
pub mod hotkey;
pub mod icons;
pub mod logs;
pub mod migration;
pub mod network;
pub mod profiles;
pub mod runtime;
pub mod service;
pub mod state;
pub mod storage;
pub mod system_dns;
pub mod system_proxy;
pub mod tasks;
pub mod traffic;
pub mod transport;
pub mod updates;
#[cfg(windows)]
pub mod uwp;

/// Smoke test proving the gxhash dev-dependency compiles and runs under the
/// target-feature flags configured in `.cargo/config.toml`. gxhash fails to
/// compile without `+aes` (plus `+sse2` / `+neon`), so a passing test build
/// here confirms the per-target rustflags are actually being applied.
#[cfg(all(test, not(target_env = "musl")))]
mod gxhash_smoke {
    use gxhash::{HashMap as GxHashMap, HashMapExt};

    #[test]
    fn gxhash_map_roundtrip() {
        let mut map: GxHashMap<&str, u32> = GxHashMap::new();
        map.insert("clash", 1);
        map.insert("nyanpasu", 2);
        assert_eq!(map.get("clash"), Some(&1));
        assert_eq!(map.get("nyanpasu"), Some(&2));
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn gxhash64_is_deterministic() {
        assert_eq!(
            gxhash::gxhash64(b"clash-nyanpasu", 0),
            gxhash::gxhash64(b"clash-nyanpasu", 0),
            "same input + seed must hash identically",
        );
    }
}
