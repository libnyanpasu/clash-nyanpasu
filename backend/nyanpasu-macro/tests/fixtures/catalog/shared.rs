#[nyanpasu_macro::rpc(http)]
fn external_shared() -> bool {
    true
}

mod nested;
