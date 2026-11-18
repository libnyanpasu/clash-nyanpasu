#[nyanpasu_rpc::rpc(http, scope = settings)]
fn external_shared() -> bool {
    true
}

mod nested;
