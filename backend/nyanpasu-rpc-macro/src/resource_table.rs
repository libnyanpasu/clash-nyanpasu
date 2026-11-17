//! Development-only generator of the protocol's resource table from the explicitly listed
//! production declaration roots.
//!
//! The extractor walks both roots as one module tree, keeping each declaration's logical module
//! path and the root it came from. Every rule about what a declaration may say lives here, not in
//! the `rpc` attribute: the attribute registers implementations and the table is the only source
//! of authorization metadata.

use std::{
    fmt, fs,
    path::{Path, PathBuf},
};

use proc_macro2::TokenStream;
use quote::{ToTokens as _, format_ident, quote};
use syn::{
    Attribute, Ident, Item, ItemFn, LitStr, Meta, Token, bracketed, parenthesized,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
};

use crate::unified_command;

/// The declaration roots: the desktop shell's native commands and the shared commands. Move these
/// together with the schema and handler inventories when commands move.
const DECLARATION_ROOTS: &[(Root, &str)] = &[
    (Root::Desktop, "../tauri/src/ipc.rs"),
    (Root::Shared, "../nyanpasu-rpc/src/commands/mod.rs"),
];
const TABLE: &str = "../nyanpasu-rpc-protocol/src/permission/generated.rs";

/// The crate a declaration root belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Root {
    /// `clash-nyanpasu`, the only crate that may declare native commands.
    Desktop,
    /// `nyanpasu-rpc`, whose commands are transport-neutral.
    Shared,
}

/// One production declaration and where it lives.
pub(super) struct Declaration {
    pub(super) item: Declared,
    /// Module segments below the declaration root.
    pub(super) namespace: Vec<String>,
    pub(super) root: Root,
}

/// What a declaration declares.
pub(super) enum Declared {
    /// A function carrying the `rpc` attribute.
    Command(Box<ItemFn>),
    /// A `resource!` invocation: an event, an HTTP route or a stream.
    Resource(ResourceDeclaration),
}

impl Declaration {
    /// The command function, when this declares a command.
    pub(super) fn function(&self) -> Option<&ItemFn> {
        match &self.item {
            Declared::Command(function) => Some(function),
            Declared::Resource(_) => None,
        }
    }

    /// The attributes on the declaration, including the guards it inherits from its modules.
    fn attrs(&self) -> &[Attribute] {
        match &self.item {
            Declared::Command(function) => &function.attrs,
            Declared::Resource(resource) => &resource.attrs,
        }
    }
}

/// A non-command resource declared with
/// `resource!(event "name", targets = [desktop, http], permission = system:read, sensitive,
/// capabilities(app_update))`.
pub(super) struct ResourceDeclaration {
    kind: Ident,
    name: LitStr,
    targets: Vec<Ident>,
    permission: Option<unified_command::PermissionOption>,
    sensitive: bool,
    capabilities: Vec<Ident>,
    attrs: Vec<Attribute>,
}

impl Parse for ResourceDeclaration {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let list = |input: ParseStream<'_>| -> syn::Result<Vec<Ident>> {
            Ok(Punctuated::<Ident, Token![,]>::parse_terminated(input)?
                .into_iter()
                .collect())
        };
        let mut declaration = Self {
            kind: input.parse()?,
            name: input.parse()?,
            targets: Vec::new(),
            permission: None,
            sensitive: false,
            capabilities: Vec::new(),
            attrs: Vec::new(),
        };
        while !input.is_empty() {
            input.parse::<Token![,]>()?;
            if input.is_empty() {
                break;
            }
            let option: Ident = input.parse()?;
            match option.to_string().as_str() {
                "targets" => {
                    input.parse::<Token![=]>()?;
                    let content;
                    bracketed!(content in input);
                    declaration.targets = list(&content)?;
                }
                "permission" => {
                    input.parse::<Token![=]>()?;
                    declaration.permission = Some(input.parse()?);
                }
                "sensitive" => declaration.sensitive = true,
                "capabilities" => {
                    let content;
                    parenthesized!(content in input);
                    declaration.capabilities = list(&content)?;
                }
                _ => return Err(syn::Error::new(option.span(), "unknown resource option")),
            }
        }
        Ok(declaration)
    }
}

/// A rule a declaration breaks.
#[derive(Debug)]
pub(super) struct Invalid {
    name: String,
    reason: String,
}

impl fmt::Display for Invalid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.name, self.reason)
    }
}

