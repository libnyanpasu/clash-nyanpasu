//! Development-only extraction from the explicitly listed production declaration roots.

use std::{fs, path::Path};

use quote::{ToTokens as _, quote};
use syn::{Attribute, Item, ItemFn, Meta, Token, punctuated::Punctuated};

use crate::unified_command;

// Move these roots together with the schema and handler inventories when commands move.
const DECLARATION_ROOTS: &[&str] = &["../tauri/src/ipc.rs"];
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
