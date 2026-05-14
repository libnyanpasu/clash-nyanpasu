use clash_nyanpasu_lib::{
    config::nyanpasu::ClashCore,
    core::clash::test_harness::{ClashTestHarness, HarnessConfig},
};

const ALL_CORES: &[ClashCore] = &[
    ClashCore::Mihomo,
    ClashCore::MihomoAlpha,
    ClashCore::ClashRs,
    ClashCore::ClashRsAlpha,
    ClashCore::ClashPremium,
];

fn core_name(core_type: ClashCore) -> &'static str {
    match core_type {
        ClashCore::Mihomo => "mihomo",
        ClashCore::MihomoAlpha => "mihomo-alpha",
        ClashCore::ClashRs => "clash-rs",
        ClashCore::ClashRsAlpha => "clash-rs-alpha",
        ClashCore::ClashPremium => "clash-premium",
    }
}

/// Runs get_version, get_configs, get_proxies against every pre-downloaded sidecar core.
/// Cores whose sidecar binary is missing are skipped with a diagnostic message.
#[tokio::test]
#[ignore]
async fn test_all_cores() {
    for &core_type in ALL_CORES {
        let name = core_name(core_type);
        eprintln!("testing core: {name}");

        let config = HarnessConfig {
            core_type,
            ..Default::default()
        };

        let harness = match ClashTestHarness::new(config).await {
            Ok(h) => h,
            Err(e) => {
                eprintln!("  failed to start harness for {name}: {e}");
                continue;
            }
        };

        let client = harness.client();

        match client.get_version().await {
            Ok(v) => eprintln!("  version: {}", v.version),
            Err(e) => eprintln!("  get_version failed: {e}"),
        }

        match client.get_configs().await {
            Ok(c) => eprintln!("  mode: {:?}", c.mode),
            Err(e) => eprintln!("  get_configs failed: {e}"),
        }

        match client.get_proxies().await {
            Ok(p) => eprintln!("  proxies count: {}", p.proxies.len()),
            Err(e) => eprintln!("  get_proxies failed: {e}"),
        }

        eprintln!("  done for {name}");
    }
}