pub(super) fn production_declarations() -> Vec<Declaration> {
    let mut declarations = Vec::new();
    for (root, path) in DECLARATION_ROOTS {
        collect_file(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join(path),
            *root,
            &[],
            &[],
            &mut declarations,
        );
    }
    assert!(
        !declarations.is_empty(),
        "production declaration roots must not be empty"
    );
    declarations
}

/// The production declarations without their location, for tests about the attribute.
pub(super) fn production_functions() -> Vec<ItemFn> {
    production_declarations()
        .into_iter()
        .filter_map(|declaration| match declaration.item {
            Declared::Command(function) => Some(*function),
            Declared::Resource(_) => None,
        })
        .collect()
}

fn collect_file(
    path: &Path,
    root: Root,
    namespace: &[String],
    inherited: &[Attribute],
    declarations: &mut Vec<Declaration>,
) {
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
    collect_items(
        &file.items,
        &directory,
        root,
        namespace,
        &attrs,
        declarations,
    );
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
    root: Root,
    namespace: &[String],
    inherited: &[Attribute],
    declarations: &mut Vec<Declaration>,
) {
    if inherited.iter().any(|attr| excludes_production(&attr.meta)) {
        return;
    }
    for item in items {
        let attrs = match item {
            Item::Fn(function) => &function.attrs,
            Item::Mod(module) => &module.attrs,
            Item::Macro(invocation) => &invocation.attrs,
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
                declarations.push(Declaration {
                    item: Declared::Command(Box::new(function)),
                    namespace: namespace.to_vec(),
                    root,
                });
            }
            Item::Macro(invocation)
                if invocation
                    .mac
                    .path
                    .segments
                    .last()
                    .is_some_and(|part| part.ident == "resource") =>
            {
                let mut resource: ResourceDeclaration = invocation
                    .mac
                    .parse_body()
                    .unwrap_or_else(|error| panic!("malformed resource declaration: {error}"));
                resource.attrs = inherited.to_vec();
                resource.attrs.extend(guards(&invocation.attrs));
                declarations.push(Declaration {
                    item: Declared::Resource(resource),
                    namespace: namespace.to_vec(),
                    root,
                });
            }
            Item::Mod(module) => {
                let name = module.ident.to_string();
                if matches!(name.as_str(), "tests" | "test_support" | "test_fixtures") {
                    continue;
                }
                let mut attrs = inherited.to_vec();
                attrs.extend(guards(&module.attrs));
                let mut namespace = namespace.to_vec();
                namespace.push(name.clone());
                if let Some((_, items)) = &module.content {
                    collect_items(
                        items,
                        &directory.join(&name),
                        root,
                        &namespace,
                        &attrs,
                        declarations,
                    );
                } else {
                    // Only ordinary explicit modules are supported, not macro-generated module trees.
                    assert!(
                        !module.attrs.iter().any(|attr| attr.path().is_ident("path")),
                        "declaration roots must use ordinary external modules"
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
                        root,
                        &namespace,
                        &attrs,
                        declarations,
                    );
                }
            }
            _ => {}
        }
    }
}

fn pascal_case(value: &syn::Ident) -> syn::Ident {
    let name: String = value
        .to_string()
        .split('_')
        .map(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .map(|first| first.to_ascii_uppercase().to_string() + chars.as_str())
                .unwrap_or_default()
        })
        .collect();
    format_ident!("{}", name, span = value.span())
}

