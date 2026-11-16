//! Extraction from the explicitly listed production declaration roots, and the aggregate
//! permission table generated from it.

use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

use quote::{ToTokens as _, quote};
use syn::{Attribute, Item, ItemFn, Meta, Token, punctuated::Punctuated};

use crate::unified_command;

// Move these roots together with the schema and handler inventories when commands move.
const DECLARATION_ROOTS: &[&str] = &["tauri/src/ipc.rs", "nyanpasu-rpc/src/commands/mod.rs"];
#[cfg(test)]
const CATALOG: &str = "../nyanpasu-rpc-protocol/src/command_catalog.rs";
/// The platforms the permission table and the capability inventory are evaluated for.
pub(super) const PLATFORMS: [&str; 3] = ["windows", "linux", "macos"];
const PERMISSION_TABLE_FILE: &str = "rpc_permission_table.txt";
const PERMISSION_TABLE_HEADER: &str = "# Generated at build time by nyanpasu_macro::rpc_permission_table!; do not edit.\n# name | permission | scope | effect | targets\n# [baseline] uses Linux rows; platform differences: + row adds/replaces, - name removes.\n\n[baseline]\n";

/// The production declarations and every source file that declared them.
#[derive(Default)]
pub(super) struct Declarations {
    pub(super) commands: Vec<ItemFn>,
    pub(super) files: Vec<PathBuf>,
}

fn production_sources_at(backend_root: &Path) -> Declarations {
    let mut declarations = Declarations::default();
    for root in DECLARATION_ROOTS {
        collect_file(&backend_root.join(root), &[], &mut declarations);
    }
    assert!(
        !declarations.commands.is_empty(),
        "production declaration roots must not be empty"
    );
    declarations
}

#[cfg(test)]
pub(super) fn production_declarations() -> Vec<ItemFn> {
    production_sources_at(&backend_root()).commands
}

#[cfg(test)]
fn backend_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the macro crate lives under backend")
        .to_owned()
}

fn collect_file(path: &Path, inherited: &[Attribute], declarations: &mut Declarations) {
    declarations.files.push(path.to_owned());
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
    collect_items(&file.items, &directory, &attrs, declarations);
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
    declarations: &mut Declarations,
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
                declarations.commands.push(function);
            }
            Item::Mod(module) => {
                let name = module.ident.to_string();
                if matches!(name.as_str(), "tests" | "test_support" | "test_fixtures") {
                    continue;
                }
                let mut attrs = inherited.to_vec();
                attrs.extend(guards(&module.attrs));
                if let Some((_, items)) = &module.content {
                    collect_items(items, &directory.join(name), &attrs, declarations);
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
                        declarations,
                    );
                }
            }
            _ => {}
        }
    }
}

/// Whether the declaration is compiled on `platform`. Production commands are guarded only by
/// `cfg(windows)` and `cfg(not(windows))`; any other guard must extend this evaluation first.
pub(super) fn enabled_on(command: &ItemFn, platform: &str) -> bool {
    // The compiler and proc-macro2's fallback space tokens differently, so compare without spaces.
    guards(&command.attrs).all(|attr| {
        let guard = attr.to_token_stream().to_string().replace(' ', "");
        match guard.as_str() {
            "#[cfg(windows)]" => platform == "windows",
            "#[cfg(not(windows))]" => platform != "windows",
            guard => panic!("extend the platform evaluation for {guard}"),
        }
    })
}

/// The snake_case scope a declaration's permission resolves to, as the tables spell it.
pub(super) fn scope_name(command: &ItemFn, options: &unified_command::Options) -> String {
    let variant =
        unified_command::scope_variant(command, options).expect("permission must resolve");
    let mut name = String::new();
    for (index, character) in variant.to_string().chars().enumerate() {
        if character.is_ascii_uppercase() && index > 0 {
            name.push('_');
        }
        name.push(character.to_ascii_lowercase());
    }
    name
}

