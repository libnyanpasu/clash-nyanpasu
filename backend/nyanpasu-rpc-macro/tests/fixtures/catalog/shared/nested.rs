#![cfg_attr(unix, cfg(target_os = "linux"))]

#[nyanpasu_rpc::rpc(local, optional_capabilities(elevation), scope = settings)]
fn nested_shared() -> bool {
    true
}
