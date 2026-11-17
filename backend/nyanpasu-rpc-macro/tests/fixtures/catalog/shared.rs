#[nyanpasu_rpc::rpc(http, permission = settings:write)]
fn external_shared() -> bool {
    true
}

mod nested;
