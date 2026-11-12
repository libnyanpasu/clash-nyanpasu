#[nyanpasu_macro::rpc(http, scope = settings)]
fn external_shared() -> bool {
    true
}

mod nested;
