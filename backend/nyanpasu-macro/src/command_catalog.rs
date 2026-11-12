//! Development-only extraction from the explicitly listed production declaration roots.

use std::{fs, path::Path};

use quote::{ToTokens as _, quote};
use syn::{Attribute, Item, ItemFn, Meta, Token, punctuated::Punctuated};

use crate::unified_command;

// Move these roots together with the schema and handler inventories when commands move.
const DECLARATION_ROOTS: &[&str] = &["../tauri/src/ipc.rs", "../nyanpasu-rpc/src/commands/mod.rs"];
const CATALOG: &str = "../nyanpasu-rpc-protocol/src/command_catalog.rs";

pub(super) fn production_declarations() -> Vec<ItemFn> {
    let mut commands = Vec::new();
    for root in DECLARATION_ROOTS {
        collect_file(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join(root),
            &[],
            &mut commands,
        );
    }
    assert!(
        !commands.is_empty(),
        "production declaration roots must not be empty"
    );
    commands
}

fn collect_file(path: &Path, inherited: &[Attribute], commands: &mut Vec<ItemFn>) {
    let source = fs::read_to_string(path).unwrap_or_else(|error| {
        panic!("cannot read declaration module {}: {error}", path.display())
    });
    let file = syn::parse_file(&source).expect("declaration module must parse");
    let mut attrs = inherited.to_vec();
    attrs.extend(guards(&file.attrs));
    let directory = if matches!(
        path.file_stem().and_then(|s| s.to_str()),
        Some("mod" | "lib")
    ) {
        path.parent().unwrap().to_owned()
    } else {
        path.with_extension("")
    };
    collect_items(&file.items, &directory, &attrs, commands);
}

fn guards(attrs: &[Attribute]) -> impl Iterator<Item = Attribute> + '_ {
    attrs
        .iter()
        .filter(|attr| attr.path().is_ident("cfg") || attr.path().is_ident("cfg_attr"))
        .cloned()
        .map(|mut attr| {
            attr.style = syn::AttrStyle::Outer;
            attr
        })
}

