//! Application-side signature oracle; protocol never links the application to run its inventory.

use std::{collections::BTreeMap, fs, path::Path};

use quote::ToTokens as _;
use syn::{
    Item, Token, Type,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
};

use crate::command_catalog::production_declarations;

type Imports = BTreeMap<String, String>;

struct Entry {
    name: syn::LitStr,
    guards: Vec<syn::Meta>,
    requests: Vec<Type>,
    result: Type,
    error: Type,
}

impl Parse for Entry {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let name = input.parse()?;
        input.parse::<Token![,]>()?;
        let mut guards = Vec::new();
        if input.peek(syn::token::Bracket) {
            let content;
            syn::bracketed!(content in input);
            guards.extend(Punctuated::<syn::Meta, Token![,]>::parse_terminated(
                &content,
            )?);
            input.parse::<Token![,]>()?;
        }
        let requests;
        syn::parenthesized!(requests in input);
        let requests = Punctuated::<Type, Token![,]>::parse_terminated(&requests)?;
        input.parse::<Token![,]>()?;
        let result = input.parse()?;
        input.parse::<Token![,]>()?;
        let error = input.parse()?;
        if !input.is_empty() {
            input.parse::<Token![,]>()?;
        }
        Ok(Self {
            name,
            guards,
            requests: requests.into_iter().collect(),
            result,
            error,
        })
    }
}

fn import_paths(tree: &syn::UseTree, prefix: &str, imports: &mut Imports) {
    match tree {
        syn::UseTree::Path(path) => {
            import_paths(&path.tree, &format!("{prefix}{}::", path.ident), imports)
        }
        syn::UseTree::Group(group) => {
            for tree in &group.items {
                import_paths(tree, prefix, imports);
            }
        }
        syn::UseTree::Name(name) => {
            if name.ident == "self" {
                let path = prefix.trim_end_matches("::");
                imports.insert(path.rsplit("::").next().unwrap().into(), path.into());
            } else {
                imports.insert(name.ident.to_string(), format!("{prefix}{}", name.ident));
            }
        }
        syn::UseTree::Rename(rename) => {
            imports.insert(
                rename.rename.to_string(),
                format!("{prefix}{}", rename.ident),
            );
        }
        // Native inline modules inherit their parent's imports through `super::*`.
        syn::UseTree::Glob(_) => assert_eq!(prefix, "super::"),
    }
}

fn contexts(items: &[Item], inherited: &Imports, result: &mut BTreeMap<String, Imports>) {
    let mut imports = inherited.clone();
    for item in items {
        if let Item::Use(item) = item {
            import_paths(&item.tree, "", &mut imports);
        }
    }
    // Native platform modules explicitly import some aliases from their parent.
    for path in imports.values_mut() {
        if let Some(relative) = path.strip_prefix("super::") {
            let (name, suffix) = relative.split_once("::").unwrap_or((relative, ""));
            if let Some(parent) = inherited.get(name) {
                *path = if suffix.is_empty() {
                    parent.clone()
                } else {
                    format!("{parent}::{suffix}")
                };
            }
        }
    }
    for item in items {
        match item {
            Item::Fn(item) => {
                result.insert(item.sig.to_token_stream().to_string(), imports.clone());
            }
            Item::Mod(item) => {
                if let Some((_, items)) = &item.content {
                    contexts(items, &imports, result);
                }
            }
            _ => {}
        }
    }
}

