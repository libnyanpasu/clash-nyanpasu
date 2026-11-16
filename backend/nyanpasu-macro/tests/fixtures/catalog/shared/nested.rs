#![cfg_attr(unix, cfg(target_os = "linux"))]

#[nyanpasu_macro::rpc(local, optional_capabilities(elevation), permission = "settings.storage_item:set")]
fn nested_shared() -> bool {
    true
}