/// Renders per-platform rows as a Linux baseline plus `+`/`-` differences, in one fixed order.
pub(super) fn render_platform_sections(
    header: &str,
    inventories: &[(&str, BTreeMap<String, String>)],
) -> String {
    let baseline = &inventories
        .iter()
        .find(|(platform, _)| *platform == "linux")
        .unwrap()
        .1;
    let mut snapshot = String::from(header);
    for row in baseline.values() {
        snapshot.push_str(row);
        snapshot.push('\n');
    }
    for (platform, rows) in inventories {
        snapshot.push_str(&format!("\n[{platform}]\n"));
        for name in baseline.keys().filter(|name| !rows.contains_key(*name)) {
            snapshot.push_str(&format!("- {name}\n"));
        }
        for (name, row) in rows {
            if baseline.get(name) != Some(row) {
                snapshot.push_str(&format!("+ {row}\n"));
            }
        }
    }
    snapshot
}

/// The aggregate permission table: one row per command and platform, derived from the same
/// declarations as the catalog. The platform sections make it identical on every build host.
pub(super) fn render_permission_table(commands: &[ItemFn]) -> String {
    let mut inventories = Vec::new();
    for platform in PLATFORMS {
        let mut rows = BTreeMap::new();
        for command in commands
            .iter()
            .filter(|command| enabled_on(command, platform))
        {
            let options = unified_command::options(command).expect("RPC options must parse");
            unified_command::validate(command, &options).expect("RPC declaration must be valid");
            let scope = scope_name(command, &options);
            let permission = options
                .permission
                .as_ref()
                .expect("permission is validated by scope_name")
                .value();
            let mut targets = vec!["desktop"];
            if options.http {
                targets.push("http");
            }
            if options.local {
                targets.push("local");
            }
            let name = command.sig.ident.to_string();
            let row = format!(
                "{name} | {permission} | {scope} | {} | {}",
                if options.query { "query" } else { "mutation" },
                targets.join(","),
            );
            assert!(
                rows.insert(name, row).is_none(),
                "duplicate {platform} command"
            );
        }
        inventories.push((platform, rows));
    }
    render_platform_sections(PERMISSION_TABLE_HEADER, &inventories)
}

/// Writes only when the content differs and replaces atomically, so concurrent builds never read
/// a partial table and an unchanged table keeps its modification time.
fn write_if_changed(path: &Path, content: &str) -> Result<(), String> {
    if fs::read_to_string(path).is_ok_and(|existing| existing == content) {
        return Ok(());
    }
    let temporary = path.with_extension(format!("tmp{}", std::process::id()));
    fs::write(&temporary, content)
        .and_then(|()| fs::rename(&temporary, path))
        .map_err(|error| format!("cannot write {}: {error}", path.display()))
}

/// Expansion of `rpc_permission_table!`: regenerates the table under `OUT_DIR` and declares it.
pub(super) fn expand_permission_table() -> Result<proc_macro2::TokenStream, String> {
    let out_dir = std::env::var_os("OUT_DIR").ok_or(
        "rpc_permission_table! needs OUT_DIR, so the invoking package must have a build script",
    )?;
    let invoking_dir = env::var_os("CARGO_MANIFEST_DIR").ok_or(
        "rpc_permission_table! needs the invoking crate's CARGO_MANIFEST_DIR at expansion time",
    )?;
    let invoking_dir = Path::new(&invoking_dir);
    let backend_root = invoking_dir
        .parent()
        .ok_or("rpc_permission_table! must be invoked from the backend shell crate")?;
    expand_permission_table_in(Path::new(&out_dir), backend_root, invoking_dir)
}

fn expand_permission_table_in(
    out_dir: &Path,
    backend_root: &Path,
    invoking_dir: &Path,
) -> Result<proc_macro2::TokenStream, String> {
    let declarations = production_sources_at(backend_root);
    let table = out_dir.join(PERMISSION_TABLE_FILE);
    write_if_changed(&table, &render_permission_table(&declarations.commands))?;
    let mut sources = declarations
        .files
        .iter()
        .map(|file| relative_source_path(invoking_dir, file))
        .collect::<Result<Vec<_>, _>>()?;
    sources.sort();
    sources.dedup();
    Ok(quote! {
        /// The permission table of the production RPC declarations, as cargo generated it.
        #[allow(dead_code)]
        pub const RPC_PERMISSION_TABLE: &str =
            include_str!(concat!(env!("OUT_DIR"), "/rpc_permission_table.txt"));
        // Every declaration file is a dependency of the invoking crate, so a change reruns this macro.
        const _: &[&str] = &[
            #(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), #sources))),*
        ];
    })
}

