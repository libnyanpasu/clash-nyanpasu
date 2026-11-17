#![cfg_attr(unix, cfg(target_os = "linux"))]

#[nyanpasu_rpc::rpc(local, optional_capabilities(elevation), permission = settings:write)]
fn nested_shared() -> bool {
    true
}
