#![cfg_attr(unix, cfg(target_os = "linux"))]

#[nyanpasu_rpc::rpc(optional_capabilities(elevation), expose = [local])]
fn nested_shared() -> bool {
    true
}