fn compact(value: impl ToString) -> String {
    value
        .to_string()
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn wire_type(ty: &Type, imports: &Imports, owner: &str) -> String {
    match ty {
        Type::Tuple(tuple) => format!(
            "({})",
            tuple
                .elems
                .iter()
                .map(|ty| wire_type(ty, imports, owner))
                .collect::<Vec<_>>()
                .join(",")
        ),
        Type::Reference(reference) => {
            assert_eq!(
                compact(reference.elem.to_token_stream()),
                "str",
                "only borrowed string results have a receiving value"
            );
            "String".into()
        }
        Type::Path(ty) => {
            assert!(ty.qself.is_none());
            let first = ty.path.segments.first().unwrap();
            let mut path = imports
                .get(&first.ident.to_string())
                .cloned()
                .unwrap_or_else(|| first.ident.to_string());
            for segment in ty.path.segments.iter().skip(1) {
                path.push_str(&format!("::{}", segment.ident));
            }
            if let Some(rest) = path.strip_prefix("crate::") {
                path = format!("{owner}::{rest}");
            }
            path = match path.as_str() {
                "nyanpasu_runtime_values::runtime::MutationOutcome" => {
                    "nyanpasu_rpc_protocol::wire::receive::MutationOutcome".into()
                }
                "nyanpasu_rpc::error::IpcError" => {
                    "nyanpasu_rpc_protocol::wire::receive::IpcError".into()
                }
                "std::path::PathBuf" => "PathBuf".into(),
                "std::string::String" => "String".into(),
                _ => path,
            };
            if let syn::PathArguments::AngleBracketed(args) =
                &ty.path.segments.last().unwrap().arguments
            {
                let args: Vec<_> = args
                    .args
                    .iter()
                    .map(|arg| match arg {
                        syn::GenericArgument::Type(ty) => wire_type(ty, imports, owner),
                        _ => panic!("audit new wire generic argument: {}", arg.to_token_stream()),
                    })
                    .collect();
                // Specta's Any wrapper is transparent on the wire.
                if path == "specta_typescript::Any" {
                    assert_eq!(args.len(), 1);
                    args[0].clone()
                } else {
                    format!("{path}<{}>", args.join(","))
                }
            } else {
                path
            }
        }
        _ => panic!("audit new wire type: {}", ty.to_token_stream()),
    }
}

#[test]
fn wire_inventory_matches_application_declaration_signatures() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut sources = vec![root.join("../tauri/src/ipc.rs")];
    sources.extend(
        fs::read_dir(root.join("../nyanpasu-rpc/src/commands"))
            .unwrap()
            .map(|entry| entry.unwrap().path()),
    );
    let mut application_imports = BTreeMap::new();
    for source in sources {
        let file = syn::parse_file(&fs::read_to_string(source).unwrap()).unwrap();
        contexts(&file.items, &Imports::new(), &mut application_imports);
    }
    let file = syn::parse_file(
        &fs::read_to_string(root.join("../nyanpasu-rpc-protocol/src/wire/closure_tests.rs"))
            .unwrap(),
    )
    .unwrap();
    let mut imports = Imports::new();
    for item in &file.items {
        if let Item::Use(item) = item {
            import_paths(&item.tree, "", &mut imports);
        }
    }
    let inventory = file
        .items
        .iter()
        .find_map(|item| match item {
            Item::Fn(item)
                if item.sig.ident == "every_registered_command_has_a_neutral_wire_closure" =>
            {
                Some(item)
            }
            _ => None,
        })
        .unwrap();
    let mut expected = Vec::new();
    for statement in &inventory.block.stmts {
        let syn::Stmt::Macro(statement) = statement else {
            continue;
        };
        if !statement.mac.path.is_ident("command") {
            continue;
        }
        let entry: Entry = syn::parse2(statement.mac.tokens.clone()).unwrap();
        expected.push((
            entry.name.value(),
            entry
                .guards
                .iter()
                .map(|guard| compact(guard.to_token_stream()))
                .collect::<Vec<_>>(),
            entry
                .requests
                .iter()
                .map(|ty| wire_type(ty, &imports, "nyanpasu_rpc_protocol"))
                .collect::<Vec<_>>(),
            wire_type(&entry.result, &imports, "nyanpasu_rpc_protocol"),
            wire_type(&entry.error, &imports, "nyanpasu_rpc_protocol"),
        ));
    }
    let mut actual = Vec::new();
    for command in production_declarations() {
        let imports = &application_imports[&command.sig.to_token_stream().to_string()];
        let requests = command
            .sig
            .inputs
            .iter()
            .filter_map(|arg| {
                let syn::FnArg::Typed(arg) = arg else {
                    panic!("command receiver")
                };
                if let Type::Path(ty) = &*arg.ty {
                    let name = ty.path.segments.last().unwrap().ident.to_string();
                    // Dependencies and native channel descriptors are injected by the adapter.
                    if matches!(
                        name.as_str(),
                        "State"
                            | "RpcState"
                            | "RpcCaller"
                            | "AppHandle"
                            | "Window"
                            | "Webview"
                            | "WebviewWindow"
                            | "Channel"
                    ) {
                        return None;
                    }
                }
                Some(wire_type(&arg.ty, imports, "nyanpasu_rpc"))
            })
            .collect::<Vec<_>>();
        let mut result = syn::parse_quote!(());
        let mut error = syn::parse_quote!(nyanpasu_rpc::error::IpcError);
        if let syn::ReturnType::Type(_, ty) = &command.sig.output {
            result = *ty.clone();
            if let Type::Path(path) = &**ty {
                let segment = path.path.segments.last().unwrap();
                let name = segment.ident.to_string();
                if matches!(name.as_str(), "Result" | "LogResult" | "CoreLogResult") {
                    let args: Vec<_> = match &segment.arguments {
                        syn::PathArguments::AngleBracketed(args) => args
                            .args
                            .iter()
                            .filter_map(|arg| match arg {
                                syn::GenericArgument::Type(ty) => Some(ty.clone()),
                                _ => None,
                            })
                            .collect(),
                        _ => Vec::new(),
                    };
                    result = args
                        .first()
                        .cloned()
                        .unwrap_or_else(|| syn::parse_quote!(()));
                    let resolved = wire_type(ty, imports, "nyanpasu_rpc");
                    let alias = resolved.split('<').next().unwrap();
                    let default_error = match alias {
                        "nyanpasu_rpc::error::Result" => {
                            syn::parse_quote!(nyanpasu_rpc::error::IpcError)
                        }
                        "nyanpasu_logging::LogResult" => {
                            syn::parse_quote!(nyanpasu_logging::LogError)
                        }
                        "nyanpasu_rpc_protocol::wire::logs::CoreLogResult" => {
                            syn::parse_quote!(nyanpasu_rpc_protocol::wire::logs::CoreLogError)
                        }
                        "Result" | "std::result::Result" => {
                            assert_eq!(args.len(), 2, "standard Result has two type arguments");
                            args[1].clone()
                        }
                        _ => panic!(
                            "audit the error type of result alias {alias} in {}",
                            command.sig.ident
                        ),
                    };
                    error = args.get(1).cloned().unwrap_or(default_error);
                }
            }
        }
        actual.push((
            command.sig.ident.to_string(),
            command
                .attrs
                .iter()
                .filter(|attr| attr.path().is_ident("cfg") || attr.path().is_ident("cfg_attr"))
                .map(|attr| compact(attr.meta.to_token_stream()))
                .collect::<Vec<_>>(),
            requests,
            wire_type(&result, imports, "nyanpasu_rpc"),
            wire_type(&error, imports, "nyanpasu_rpc"),
        ));
    }
    actual.sort();
    expected.sort();
    assert_eq!(
        actual.len(),
        expected.len(),
        "include every cfg declaration"
    );
    for (actual, expected) in actual.iter().zip(&expected) {
        assert_eq!(
            actual, expected,
            "request/result/error signature for {}",
            actual.0
        );
    }
}
