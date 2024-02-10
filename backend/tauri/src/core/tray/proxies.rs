use crate::core::clash::proxies::{ProxiesGuard, ProxiesGuardExt};

async fn loop_task() {
    loop {
        ProxiesGuard::global().update().await.unwrap();
        {
            let guard = ProxiesGuard::global().read();
            let proxies = guard.inner();
            let str = simd_json::to_string_pretty(proxies).unwrap();
            println!("{:?}", str);
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    }
}
pub fn test() {
    tauri::async_runtime::spawn(loop_task());
}