/// The `include_str!` operand is relative to the invoking crate, so a copied build cache never
/// carries an absolute path from the checkout that compiled the proc macro.
fn relative_source_path(invoking_dir: &Path, source: &Path) -> Result<String, String> {
    let from: Vec<_> = invoking_dir.components().collect();
    let to: Vec<_> = source.components().collect();
    let common = from
        .iter()
        .zip(&to)
        .take_while(|(left, right)| left == right)
        .count();
    if common == 0 {
        return Err(format!(
            "{} is not on the invoking crate's path root",
            source.display()
        ));
    }

    let mut relative = String::new();
    for _ in &from[common..] {
        relative.push_str("/..");
    }
    for component in &to[common..] {
        let text = component
            .as_os_str()
            .to_str()
            .ok_or_else(|| format!("{} is not valid UTF-8", source.display()))?;
        relative.push('/');
        relative.push_str(text);
    }
    Ok(relative)
}

#[cfg(test)]
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
        let metadata = unified_command::metadata(command, &options, quote!(crate))
            .expect("RPC permission must resolve");
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

#[cfg(test)]
const PERMISSION_TABLE_SNAPSHOT: &str = "tests/fixtures/rpc-permission-table.txt";

#[test]
#[ignore = "explicit generator; freshness checks never write"]
fn export_rpc_permission_table() {
    fs::write(
        Path::new(env!("CARGO_MANIFEST_DIR")).join(PERMISSION_TABLE_SNAPSHOT),
        render_permission_table(&production_declarations()),
    )
    .expect("permission table snapshot must be writable");
}

/// The snapshot is the reviewed copy of the table the build generates; `render_permission_table`
/// is the function the macro writes `OUT_DIR` from, so equality here is equality with the build.
#[test]
fn rpc_permission_table_snapshot_is_fresh() {
    let actual =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(PERMISSION_TABLE_SNAPSHOT))
            .unwrap();
    assert_eq!(
        actual.replace("\r\n", "\n"),
        render_permission_table(&production_declarations()),
        "run export_rpc_permission_table explicitly after declaration changes"
    );
}

#[test]
fn permission_table_covers_every_command_once_per_platform() {
    let table = render_permission_table(&production_declarations());
    let baseline: Vec<_> = table
        .lines()
        .skip_while(|line| *line != "[baseline]")
        .skip(1)
        .take_while(|line| !line.is_empty())
        .collect();
    assert_eq!(baseline.len(), 193);
    let mut permissions: BTreeMap<&str, (&str, &str)> = BTreeMap::new();
    let mut names = std::collections::BTreeSet::new();
    for row in &baseline {
        let columns: Vec<_> = row.split(" | ").collect();
        assert_eq!(columns.len(), 5, "{row}");
        assert!(names.insert(columns[0]), "duplicate {}", columns[0]);
        // One permission means one scope and one effect everywhere.
        let previous = permissions.insert(columns[1], (columns[2], columns[3]));
        assert!(
            previous.is_none_or(|previous| previous == (columns[2], columns[3])),
            "ambiguous permission {}",
            columns[1]
        );
        let action = columns[1].split_once(':').unwrap().1;
        assert_eq!(
            columns[2] == "sensitive_read",
            action.starts_with("reveal"),
            "{row}"
        );
        assert_eq!(
            columns[2] == "remote_admin",
            columns[1].starts_with("remote_admin."),
            "{row}"
        );
    }
    // Permissions, scopes, effects and targets do not depend on the platform: the guarded
    // declarations differ only in their handlers, so every platform repeats the baseline.
    assert!(
        table.ends_with("\n[windows]\n\n[linux]\n\n[macos]\n"),
        "{table}"
    );
}