// Evaluate only the test switch. Platform/feature predicates remain symbolic in the artifact.
fn without_tests(meta: &Meta) -> Option<bool> {
    match meta {
        Meta::Path(path) if path.is_ident("test") => Some(false),
        Meta::List(list) => {
            let values = list
                .parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
                .expect("cfg predicates must parse");
            let values: Vec<_> = values.iter().map(without_tests).collect();
            if list.path.is_ident("not") && values.len() == 1 {
                values[0].map(|value| !value)
            } else if list.path.is_ident("all") {
                if values.contains(&Some(false)) {
                    Some(false)
                } else if values.iter().all(|value| *value == Some(true)) {
                    Some(true)
                } else {
                    None
                }
            } else if list.path.is_ident("any") {
                if values.contains(&Some(true)) {
                    Some(true)
                } else if values.iter().all(|value| *value == Some(false)) {
                    Some(false)
                } else {
                    None
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

fn excludes_production(meta: &Meta) -> bool {
    let Meta::List(list) = meta else { return false };
    if list.path.is_ident("cfg") {
        return without_tests(&list.parse_args().expect("cfg must parse")) == Some(false);
    }
    if list.path.is_ident("cfg_attr") {
        let args = list
            .parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
            .expect("cfg_attr must parse");
        let mut args = args.iter();
        return args
            .next()
            .is_some_and(|condition| without_tests(condition) == Some(true))
            && args.any(excludes_production);
    }
    false
}

fn collect_items(
    items: &[Item],
    directory: &Path,
    inherited: &[Attribute],
    commands: &mut Vec<ItemFn>,
) {
    if inherited.iter().any(|attr| excludes_production(&attr.meta)) {
        return;
    }
    for item in items {
        let attrs = match item {
            Item::Fn(function) => &function.attrs,
            Item::Mod(module) => &module.attrs,
            _ => continue,
        };
        if attrs.iter().any(|attr| {
            excludes_production(&attr.meta)
                || attr
                    .path()
                    .segments
                    .last()
                    .is_some_and(|part| part.ident == "test")
        }) {
            continue;
        }
        match item {
            Item::Fn(function)
                if function.attrs.iter().any(|attr| {
                    attr.path()
                        .segments
                        .last()
                        .is_some_and(|part| part.ident == "rpc" || part.ident == "command")
                }) =>
            {
                let mut function = function.clone();
                function.attrs.splice(0..0, inherited.iter().cloned());
                commands.push(function);
            }
            Item::Mod(module) => {
                let name = module.ident.to_string();
                if matches!(name.as_str(), "tests" | "test_support" | "test_fixtures") {
                    continue;
                }
                let mut attrs = inherited.to_vec();
                attrs.extend(guards(&module.attrs));
                if let Some((_, items)) = &module.content {
                    collect_items(items, &directory.join(name), &attrs, commands);
                } else {
                    // Only ordinary explicit modules are supported, not macro-generated module trees.
                    assert!(
                        !module.attrs.iter().any(|attr| attr.path().is_ident("path")),
                        "catalog roots must use ordinary external modules"
                    );
                    let flat = directory.join(format!("{name}.rs"));
                    let nested = directory.join(&name).join("mod.rs");
                    assert_ne!(
                        flat.exists(),
                        nested.exists(),
                        "expected one module file for {name}"
                    );
                    collect_file(
                        if flat.exists() { &flat } else { &nested },
                        &attrs,
                        commands,
                    );
                }
            }
            _ => {}
        }
    }
}

fn render(commands: &[ItemFn]) -> String {
    let mut output = String::from(
        "// Generated by export_rpc_command_catalog; do not edit.\n\n/// Production command metadata, including commands whose native wrappers are not linked.\n#[rustfmt::skip]\npub const COMMAND_CATALOG: &[crate::CommandMetadata] = &[\n",
    );
    for command in commands {
        let options = unified_command::options(command).expect("RPC options must parse");
        unified_command::validate(command, &options).expect("RPC declaration must be valid");
        for attr in guards(&command.attrs) {
            output.push_str(&format!("    {}\n", attr.to_token_stream()));
        }
        let metadata = unified_command::metadata(command, &options, quote!(crate));
        output.push_str(&format!("    {metadata},\n"));
    }
    output.push_str("];\n");
    output
}

#[test]
#[ignore = "explicit generator; freshness checks never write"]
fn export_rpc_command_catalog() {
    fs::write(
        Path::new(env!("CARGO_MANIFEST_DIR")).join(CATALOG),
        render(&production_declarations()),
    )
    .expect("catalog must be writable");
}

#[test]
fn rpc_command_catalog_is_fresh() {
    let actual = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(CATALOG)).unwrap();
    assert_eq!(
        actual.replace("\r\n", "\n"),
        render(&production_declarations()),
        "run export_rpc_command_catalog explicitly after declaration changes"
    );
}

#[test]
fn catalog_preserves_mutually_exclusive_platform_cfg() {
    let source = syn::parse_file(
        r#"
        #[cfg(windows)] mod platform {
            #[cfg_attr(unix, cfg(target_os = "linux"))]
            mod nested { #[nyanpasu_macro::rpc] #[tauri::command] fn platform_call() {} }
        }
        #[cfg(not(windows))] #[nyanpasu_macro::rpc] #[tauri::command] fn platform_call() {}
    "#,
    )
    .unwrap();
    let mut commands = Vec::new();
    collect_items(&source.items, Path::new("unused"), &[], &mut commands);
    assert_eq!(commands.len(), 2);
    let output = render(&commands);
    assert!(output.contains("cfg (windows)"));
    assert!(output.contains("cfg (not (windows))"));
    assert!(output.contains("cfg_attr (unix , cfg (target_os = \"linux\"))"));
    let production = production_declarations();
    let uwp = production
        .iter()
        .filter(|command| command.sig.ident == "invoke_uwp_tool")
        .collect::<Vec<_>>();
    assert_eq!(uwp.len(), 2);
    assert!(uwp.iter().any(|command| {
        command
            .to_token_stream()
            .to_string()
            .contains("cfg (windows)")
    }));
    assert!(uwp.iter().any(|command| {
        command
            .to_token_stream()
            .to_string()
            .contains("cfg (not (windows))")
    }));
}

#[test]
fn catalog_follows_external_shared_modules() {
    let mut commands = Vec::new();
    collect_file(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/catalog/mod.rs"),
        &[],
        &mut commands,
    );
    assert_eq!(commands.len(), 2);
    assert_eq!(commands[0].sig.ident, "external_shared");
    assert_eq!(commands[1].sig.ident, "nested_shared");
    let output = render(&commands);
    assert_eq!(output.matches("cfg (windows)").count(), 2);
    assert_eq!(output.matches("CommandKind :: Shared").count(), 2);
    assert!(output.contains("RpcTarget :: Http"));
    assert!(output.contains("RpcTarget :: Local"));
}

#[test]
fn catalog_excludes_test_fixtures() {
    let source = syn::parse_file(
        r#"
        #[cfg(test)] mod fixtures { #[nyanpasu_macro::rpc] fn fixture() -> bool { true } }
        #[cfg(all(test, windows))] mod external_fixture;
        #[cfg_attr(not(test), cfg(any()))] mod conditional_fixture;
        mod test_support { #[nyanpasu_macro::rpc] fn support() -> bool { true } }
        #[test] #[nyanpasu_macro::rpc] fn fixture_test() -> bool { true }
        #[tokio::test] #[nyanpasu_macro::rpc] async fn async_fixture() -> bool { true }
        #[cfg(any(test, windows))] #[nyanpasu_macro::rpc] fn production() -> bool { true }
    "#,
    )
    .unwrap();
    let mut commands = Vec::new();
    collect_items(&source.items, Path::new("unused"), &[], &mut commands);
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].sig.ident, "production");
    assert!(!render(&production_declarations()).contains("audit_echo"));
}

#[test]
fn full_http_exposure_is_preserved_across_shared_split() {
    // E2a parent f56f08143: all 104 HTTP declarations are unconditional across
    // platform/feature cfgs. R3 removes set_hotkeys from HTTP; preserve the
    // remaining declarations and guards, not only names. R12 removes the three
    // icon reads in favor of the special byte entries.
    const BEFORE: &[&str] = &[
        "activate_profile",
        "change_clash_core",
        "clash_api_delete_connections",
        "clash_api_get_configs",
        "clash_api_get_group_delay",
        "clash_api_get_providers_proxies",
        "clash_api_get_providers_rules",
        "clash_api_get_proxy_delay",
        "clash_api_get_rules",
        "clash_api_get_version",
        "clash_api_healthcheck_proxy_provider",
        "clash_api_update_providers_rules",
        "clear_clash_ws_history",
        "clear_core_logs",
        "clear_proxy_fixed",
        "clear_storage",
        "close_log_session",
        "create_profile",
        "create_profile_folder",
        "create_proxy_group_folder",
        "delete_profile",
        "enhance_profiles",
        "fetch_latest_core_versions",
        "get_all_storage_items",
        "get_app_config",
        "get_clash_config",
        "get_clash_info",
        "get_clash_ws_snapshot",
        "get_config_features",
        "get_configuration_status",
        "get_core_agent_skills",
        "get_core_dir",
        "get_core_log",
        "get_core_log_status",
        "get_core_status",
        "get_debug_http_status",
        "get_hotkey_functions",
        "get_hotkeys",
        "get_ip_lookup_providers",
        "get_postprocessing_output",
        "get_profile_sync_logs",
        "get_profile_sync_runs",
        "get_profile_sync_status",
        "get_profiles",
        "get_proxies",
        "get_runtime_config",
        "get_runtime_exists",
        "get_runtime_yaml",
        "get_service_install_prompt",
        "get_storage_item",
        "get_sys_proxy",
        "get_system_accent_color",
        "get_traffic_summary",
        "import_profile",
        "inspect_applied_runtime",
        "inspect_runtime",
        "inspect_runtime_node",
        "inspect_updater",
        "is_appimage",
        "is_portable",
        "is_tray_icon_set",
        "is_widget_icon_set",
        "list_log_files",
        "mutate_proxies",
        "open_log_session",
        "patch_app_config",
        "patch_clash_config",
        "patch_profile_metadata",
        "patch_remote_profile_options",
        "patch_runtime_overrides",
        "probe_direct_egress",
        "query_core_logs",
        "query_logs",
        "query_traffic_active_connection_ids",
        "query_traffic_closed_connections",
        "query_traffic_report",
        "query_traffic_usage",
        "query_traffic_usage_by_keys",
        "read_profile_file",
        "remove_storage_item",
        "reorder_profile",
        "reorder_profiles_by_list",
        "replace_profile_definition",
        "report_frontend_events",
        "restart_sidecar",
        "retry_configuration_effect",
        "retry_configuration_runtime",
        "save_profile_file",
        "select_proxy",
        "set_clash_ws_recording",
        "set_global_transforms",
        "set_profile_layout",
        "set_profile_valid_fields",
        "set_proxy_group_layout",
        "set_storage_item",
        "set_tray_icon_from_bytes",
        "set_widget_icon_from_bytes",
        "status_service",
        "update_profile",
        "update_proxy_provider",
    ];
    let mut actual = Vec::new();
    for command in production_declarations() {
        let options = unified_command::options(&command).unwrap();
        if !unified_command::metadata(&command, &options, quote!(::nyanpasu_rpc_protocol))
            .to_string()
            .contains("RpcTarget :: Http")
        {
            continue;
        }
        assert!(
            guards(&command.attrs).next().is_none(),
            "HTTP cfg changed: {}",
            command.sig.ident
        );
        assert!(
            !unified_command::is_native(&command),
            "HTTP entry must be shared: {}",
            command.sig.ident
        );
        let expanded = unified_command::expand(command.clone())
            .unwrap()
            .to_string();
        assert!(
            expanded.contains("RpcTarget :: Http"),
            "HTTP handler missing: {}",
            command.sig.ident
        );
        assert!(!expanded.contains(":: tauri"));
        actual.push(command.sig.ident.to_string());
    }
    actual.sort();
    assert_eq!(actual, BEFORE);
}

#[test]
fn removed_icon_reads_are_absent_from_the_command_catalog() {
    let commands = production_declarations();
    let catalog = render(&commands);
    for name in ["get_cached_icon", "get_tray_icon", "get_widget_icon"] {
        assert!(
            !catalog.contains(&format!("\"{name}\"")),
            "removed read: {name}"
        );
    }
    for name in [
        "is_tray_icon_set",
        "is_widget_icon_set",
        "set_tray_icon_from_bytes",
        "set_widget_icon_from_bytes",
    ] {
        assert!(
            catalog.contains(&format!("\"{name}\"")),
            "preserved operation: {name}"
        );
    }
}

#[test]
fn shared_signatures_have_no_shell_owned_types() {
    fn use_paths(tree: &syn::UseTree, prefix: String, paths: &mut Vec<String>) {
        match tree {
            syn::UseTree::Path(path) => {
                use_paths(&path.tree, format!("{prefix}{}::", path.ident), paths)
            }
            syn::UseTree::Group(group) => {
                for item in &group.items {
                    use_paths(item, prefix.clone(), paths);
                }
            }
            syn::UseTree::Name(name) => paths.push(format!("{prefix}{}", name.ident)),
            syn::UseTree::Rename(rename) => paths.push(format!("{prefix}{}", rename.ident)),
            syn::UseTree::Glob(_) => panic!("neutral command imports must be explicit"),
        }
    }
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../nyanpasu-rpc/src/commands");
    let mut count = 0;
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        assert!(
            path.is_file(),
            "audit new nested neutral modules explicitly"
        );
        if path.extension().is_none_or(|extension| extension != "rs") {
            continue;
        }
        count += 1;
        let source = fs::read_to_string(&path).unwrap();
        let file = syn::parse_file(&source).unwrap();
        let tokens = file.to_token_stream().to_string();
        for forbidden in [
            "tauri ::",
            "AppHandle",
            "Webview",
            "JavaScriptChannelId",
            "crate :: ipc",
            "crate :: core",
            "crate :: utils",
            "crate :: window",
            "crate :: adapters",
            "nyanpasu_crash_reporter",
        ] {
            assert!(
                !tokens.contains(forbidden),
                "{} contains {forbidden}",
                path.display()
            );
        }
        for item in &file.items {
            if let Item::Use(item) = item {
                let mut paths = Vec::new();
                use_paths(&item.tree, String::new(), &mut paths);
                for imported in paths {
                    assert!(
                        !imported.starts_with("crate::")
                            || imported.starts_with("crate::registry::")
                            || imported.starts_with("crate::error::")
                            || imported.starts_with("crate::ports::"),
                        "shell-owned import: {imported}"
                    );
                }
            }
        }
    }
    assert!(
        count >= 4,
        "the complete shared signature and archive closure must be inspected"
    );
}
