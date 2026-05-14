use clash_nyanpasu_lib::{
    config::nyanpasu::ClashCore,
    core::clash::test_harness::{ClashTestHarness, HarnessConfig},
};

/// Set up a harness for the given core type using the pre-downloaded sidecar binary.
/// Returns None if the sidecar binary cannot be found or the harness fails to start.
pub async fn setup_harness(core_type: ClashCore) -> Option<ClashTestHarness> {
    let config = HarnessConfig {
        core_type,
        ..Default::default()
    };
    match ClashTestHarness::new(config).await {
        Ok(h) => Some(h),
        Err(e) => {
            eprintln!("harness setup failed: {e}");
            None
        }
    }
}
