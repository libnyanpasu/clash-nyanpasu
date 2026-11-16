//! The aggregate RPC permission table, which cargo regenerates under `OUT_DIR` whenever this crate
//! compiles, from the same declarations that `#[rpc]` expands.

nyanpasu_macro::rpc_permission_table!();

#[cfg(test)]
mod tests {
    use super::RPC_PERMISSION_TABLE;

    /// The table this build generated is the reviewed snapshot beside the extractor, on every platform
    /// section. After a permission change, review the diff and run
    /// `cargo test -p nyanpasu-macro --features syn/full export_rpc_permission_table -- --ignored`.
    #[test]
    fn generated_table_is_the_reviewed_snapshot() {
        assert_eq!(
            RPC_PERMISSION_TABLE,
            include_str!("../../../nyanpasu-macro/tests/fixtures/rpc-permission-table.txt")
                .replace("\r\n", "\n"),
        );
    }
}
