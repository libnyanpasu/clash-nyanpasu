#[nyanpasu_macro::rpc(http, permission = "settings.storage_item:set")]
fn external_shared() -> bool {
    true
}

mod nested;
