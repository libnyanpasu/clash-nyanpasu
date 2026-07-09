pub mod format;
pub mod state;

/// Smoke test proving the gxhash dependency compiles and runs under the
/// target-feature flags configured in `.cargo/config.toml`. gxhash fails to
/// compile without `+aes` (plus `+sse2` / `+neon`), so a passing build here
/// confirms the per-target rustflags are actually being applied.
#[cfg(test)]
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
        let a = gxhash::gxhash64(b"clash-nyanpasu", 0);
        let b = gxhash::gxhash64(b"clash-nyanpasu", 0);
        assert_eq!(a, b, "same input + seed must hash identically");
        assert_ne!(
            gxhash::gxhash64(b"a", 0),
            gxhash::gxhash64(b"b", 0),
            "different inputs should (almost surely) differ"
        );
    }
}