#[test]
fn rpc_permission_table_is_generated_deterministically_under_out_dir_only() {
    let directory = std::env::temp_dir().join(format!(
        "nyanpasu-macro-permission-table-{}",
        std::process::id()
    ));
    fs::create_dir_all(&directory).unwrap();
    let sources = backend_root();
    let before =
        fs::read_to_string(sources.join("nyanpasu-rpc-protocol/src/command_catalog.rs")).unwrap();
    let invoking = sources.join("tauri");

    let first = expand_permission_table_in(&directory, &sources, &invoking)
        .unwrap()
        .to_string();
    let table = directory.join(PERMISSION_TABLE_FILE);
    let written = fs::read_to_string(&table).unwrap();
    let modified = fs::metadata(&table).unwrap().modified().unwrap();
    let second = expand_permission_table_in(&directory, &sources, &invoking)
        .unwrap()
        .to_string();

    assert_eq!(first, second, "the expansion must be deterministic");
    assert_eq!(written, render_permission_table(&production_declarations()));
    assert_eq!(
        fs::metadata(&table).unwrap().modified().unwrap(),
        modified,
        "an unchanged table is not rewritten"
    );
    assert_eq!(
        fs::read_dir(&directory).unwrap().count(),
        1,
        "only the table is written"
    );
    assert!(first.contains("RPC_PERMISSION_TABLE"));
    assert!(first.contains("CARGO_MANIFEST_DIR"));
    assert!(first.contains("OUT_DIR"));
    assert!(
        !first.contains(sources.to_string_lossy().as_ref()),
        "the expansion must not bake the checkout path"
    );
    // Both roots and their module files are dependencies of the invoking crate.
    for tracked in ["/src/ipc.rs", "/../nyanpasu-rpc/src/commands/profiles.rs"] {
        assert!(first.contains(tracked), "{tracked} is not tracked");
    }
    assert_eq!(
        fs::read_to_string(sources.join("nyanpasu-rpc-protocol/src/command_catalog.rs")).unwrap(),
        before,
        "generation never writes shared source"
    );
    fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn permission_table_uses_the_invoking_checkout() {
    let directory = std::env::temp_dir().join(format!(
        "nyanpasu-macro-copied-checkout-{}",
        std::process::id()
    ));
    let backend = directory.join("backend");
    let tauri = backend.join("tauri");
    let commands = backend.join("nyanpasu-rpc/src/commands");
    fs::create_dir_all(tauri.join("src")).unwrap();
    fs::create_dir_all(&commands).unwrap();
    fs::write(
        tauri.join("src/ipc.rs"),
        r#"
        #[nyanpasu_macro::rpc(query, permission = "core.version:view")]
        #[tauri::command]
        fn command_from_the_current_checkout() {}
        "#,
    )
    .unwrap();
    fs::write(commands.join("mod.rs"), "").unwrap();
    let out = directory.join("out");
    fs::create_dir_all(&out).unwrap();

    let expanded = expand_permission_table_in(&out, &backend, &tauri).unwrap();
    let expanded = expanded.to_string();
    let table = fs::read_to_string(out.join(PERMISSION_TABLE_FILE)).unwrap();
    assert!(table.contains("command_from_the_current_checkout"));
    assert!(expanded.contains("/src/ipc.rs"));
    assert!(
        !expanded.contains(directory.to_string_lossy().as_ref()),
        "only paths relative to the invoking crate are emitted"
    );

    fs::remove_dir_all(directory).unwrap();
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
            mod nested { #[nyanpasu_macro::rpc(permission = "settings.storage_item:set")] #[tauri::command] fn platform_call() {} }
        }
        #[cfg(not(windows))] #[nyanpasu_macro::rpc(permission = "settings.storage_item:set")] #[tauri::command] fn platform_call() {}
    "#,
    )
    .unwrap();
    let mut declarations = Declarations::default();
    collect_items(&source.items, Path::new("unused"), &[], &mut declarations);
    assert_eq!(declarations.commands.len(), 2);
    let output = render(&declarations.commands);
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
    let mut declarations = Declarations::default();
    collect_file(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/catalog/mod.rs"),
        &[],
        &mut declarations,
    );
    let commands = &declarations.commands;
    assert_eq!(commands.len(), 2);
    assert_eq!(commands[0].sig.ident, "external_shared");
    assert_eq!(commands[1].sig.ident, "nested_shared");
    // Every module file that declared a command is tracked, so cargo rebuilds when one changes.
    assert_eq!(declarations.files.len(), 3);
    let output = render(commands);
    assert_eq!(output.matches("cfg (windows)").count(), 2);
    assert_eq!(output.matches("CommandKind :: Shared").count(), 2);
    assert!(output.contains("RpcTarget :: Http"));
    assert!(output.contains("RpcTarget :: Local"));
}

