#[nyanpasu_rpc::rpc(expose = [http])]
fn external_shared() -> bool {
    true
}

mod nested;