/// One table row, after checking the rules a declaration must satisfy.
fn row(declaration: &Declaration) -> Result<TokenStream, Invalid> {
    let function = match &declaration.item {
        Declared::Command(function) => function,
        Declared::Resource(resource) => return resource_row(resource, &declaration.namespace),
    };
    let invalid = |reason: &str| Invalid {
        name: function.sig.ident.to_string(),
        reason: reason.to_owned(),
    };
    let options =
        unified_command::options(function).map_err(|error| invalid(&error.to_string()))?;
    unified_command::validate(function, &options).map_err(|error| invalid(&error.to_string()))?;
    let Some(permission) = &options.permission else {
        return Err(invalid(
            "a legacy declaration requires an explicit `permission = resource:access`",
        ));
    };
    let permission = permission_tokens(permission);
    let sensitive = options.sensitive;
    let native = unified_command::is_native(function);
    if native && declaration.root != Root::Desktop {
        return Err(invalid(
            "only the desktop shell's declaration root may declare native commands",
        ));
    }

    let name = function.sig.ident.to_string();
    let namespace = declaration.namespace.join(".");
    let kind = if native {
        quote!(Native)
    } else {
        quote!(Shared)
    };
    let effect = if options.query {
        quote!(Query)
    } else {
        quote!(Mutation)
    };
    let mut targets = vec![quote!(crate::RpcTarget::Desktop)];
    if options.http {
        targets.push(quote!(crate::RpcTarget::Http));
    }
    if options.local {
        targets.push(quote!(crate::RpcTarget::Local));
    }
    let required = capability_variants(&options.capabilities);
    let optional = capability_variants(&options.optional_capabilities);
    Ok(quote! {
        crate::permission::ResourceMetadata {
            name: #name,
            namespace: #namespace,
            kind: crate::permission::ResourceKind::Command(crate::CommandKind::#kind),
            effect: crate::CommandEffect::#effect,
            permission: #permission,
            source: crate::permission::PermissionSource::Legacy,
            sensitive: #sensitive,
            targets: &[#(#targets),*],
            capabilities: &[#(#required),*],
            optional_capabilities: &[#(#optional),*],
        }
    })
}

/// The requirement a legacy declaration names; parsing already checked the names.
fn permission_tokens(permission: &unified_command::PermissionOption) -> TokenStream {
    let resource = pascal_case(&permission.resource);
    let access = pascal_case(&permission.access);
    quote!(nyanpasu_config::application::Permission::new(
        nyanpasu_config::application::Resource::#resource,
        nyanpasu_config::application::Access::#access
    ))
}

fn capability_variants(values: &[Ident]) -> Vec<TokenStream> {
    values
        .iter()
        .map(|value| {
            let variant = pascal_case(value);
            quote!(crate::ShellCapability::#variant)
        })
        .collect()
}

/// The row of an event, route or stream. They only read, and name their exposure explicitly
/// because routes and streams are not offered to the desktop.
fn resource_row(
    resource: &ResourceDeclaration,
    namespace: &[String],
) -> Result<TokenStream, Invalid> {
    let name = resource.name.value();
    let invalid = |reason: &str| Invalid {
        name: name.clone(),
        reason: reason.to_owned(),
    };
    let kind = match resource.kind.to_string().as_str() {
        "event" => quote!(Event),
        "route" => quote!(Route),
        "stream" => quote!(Stream),
        _ => return Err(invalid("a resource is an `event`, a `route` or a `stream`")),
    };
    let Some(permission) = &resource.permission else {
        return Err(invalid(
            "a resource requires an explicit `permission = resource:access`",
        ));
    };
    if permission.access != "read" {
        return Err(invalid("events, routes and streams only read"));
    }
    let permission = permission_tokens(permission);
    let sensitive = resource.sensitive;
    if resource.targets.is_empty() {
        return Err(invalid("a resource requires explicit `targets = [...]`"));
    }
    let mut targets = Vec::new();
    for target in &resource.targets {
        if !matches!(target.to_string().as_str(), "desktop" | "http" | "local") {
            return Err(invalid("targets are `desktop`, `http` and `local`"));
        }
        let variant = pascal_case(target);
        targets.push(quote!(crate::RpcTarget::#variant));
    }
    let capabilities = capability_variants(&resource.capabilities);
    let namespace = namespace.join(".");
    Ok(quote! {
        crate::permission::ResourceMetadata {
            name: #name,
            namespace: #namespace,
            kind: crate::permission::ResourceKind::#kind,
            effect: crate::CommandEffect::Query,
            permission: #permission,
            source: crate::permission::PermissionSource::Legacy,
            sensitive: #sensitive,
            targets: &[#(#targets),*],
            capabilities: &[#(#capabilities),*],
            optional_capabilities: &[],
        }
    })
}

fn render(declarations: &[Declaration]) -> Result<String, Vec<Invalid>> {
    let mut output = String::from(
        "// Generated by export_rpc_resource_table; do not edit.\n\n/// Every production resource, including commands whose native wrappers are not linked.\n#[rustfmt::skip]\npub const RESOURCES: &[crate::permission::ResourceMetadata] = &[\n",
    );
    let mut errors = Vec::new();
    for declaration in declarations {
        match row(declaration) {
            Ok(row) => {
                for attr in guards(declaration.attrs()) {
                    output.push_str(&format!("    {}\n", attr.to_token_stream()));
                }
                output.push_str(&format!("    {row},\n"));
            }
            Err(error) => errors.push(error),
        }
    }
    output.push_str("];\n");
    if errors.is_empty() {
        Ok(output)
    } else {
        Err(errors)
    }
}

fn render_production() -> String {
    render(&production_declarations()).unwrap_or_else(|errors| {
        let errors: Vec<_> = errors.iter().map(ToString::to_string).collect();
        panic!("invalid declarations:\n{}", errors.join("\n"))
    })
}

#[test]
#[ignore = "explicit generator; freshness checks never write"]
fn export_rpc_resource_table() {
    fs::write(
        Path::new(env!("CARGO_MANIFEST_DIR")).join(TABLE),
        render_production(),
    )
    .expect("the resource table must be writable");
}

#[test]
fn rpc_resource_table_is_fresh() {
    let actual = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(TABLE)).unwrap();
    assert_eq!(
        actual.replace("\r\n", "\n"),
        render_production(),
        "run export_rpc_resource_table explicitly after declaration changes"
    );
}

fn declarations_in(source: &str) -> Vec<Declaration> {
    let source = syn::parse_file(source).unwrap();
    let mut declarations = Vec::new();
    collect_items(
        &source.items,
        Path::new("unused"),
        Root::Shared,
        &[],
        &[],
        &mut declarations,
    );
    declarations
}

fn rejection(source: &str) -> String {
    let errors = render(&declarations_in(source)).unwrap_err();
    assert_eq!(errors.len(), 1, "{errors:?}");
    errors[0].to_string()
}

#[test]
fn a_legacy_declaration_requires_a_permission() {
    for source in [
        "#[nyanpasu_rpc::rpc(http)] fn update() -> bool { true }",
        "#[nyanpasu_rpc::rpc(http, query)] fn update() -> bool { true }",
    ] {
        assert!(
            rejection(source).contains(
                "update: a legacy declaration requires an explicit `permission = resource:access`"
            ),
            "{source}"
        );
    }
}

#[test]
fn a_permission_names_a_resource_and_a_level() {
    assert!(
        rejection(
            "#[nyanpasu_rpc::rpc(query, permission = secrets:read)] fn inspect() -> bool { true }"
        )
        .contains("`secrets` is not a resource")
    );
    assert!(
        rejection(
            "#[nyanpasu_rpc::rpc(query, permission = proxy:admin)] fn inspect() -> bool { true }"
        )
        .contains("`admin` is not an access level")
    );
}

#[test]
fn the_permission_and_marking_are_written_as_declared() {
    let table = render(&declarations_in(
        "#[nyanpasu_rpc::rpc(http, query, permission = profile:read, sensitive)] fn status() -> bool { true }",
    ))
    .unwrap();
    assert!(table.contains("effect : crate :: CommandEffect :: Query"));
    assert!(
        table.contains("Resource :: Profile , nyanpasu_config :: application :: Access :: Read")
    );
    assert!(table.contains("sensitive : true"));
    assert!(table.contains("source : crate :: permission :: PermissionSource :: Legacy"));
}

#[test]
fn the_permission_is_independent_of_the_effect() {
    let table = render(&declarations_in(
        "#[nyanpasu_rpc::rpc(query, permission = proxy:write)] fn inspect() -> bool { true }",
    ))
    .unwrap();
    assert!(table.contains("CommandEffect :: Query"));
    assert!(table.contains("Access :: Write"));
    assert!(table.contains("sensitive : false"));
    let table = render(&declarations_in(
        "#[nyanpasu_rpc::rpc(http, permission = logs:read)] fn open_resource() -> bool { true }",
    ))
    .unwrap();
    assert!(table.contains("CommandEffect :: Mutation"));
    assert!(table.contains("Access :: Read"));
}

#[test]
fn only_the_desktop_root_declares_native_commands() {
    assert!(
        rejection(
            "#[nyanpasu_rpc::rpc(permission = system:write)] #[tauri::command] fn open_that() -> bool { true }"
        )
        .contains("only the desktop shell's declaration root may declare native commands")
    );
}

#[test]
fn every_invalid_declaration_is_reported() {
    let errors = render(&declarations_in(
        "#[nyanpasu_rpc::rpc(http)] fn first() -> bool { true }
         #[nyanpasu_rpc::rpc(local)] fn second() -> bool { true }",
    ))
    .unwrap_err();
    assert_eq!(errors.len(), 2);
}

#[test]
fn table_preserves_mutually_exclusive_platform_cfg() {
    let mut declarations = declarations_in(
        r#"
        #[cfg(windows)] mod platform {
            #[cfg_attr(unix, cfg(target_os = "linux"))]
            mod nested { #[nyanpasu_rpc::rpc(permission = settings:write)] #[tauri::command] fn platform_call() {} }
        }
        #[cfg(not(windows))] #[nyanpasu_rpc::rpc(permission = settings:write)] #[tauri::command] fn platform_call() {}
    "#,
    );
    for declaration in &mut declarations {
        declaration.root = Root::Desktop;
    }
    assert_eq!(declarations.len(), 2);
    assert_eq!(declarations[0].namespace, ["platform", "nested"]);
    assert!(declarations[1].namespace.is_empty());
    let output = render(&declarations).unwrap();
    assert!(output.contains("cfg (windows)"));
    assert!(output.contains("cfg (not (windows))"));
    assert!(output.contains("cfg_attr (unix , cfg (target_os = \"linux\"))"));
    assert!(output.contains("namespace : \"platform.nested\""));
    let production = production_functions();
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
fn table_follows_external_shared_modules() {
    let mut declarations = Vec::new();
    collect_file(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/catalog/mod.rs"),
        Root::Shared,
        &[],
        &[],
        &mut declarations,
    );
    assert_eq!(declarations.len(), 2);
    assert_eq!(
        declarations[0].function().unwrap().sig.ident,
        "external_shared"
    );
    assert_eq!(declarations[0].namespace, ["shared"]);
    assert_eq!(
        declarations[1].function().unwrap().sig.ident,
        "nested_shared"
    );
    assert_eq!(declarations[1].namespace, ["shared", "nested"]);
    let output = render(&declarations).unwrap();
    assert_eq!(output.matches("cfg (windows)").count(), 2);
    assert_eq!(output.matches("CommandKind :: Shared").count(), 2);
    assert!(output.contains("namespace : \"shared.nested\""));
    assert!(output.contains("RpcTarget :: Http"));
    assert!(output.contains("RpcTarget :: Local"));
}

#[test]
fn production_namespaces_follow_the_module_tree() {
    let declarations = production_declarations();
    let find = |name: &str| {
        declarations
            .iter()
            .find(|declaration| {
                declaration
                    .function()
                    .is_some_and(|function| function.sig.ident == name)
            })
            .unwrap_or_else(|| panic!("missing {name}"))
    };
    let shared = find("read_profile_file");
    assert_eq!(shared.root, Root::Shared);
    assert_eq!(shared.namespace, ["profiles"]);
    let native = find("read_clipboard_text");
    assert_eq!(native.root, Root::Desktop);
    assert!(native.namespace.is_empty());
}

#[test]
fn table_excludes_test_fixtures() {
    let declarations = declarations_in(
        r#"
        #[cfg(test)] mod fixtures { #[nyanpasu_rpc::rpc(permission = settings:write)] fn fixture() -> bool { true } }
        #[cfg(all(test, windows))] mod external_fixture;
        #[cfg_attr(not(test), cfg(any()))] mod conditional_fixture;
        mod test_support { #[nyanpasu_rpc::rpc(permission = settings:write)] fn support() -> bool { true } }
        #[test] #[nyanpasu_rpc::rpc(permission = settings:write)] fn fixture_test() -> bool { true }
        #[tokio::test] #[nyanpasu_rpc::rpc(permission = settings:write)] async fn async_fixture() -> bool { true }
        #[cfg(any(test, windows))] #[nyanpasu_rpc::rpc(permission = settings:write)] fn production() -> bool { true }
    "#,
    );
    assert_eq!(declarations.len(), 1);
    assert_eq!(declarations[0].function().unwrap().sig.ident, "production");
    assert!(!render_production().contains("audit_echo"));
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
    for command in production_functions() {
        let options = unified_command::options(&command).unwrap();
        if !options.http {
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
            expanded.contains("transport_neutral : true"),
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
fn removed_icon_reads_are_absent_from_the_resource_table() {
    let table = render_production();
    for name in ["get_cached_icon", "get_tray_icon", "get_widget_icon"] {
        assert!(
            !table.contains(&format!("\"{name}\"")),
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
            table.contains(&format!("\"{name}\"")),
            "preserved operation: {name}"
        );
    }
}

/// Every shell-owned type or import in the shared declaration tree, one line each.
fn shell_owned_references(directory: &Path) -> (usize, Vec<String>) {
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
            syn::UseTree::Glob(_) => paths.push(format!("{prefix}*")),
        }
    }
    fn files(directory: &Path, found: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                files(&path, found);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                found.push(path);
            }
        }
    }
    let mut paths = Vec::new();
    files(directory, &mut paths);
    paths.sort();
    let mut references = Vec::new();
    for path in &paths {
        let source = fs::read_to_string(path).unwrap();
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
            if tokens.contains(forbidden) {
                references.push(format!("{} contains {forbidden}", path.display()));
            }
        }
        for item in &file.items {
            if let Item::Use(item) = item {
                let mut imported = Vec::new();
                use_paths(&item.tree, String::new(), &mut imported);
                for imported in imported {
                    if imported.ends_with('*') {
                        references.push(format!(
                            "{}: neutral command imports must be explicit: {imported}",
                            path.display()
                        ));
                    } else if imported.starts_with("crate::")
                        && !imported.starts_with("crate::registry::")
                        && !imported.starts_with("crate::error::")
                        && !imported.starts_with("crate::ports::")
                    {
                        references.push(format!(
                            "{}: shell-owned import: {imported}",
                            path.display()
                        ));
                    }
                }
            }
        }
    }
    (paths.len(), references)
}

#[test]
fn shared_signatures_have_no_shell_owned_types() {
    let (count, references) = shell_owned_references(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../nyanpasu-rpc/src/commands"),
    );
    assert!(references.is_empty(), "{}", references.join("\n"));
    assert!(
        count >= 4,
        "the complete shared signature and archive closure must be inspected"
    );
}

#[test]
fn the_signature_guard_descends_into_nested_modules() {
    let (count, references) = shell_owned_references(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/signature-guard"),
    );
    assert_eq!(count, 2);
    assert_eq!(references.len(), 2, "{references:?}");
    assert!(references[0].contains("contains AppHandle"));
    assert!(references[1].contains("shell-owned import: crate::shell::Window"));
}

#[test]
fn resources_render_with_their_explicit_exposure() {
    let table = render(&declarations_in(
        r#"
        nyanpasu_rpc::resource!(event "storage-value-changed-event", targets = [desktop, http], permission = settings:read, sensitive);
        nyanpasu_rpc::resource!(event "app-update-state-changed", targets = [desktop], permission = system:read, capabilities(app_update));
        nyanpasu_rpc::resource!(route "icon_fetch", targets = [http], permission = proxy:read);
        nyanpasu_rpc::resource!(stream "connection_details", targets = [http, local], permission = traffic:read);
        "#,
    ))
    .unwrap();
    assert!(table.contains("name : \"storage-value-changed-event\""));
    assert!(table.contains("kind : crate :: permission :: ResourceKind :: Event"));
    assert!(
        table.contains("Resource :: Settings , nyanpasu_config :: application :: Access :: Read")
    );
    assert_eq!(table.matches("sensitive : true").count(), 1);
    assert!(table.contains("capabilities : & [crate :: ShellCapability :: AppUpdate]"));
    assert!(table.contains("kind : crate :: permission :: ResourceKind :: Route"));
    assert!(table.contains("targets : & [crate :: RpcTarget :: Http]"));
    assert!(table.contains("kind : crate :: permission :: ResourceKind :: Stream"));
    assert_eq!(table.matches("CommandEffect :: Query").count(), 4);
}

#[test]
fn a_resource_names_its_kind_permission_and_targets() {
    for (source, reason) in [
        (
            r#"nyanpasu_rpc::resource!(signal "x", targets = [http], permission = proxy:read);"#,
            "a resource is an `event`, a `route` or a `stream`",
        ),
        (
            r#"nyanpasu_rpc::resource!(event "x", targets = [http]);"#,
            "a resource requires an explicit `permission = resource:access`",
        ),
        (
            r#"nyanpasu_rpc::resource!(event "x", targets = [http], permission = proxy:write);"#,
            "events, routes and streams only read",
        ),
        (
            r#"nyanpasu_rpc::resource!(event "x", permission = proxy:read);"#,
            "a resource requires explicit `targets = [...]`",
        ),
        (
            r#"nyanpasu_rpc::resource!(event "x", targets = [web], permission = proxy:read);"#,
            "targets are `desktop`, `http` and `local`",
        ),
    ] {
        assert_eq!(rejection(source), format!("x: {reason}"), "{source}");
    }
}

#[test]
fn production_resources_are_collected() {
    let declarations = production_declarations();
    let resources = declarations
        .iter()
        .filter(|declaration| declaration.function().is_none())
        .count();
    assert_eq!(resources, 20);
}