#[test]
fn catalog_excludes_test_fixtures() {
    let source = syn::parse_file(
        r#"
        #[cfg(test)] mod fixtures { #[nyanpasu_macro::rpc(permission = "settings.storage_item:set")] fn fixture() -> bool { true } }
        #[cfg(all(test, windows))] mod external_fixture;
        #[cfg_attr(not(test), cfg(any()))] mod conditional_fixture;
        mod test_support { #[nyanpasu_macro::rpc(permission = "settings.storage_item:set")] fn support() -> bool { true } }
        #[test] #[nyanpasu_macro::rpc(permission = "settings.storage_item:set")] fn fixture_test() -> bool { true }
        #[tokio::test] #[nyanpasu_macro::rpc(permission = "settings.storage_item:set")] async fn async_fixture() -> bool { true }
        #[cfg(any(test, windows))] #[nyanpasu_macro::rpc(permission = "settings.storage_item:set")] fn production() -> bool { true }
    "#,
    )
    .unwrap();
    let mut declarations = Declarations::default();
    collect_items(&source.items, Path::new("unused"), &[], &mut declarations);
    assert_eq!(declarations.commands.len(), 1);
    assert_eq!(declarations.commands[0].sig.ident, "production");
    assert!(!render(&production_declarations()).contains("audit_echo"));
}

#[test]
fn full_http_exposure_is_preserved_across_shared_split() {
    // E2a parent f56f08143: all 104 HTTP declarations are unconditional across
    // platform/feature cfgs. R3 removes set_hotkeys from HTTP; preserve the
    // remaining declarations and guards, not only names. R12 removes the three
    // icon reads in favor of the special byte entries. Remote access renames the debug
    // HTTP status read to get_http_server_status, which no longer returns a credential.
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
        "clash_api_get_settings",
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
        "get_app_settings",
        "get_clash_config",
        "get_clash_info",
        "get_clash_settings",
        "get_clash_ws_snapshot",
        "get_config_features",
        "get_configuration_status",
        "get_configuration_status_summary",
        "get_core_agent_skills",
        "get_core_agent_skills_summary",
        "get_core_dir",
        "get_core_log",
        "get_core_log_status",
        "get_core_log_status_summary",
        "get_core_status",
        "get_core_status_summary",
        "get_hotkey_functions",
        "get_hotkeys",
        "get_http_server_status",
        "get_ip_lookup_providers",
        "get_postprocessing_output",
        "get_profile_sync_logs",
        "get_profile_sync_runs",
        "get_profile_sync_runs_summary",
        "get_profile_sync_status",
        "get_profile_sync_status_summary",
        "get_profiles",
        "get_profiles_listing",
        "get_proxies",
        "get_proxy_group_icons",
        "get_runtime_config",
        "get_runtime_exists",
        "get_runtime_yaml",
        "get_service_install_prompt",
        "get_service_status_summary",
        "get_storage_item",
        "get_sys_proxy",
        "get_system_accent_color",
        "get_traffic_summary",
        "import_profile",
        "inspect_applied_runtime",
        "inspect_runtime",
        "inspect_runtime_node",
        "inspect_updater",
        "inspect_updater_summary",
        "is_appimage",
        "is_portable",
        "is_tray_icon_set",
        "is_widget_icon_set",
        "list_log_files",
        "mutate_proxies",
        "open_log_session",
        "patch_app_config",
        "patch_clash_config",
        "patch_clash_system_config",
        "patch_profile_metadata",
        "patch_remote_profile_options",
        "patch_runtime_access",
        "patch_runtime_overrides",
        "patch_system_settings",
        "probe_direct_egress",
        "query_core_logs",
        "query_core_logs_summary",
        "query_logs",
        "query_logs_summary",
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
        "rpc_describe",
        "save_profile_file",
        "select_proxy",
        "set_clash_ws_recording",
        "set_global_transforms",
        "set_profile_layout",
        "set_profile_valid_fields",
        "set_proxy_group_layout",
        "set_proxy_mode",
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
            .unwrap()
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
