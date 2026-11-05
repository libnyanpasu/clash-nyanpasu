use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Error, FnArg, ItemFn, Pat, ReturnType, Type, spanned::Spanned};

#[derive(Default)]
pub struct Options {
    http: bool,
    local: bool,
    result: bool,
    owner: bool,
    capabilities: Vec<syn::Ident>,
    optional_capabilities: Vec<syn::Ident>,
}

impl syn::parse::Parse for Options {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut options = Self::default();
        while !input.is_empty() {
            let option: syn::Ident = input.parse()?;
            match option.to_string().as_str() {
                "http" => options.http = true,
                "local" => options.local = true,
                "result" => options.result = true,
                "owner" => options.owner = true,
                "capabilities" => {
                    let list;
                    syn::parenthesized!(list in input);
                    options
                        .capabilities
                        .extend(list.parse_terminated(syn::Ident::parse, syn::Token![,])?);
                }
                "optional_capabilities" => {
                    let list;
                    syn::parenthesized!(list in input);
                    options
                        .optional_capabilities
                        .extend(list.parse_terminated(syn::Ident::parse, syn::Token![,])?);
                }
                _ => {
                    return Err(Error::new(
                        option.span(),
                        "expected http, local, result, owner, capabilities(...) or optional_capabilities(...)",
                    ));
                }
            }
            if !input.is_empty() {
                input.parse::<syn::Token![,]>()?;
            }
        }
        Ok(options)
    }
}

/// Where the generated code finds the registry types.
const REGISTRY: &str = "::nyanpasu_rpc";

#[cfg(test)]
pub fn expand(item: ItemFn) -> syn::Result<TokenStream> {
    let options = item
        .attrs
        .iter()
        .find(|attr| {
            attr.path()
                .segments
                .last()
                .is_some_and(|s| s.ident == "rpc")
        })
        .filter(|attr| matches!(attr.meta, syn::Meta::List(_)))
        .map(|attr| attr.parse_args())
        .transpose()?
        .unwrap_or_default();
    expand_with_options(item, options)
}

/// Expands one command into a transport-neutral handler, served to the HTTP
/// and local-socket targets it opts into, plus a Tauri IPC handler when the
/// item is also a `#[tauri::command]`. Both register together with the
/// command's targets and shell capabilities.
pub fn expand_with_options(item: ItemFn, options: Options) -> syn::Result<TokenStream> {
    let registry: syn::Path = syn::parse_str(REGISTRY).expect("the registry path parses");
    let original = &item.sig;
    let name = &original.ident;
    let implementation_name = format_ident!("__unified_impl_{}", name);
    let neutral_handler = format_ident!("__unified_rpc_{}", name);
    let tauri_handler = format_ident!("__unified_tauri_{}", name);

    let is_tauri_command = item.attrs.iter().any(is_tauri_command_attribute);
    let neutral_compatible = can_share_with_neutral(original, options.owner);
    if !is_tauri_command && !neutral_compatible {
        return Err(Error::new(
            original.span(),
            "a command without #[tauri::command] must have a transport-neutral signature",
        ));
    }
    if options.local && !neutral_compatible {
        return Err(Error::new(
            original.span(),
            "rpc(local) requires a transport-neutral signature",
        ));
    }
    if options.local
        && let Some(elevation) = options
            .capabilities
            .iter()
            .chain(&options.optional_capabilities)
            .find(|capability| *capability == "elevation")
    {
        return Err(Error::new(
            elevation.span(),
            "a command that can reach elevation must not be exposed over the local socket",
        ));
    }

    let mut implementation = item.clone();
    implementation.attrs = item
        .attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("cfg"))
        .cloned()
        .collect();
    implementation.vis = syn::Visibility::Inherited;
    implementation.sig.ident = implementation_name.clone();

    let mut wrapper_args = Vec::new();
    let mut neutral_args = Vec::new();
    let mut fields = Vec::new();
    let mut field_names = Vec::new();
    let mut tauri_fields = Vec::new();
    let mut tauri_field_names = Vec::new();
    let mut tauri_supported = true;

    for argument in implementation.sig.inputs.iter_mut() {
        let FnArg::Typed(argument) = argument else {
            return Err(Error::new(
                argument.span(),
                "self receivers are not supported",
            ));
        };
        let Pat::Ident(binding) = argument.pat.as_ref() else {
            return Err(Error::new(
                argument.pat.span(),
                "command parameters must be identifiers",
            ));
        };
        let ident = binding.ident.clone();

        if let Some(state_type) = state_inner_type(&argument.ty) {
            let state_name = type_name(&state_type);
            argument.ty = Box::new(syn::parse_quote!(&#state_type));
            wrapper_args.push(quote!(&*#ident));
            if state_name.as_deref() == Some("NyanpasuClient") {
                neutral_args.push(quote!(&dependencies.client));
            } else if state_name.as_deref() == Some("Storage") {
                neutral_args.push(quote!(&dependencies.storage));
            } else if state_name.as_deref() == Some("PathResolver") {
                neutral_args.push(quote!(&dependencies.paths));
            } else if state_name.as_deref() == Some("WebTickets") {
                neutral_args.push(quote!(&dependencies.web_tickets));
            }
        } else if is_context_type(&argument.ty, "AppHandle") {
            wrapper_args.push(quote!(#ident));
        } else if options.owner && is_owner_context(&argument.ty) {
            *argument.ty = syn::parse_quote!(::nyanpasu_rpc_protocol::RpcOwner);
            wrapper_args.push(quote!(::nyanpasu_rpc_protocol::RpcOwner::desktop(#ident.label())));
            neutral_args.push(quote!(owner.clone()));
        } else if is_context_type(&argument.ty, "Window")
            || is_context_type(&argument.ty, "Webview")
            || is_context_type(&argument.ty, "WebviewWindow")
        {
            wrapper_args.push(quote!(#ident));
        } else if is_tauri_channel(&argument.ty) {
            // Decode the frontend descriptor, then bind it to the invoking webview.
            // Channel commands have no transport-neutral handler.
            wrapper_args.push(quote!(#ident));
            tauri_fields.push(quote!(#ident: ::tauri::ipc::JavaScriptChannelId));
            tauri_field_names.push(ident.clone());
        } else {
            let ty = argument.ty.clone();
            fields.push(quote!(#ident: #ty));
            field_names.push(ident.clone());
            tauri_fields.push(quote!(#ident: #ty));
            tauri_field_names.push(ident.clone());
            wrapper_args.push(quote!(#ident));
            neutral_args.push(quote!(#ident));
            if contains_reference(&argument.ty) {
                tauri_supported = false;
            }
        }
    }

    let generic_args = static_lifetime_args(original, &mut tauri_supported);
    let attrs = &item.attrs;
    let cfg_attrs = implementation.attrs.iter().collect::<Vec<_>>();
    let vis = &item.vis;
    let block = &item.block;
    let wrapper_call = call(
        &implementation_name,
        &wrapper_args,
        original.asyncness.is_some(),
        None,
    );
    let wrapper = quote! {
        #(#attrs)*
        #vis #original {
            #wrapper_call
        }
    };
    implementation.block = block.clone();

    // A Tauri command reaches the desktop through its own handler, so its
    // neutral handler is real only for the targets it opts into.
    let serves_neutral = options.http || options.local || !is_tauri_command;
    let neutral_handler_body = if neutral_compatible && serves_neutral {
        let parse = parser(&fields, &field_names);
        let neutral_call = call(
            &implementation_name,
            &neutral_args,
            original.asyncness.is_some(),
            None,
        );
        let output = output_value(&registry, &neutral_call, &original.output, options.result);
        quote! {
            #parse
            let output = #output?;
            ::serde_json::to_value(output)
                .map_err(#registry::application_error)
        }
    } else {
        quote! {
            Err(::nyanpasu_rpc_protocol::RpcError::unsupported(
                stringify!(#name),
                ::nyanpasu_rpc_protocol::RpcTarget::Http,
            ))
        }
    };

    let tauri_handler_definition = if !is_tauri_command {
        None
    } else if tauri_supported {
        let mut state_bindings = Vec::new();
        let mut tauri_args = Vec::new();
        for argument in &original.inputs {
            let FnArg::Typed(argument) = argument else {
                continue;
            };
            let Pat::Ident(binding) = argument.pat.as_ref() else {
                continue;
            };
            let ident = &binding.ident;
            if let Some(inner) = state_inner_type(&argument.ty) {
                let state_name = format_ident!("__state_{}", ident);
                state_bindings
                    .push(quote!(let #state_name = ::tauri::Manager::state::<#inner>(&app);));
                tauri_args.push(quote!(&*#state_name));
            } else if is_context_type(&argument.ty, "AppHandle") {
                tauri_args.push(quote!(app.clone()));
            } else if options.owner && is_owner_context(&argument.ty) {
                tauri_args.push(quote!(::nyanpasu_rpc_protocol::RpcOwner::desktop(
                    window.label()
                )));
            } else if is_context_type(&argument.ty, "Window") {
                tauri_args.push(quote!(window.clone()));
            } else if is_context_type(&argument.ty, "Webview")
                || is_context_type(&argument.ty, "WebviewWindow")
            {
                tauri_args.push(quote!(webview.clone()));
            } else if is_tauri_channel(&argument.ty) {
                tauri_args.push(quote!(#ident.channel_on(webview.clone())));
            } else {
                tauri_args.push(quote!(#ident));
            }
        }
        let generic_args = if generic_args.is_empty() {
            None
        } else {
            Some(generic_args.as_slice())
        };
        let tauri_call = call(
            &implementation_name,
            &tauri_args,
            original.asyncness.is_some(),
            generic_args,
        );
        let output = output_value(&registry, &tauri_call, &original.output, options.result);
        let parser = parser(&tauri_fields, &tauri_field_names);
        Some(quote! {
            #(#cfg_attrs)*
            fn #tauri_handler(
                app: ::tauri::AppHandle,
                window: ::tauri::Window,
                webview: ::tauri::Webview,
                params: ::serde_json::Value,
            ) -> #registry::RpcFuture {
                ::std::boxed::Box::pin(async move {
                    #parser
                    #(#state_bindings)*
                    let output = #output?;
                    ::serde_json::to_value(output)
                        .map_err(#registry::application_error)
                })
            }
        })
    } else {
        Some(quote! {
            #(#cfg_attrs)*
            fn #tauri_handler(
                _app: ::tauri::AppHandle,
                _window: ::tauri::Window,
                _webview: ::tauri::Webview,
                _params: ::serde_json::Value,
            ) -> #registry::RpcFuture {
                ::std::boxed::Box::pin(async move {
                    Err(::nyanpasu_rpc_protocol::RpcError::unsupported(
                        stringify!(#name),
                        ::nyanpasu_rpc_protocol::RpcTarget::Desktop,
                    ))
                })
            }
        })
    };
    let tauri_entry = tauri_handler_definition.is_some().then(|| {
        quote! {
            #(#cfg_attrs)*
            ::inventory::submit! {
                crate::unified_rpc::TauriCommandEntry {
                    name: stringify!(#name),
                    handler: #tauri_handler,
                }
            }
        }
    });

    let mut targets = vec![quote!(::nyanpasu_rpc_protocol::RpcTarget::Desktop)];
    if options.http && neutral_compatible {
        targets.push(quote!(::nyanpasu_rpc_protocol::RpcTarget::Http));
    }
    if options.local {
        targets.push(quote!(::nyanpasu_rpc_protocol::RpcTarget::Local));
    }
    let capability_variants = |capabilities: &[syn::Ident]| {
        capabilities
            .iter()
            .map(|capability| {
                let variant = format_ident!(
                    "{}",
                    snake_to_pascal(&capability.to_string()),
                    span = capability.span()
                );
                quote!(::nyanpasu_rpc_protocol::ShellCapability::#variant)
            })
            .collect::<Vec<_>>()
    };
    let capabilities = capability_variants(&options.capabilities);
    let optional_capabilities = capability_variants(&options.optional_capabilities);

    Ok(quote! {
        #implementation
        #wrapper

        #(#cfg_attrs)*
        fn #neutral_handler(
            dependencies: ::std::sync::Arc<#registry::RpcDependencies>,
            owner: ::nyanpasu_rpc_protocol::RpcOwner,
            params: ::serde_json::Value,
        ) -> #registry::RpcFuture {
            ::std::boxed::Box::pin(async move {
                #neutral_handler_body
            })
        }

        #tauri_handler_definition
        #tauri_entry

        #(#cfg_attrs)*
        ::inventory::submit! {
            #registry::CommandEntry {
                name: stringify!(#name),
                targets: &[#(#targets),*],
                capabilities: &[#(#capabilities),*],
                optional_capabilities: &[#(#optional_capabilities),*],
                handler: #neutral_handler,
            }
        }
    })
}

fn snake_to_pascal(name: &str) -> String {
    name.split('_')
        .map(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .map(|first| first.to_ascii_uppercase().to_string() + chars.as_str())
                .unwrap_or_default()
        })
        .collect()
}

fn is_tauri_command_attribute(attribute: &syn::Attribute) -> bool {
    let segments = &attribute.path().segments;
    segments.len() == 2 && segments[0].ident == "tauri" && segments[1].ident == "command"
}

fn call(
    name: &syn::Ident,
    args: &[TokenStream],
    is_async: bool,
    generic_args: Option<&[TokenStream]>,
) -> TokenStream {
    let generics = generic_args
        .map(|args| quote!(::<#(#args),*>))
        .unwrap_or_default();
    if is_async {
        quote!(#name #generics (#(#args),*).await)
    } else {
        quote!(#name #generics (#(#args),*))
    }
}

fn output_value(
    registry: &syn::Path,
    call: &TokenStream,
    output: &ReturnType,
    result: bool,
) -> TokenStream {
    match output {
        ReturnType::Type(_, ty) if result || is_result(ty) => {
            quote!(#call.map_err(#registry::application_error))
        }
        ReturnType::Type(_, _) => quote!(Ok(#call)),
        ReturnType::Default => quote!(Ok(#call)),
    }
}

fn parser(fields: &[TokenStream], names: &[syn::Ident]) -> TokenStream {
    if fields.is_empty() {
        quote! {
            if !params.is_null() && !matches!(&params, ::serde_json::Value::Object(map) if map.is_empty()) {
                return Err(::nyanpasu_rpc_protocol::RpcError::invalid_params("expected an empty input object"));
            }
        }
    } else {
        quote! {
            #[derive(::serde::Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args { #(#fields,)* }
            let Args { #(#names,)* } = ::serde_json::from_value(params)
                .map_err(|error| ::nyanpasu_rpc_protocol::RpcError::invalid_params(error.to_string()))?;
        }
    }
}

fn can_share_with_neutral(signature: &syn::Signature, owner: bool) -> bool {
    if signature
        .generics
        .params
        .iter()
        .any(|p| !matches!(p, syn::GenericParam::Lifetime(_)))
        || matches!(signature.output, ReturnType::Default)
    {
        return false;
    }
    for argument in &signature.inputs {
        let FnArg::Typed(argument) = argument else {
            return false;
        };
        if !matches!(argument.pat.as_ref(), Pat::Ident(_)) {
            return false;
        }
        if is_state(&argument.ty) {
            if !["NyanpasuClient", "Storage", "PathResolver", "WebTickets"]
                .iter()
                .any(|name| is_state_of(&argument.ty, name))
            {
                return false;
            }
        } else if is_context_type(&argument.ty, "AppHandle") {
            return false;
        } else if owner && is_owner_context(&argument.ty) {
            continue;
        } else if contains_reference(&argument.ty) || is_tauri_context(&argument.ty) {
            return false;
        }
    }
    true
}

fn static_lifetime_args(signature: &syn::Signature, supported: &mut bool) -> Vec<TokenStream> {
    signature
        .generics
        .params
        .iter()
        .map(|param| match param {
            syn::GenericParam::Lifetime(_) => quote!('static),
            _ => {
                *supported = false;
                quote!()
            }
        })
        .collect()
}

fn is_tauri_context(ty: &Type) -> bool {
    ["AppHandle", "Window", "Webview", "WebviewWindow"]
        .iter()
        .any(|name| is_context_type(ty, name))
        || is_tauri_channel(ty)
}

/// A parameter that stands for the caller of an `owner` command: the desktop
/// window, or `Caller` in a command that needs no shell context.
fn is_owner_context(ty: &Type) -> bool {
    is_context_type(ty, "Window") || is_context_type(ty, "Caller")
}

fn is_tauri_channel(ty: &Type) -> bool {
    matches!(ty, Type::Path(path)
        if path.path.segments.last().is_some_and(|segment| segment.ident == "Channel")
            && path.path.segments.iter().any(|segment| segment.ident == "tauri"))
}

fn is_context_type(ty: &Type, expected: &str) -> bool {
    matches!(ty, Type::Path(path) if path.path.segments.last().is_some_and(|segment| segment.ident == expected))
}

fn state_inner_type(ty: &Type) -> Option<Type> {
    let Type::Path(path) = ty else { return None };
    let segment = path.path.segments.last()?;
    if segment.ident != "State" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };
    args.args.iter().find_map(|arg| match arg {
        syn::GenericArgument::Type(ty) => Some(ty.clone()),
        _ => None,
    })
}

fn is_state(ty: &Type) -> bool {
    state_inner_type(ty).is_some()
}

fn is_state_of(ty: &Type, expected: &str) -> bool {
    state_inner_type(ty).is_some_and(|inner| is_context_type(&inner, expected))
}

fn type_name(ty: &Type) -> Option<String> {
    match ty {
        Type::Path(path) => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string()),
        _ => None,
    }
}

fn contains_reference(ty: &Type) -> bool {
    matches!(ty, Type::Reference(_))
}

fn is_result(ty: &Type) -> bool {
    matches!(ty, Type::Path(path) if path.path.segments.last().is_some_and(|segment| segment.ident == "Result"))
}

#[cfg(test)]
mod tests {
    use super::expand;
    use syn::{Item, ItemFn};

    #[test]
    fn explicit_result_alias_is_unwrapped_and_owner_is_injected() {
        let command = syn::parse_quote! {
            #[nyanpasu_macro::rpc(http, result, owner)]
            pub async fn query_logs(window: tauri::Window, request: Query) -> LogResult<Page> { todo!() }
        };
        let expanded = expand(command).unwrap().to_string();
        assert!(
            expanded.contains(
                "__unified_impl_query_logs (owner . clone () , request) . await . map_err"
            )
        );
        assert!(expanded.contains("RpcOwner :: desktop (window . label ())"));
        assert!(!expanded.contains("RpcError :: unsupported"));
        assert!(!expanded.contains("window : tauri :: Window , request : Query ,"));
    }

    #[test]
    fn dependency_free_commands_can_use_http() {
        let command = syn::parse_quote! { #[nyanpasu_macro::rpc(http)] pub fn functions() -> Vec<String> { vec![] } };
        assert!(
            !expand(command)
                .unwrap()
                .to_string()
                .contains("RpcError :: unsupported")
        );
    }

    #[test]
    fn the_path_resolver_state_is_served_from_the_http_dependencies() {
        let command = syn::parse_quote! {
            #[nyanpasu_macro::rpc(http)]
            pub fn is_tray_icon_set(paths: State<'_, PathResolver>, mode: TrayIcon) -> Result<bool> { todo!() }
        };
        let expanded = expand(command).unwrap().to_string();

        assert!(expanded.contains("& dependencies . paths"));
        assert!(!expanded.contains("RpcError :: unsupported"));
    }

    #[test]
    fn the_web_ticket_port_is_served_from_the_local_dependencies() {
        let command = syn::parse_quote! {
            #[nyanpasu_macro::rpc(local)]
            pub async fn issue(tickets: State<'_, WebTickets>) -> Result<WebTicket> { todo!() }
        };
        let expanded = expand(command).unwrap().to_string();

        assert!(expanded.contains("& dependencies . web_tickets"));
        assert!(!expanded.contains("RpcError :: unsupported (stringify ! (issue) , :: nyanpasu_rpc_protocol :: RpcTarget :: Local"));
    }

    #[test]
    fn channel_parameters_are_rebuilt_for_desktop_but_unsupported_over_http() {
        let command = syn::parse_quote! {
            #[nyanpasu_macro::rpc(http, owner)]
            #[tauri::command]
            pub fn stream(window: tauri::Window, on_chunk: tauri::ipc::Channel<String>) -> Result<()> { todo!() }
        };
        let expanded = expand(command).unwrap().to_string();

        assert!(expanded.contains("on_chunk : :: tauri :: ipc :: JavaScriptChannelId"));
        assert!(expanded.contains("on_chunk . channel_on (webview . clone ())"));
        assert!(expanded.contains("RpcOwner :: desktop (window . label ())"));
        assert!(!expanded.contains("struct Args { window"));
        assert!(expanded.contains("RpcError :: unsupported (stringify ! (stream)"));
        assert!(!expanded.contains(HTTP_TARGET));
    }

    #[test]
    fn channel_parameter_json_parser_uses_camel_case_fields() {
        let command = syn::parse_quote! {
            #[nyanpasu_macro::rpc(http)]
            #[tauri::command]
            pub fn stream(on_chunk: tauri::ipc::Channel<String>, page_size: usize) -> Result<()> {
                todo!()
            }
        };
        let expanded = expand(command).unwrap().to_string();

        assert!(expanded.contains("rename_all = \"camelCase\""));
        assert!(expanded.contains("page_size : usize"));
        assert!(expanded.contains("from_value (params)"));
        assert!(expanded.contains("on_chunk : :: tauri :: ipc :: JavaScriptChannelId"));
    }

    /// The registry exposes a command over HTTP only when its targets say so.
    const HTTP_TARGET: &str =
        "RpcTarget :: Desktop , :: nyanpasu_rpc_protocol :: RpcTarget :: Http";

    #[test]
    fn http_requires_opt_in_and_accepts_only_lifetime_generics() {
        for (source, supported) in [
            (
                "#[tauri::command] pub fn open_that(path: String) -> Result<()> { todo!() }",
                false,
            ),
            (
                "#[nyanpasu_macro::rpc(http)] #[tauri::command] pub fn collect_envs<'a>() -> Result<EnvInfo<'a>> { todo!() }",
                true,
            ),
            (
                "#[nyanpasu_macro::rpc(http)] #[tauri::command] pub fn generic<T>() -> Result<T> { todo!() }",
                false,
            ),
            (
                "#[nyanpasu_macro::rpc(http)] #[tauri::command] pub fn generic<const N: usize>() -> Result<usize> { todo!() }",
                false,
            ),
            (
                "#[nyanpasu_macro::rpc(http)] #[tauri::command] pub fn window(window: tauri::Window) -> Result<()> { todo!() }",
                false,
            ),
            (
                "#[nyanpasu_macro::rpc(http)] #[tauri::command] pub fn desktop(app: tauri::AppHandle) -> Result<()> { todo!() }",
                false,
            ),
        ] {
            let command: ItemFn = syn::parse_str(source).unwrap();
            let expanded = expand(command).unwrap().to_string();
            assert_eq!(expanded.contains(HTTP_TARGET), supported, "{source}");
            assert_eq!(
                !expanded.contains("RpcError :: unsupported"),
                supported,
                "{source}"
            );
        }
    }

    #[test]
    fn local_opts_in_separately_and_records_capabilities() {
        let command = syn::parse_quote! {
            #[nyanpasu_macro::rpc(http, local, capabilities(os_integration, hotkey))]
            #[tauri::command]
            pub async fn patch(client: State<'_, NyanpasuClient>, patch: Patch) -> Result<()> { todo!() }
        };
        let expanded = expand(command).unwrap().to_string();

        assert!(expanded.contains(&format!(
            "{HTTP_TARGET} , :: nyanpasu_rpc_protocol :: RpcTarget :: Local"
        )));
        assert!(expanded.contains(
            "capabilities : & [:: nyanpasu_rpc_protocol :: ShellCapability :: OsIntegration , \
             :: nyanpasu_rpc_protocol :: ShellCapability :: Hotkey]"
        ));
        assert!(expanded.contains("TauriCommandEntry"));
    }

    #[test]
    fn optional_capabilities_are_recorded_apart_from_required_ones() {
        let command = syn::parse_quote! {
            #[nyanpasu_macro::rpc(http, capabilities(clipboard), optional_capabilities(hotkey))]
            pub async fn patch(client: State<'_, NyanpasuClient>) -> Result<()> { todo!() }
        };
        let expanded = expand(command).unwrap().to_string();

        assert!(expanded.contains(
            "capabilities : & [:: nyanpasu_rpc_protocol :: ShellCapability :: Clipboard] , optional_capabilities : & [:: nyanpasu_rpc_protocol :: ShellCapability :: Hotkey]"
        ));
    }

    #[test]
    fn a_caller_parameter_is_the_owner_of_a_neutral_command() {
        let command = syn::parse_quote! {
            #[nyanpasu_macro::rpc(http, result, owner)]
            pub async fn open(caller: Caller, source: Source) -> LogResult<Session> { todo!() }
        };
        let expanded = expand(command).unwrap().to_string();

        assert!(expanded.contains("__unified_impl_open (owner . clone () , source) . await"));
        assert!(expanded.contains("caller : :: nyanpasu_rpc_protocol :: RpcOwner"));
        assert!(!expanded.contains("struct Args { caller"));
        assert!(!expanded.contains("RpcError :: unsupported"));
    }

    #[test]
    fn local_is_refused_for_desktop_signatures_and_elevation() {
        for source in [
            "#[nyanpasu_macro::rpc(local)] #[tauri::command] pub fn quit(app: tauri::AppHandle) -> Result<()> { todo!() }",
            "#[nyanpasu_macro::rpc(local, capabilities(elevation))] #[tauri::command] pub async fn update(client: State<'_, NyanpasuClient>) -> Result<()> { todo!() }",
            "#[nyanpasu_macro::rpc(local, optional_capabilities(elevation))] pub async fn update(client: State<'_, NyanpasuClient>) -> Result<()> { todo!() }",
            "#[nyanpasu_macro::rpc] pub fn window(window: tauri::Window) -> Result<()> { todo!() }",
        ] {
            let command: ItemFn = syn::parse_str(source).unwrap();
            assert!(expand(command).is_err(), "{source}");
        }
    }

    #[test]
    fn neutral_commands_serve_every_target_without_a_tauri_handler() {
        let command = syn::parse_quote! {
            #[nyanpasu_macro::rpc(local)]
            #[specta::specta]
            pub async fn stop(client: State<'_, NyanpasuClient>) -> Result<Report> { todo!() }
        };
        let expanded = expand(command).unwrap().to_string();

        assert!(expanded.contains("__unified_impl_stop (& dependencies . client) . await"));
        assert!(!expanded.contains("RpcError :: unsupported"));
        assert!(!expanded.contains("tauri ::"));
        assert!(!expanded.contains(HTTP_TARGET));
        assert!(expanded.contains("RpcTarget :: Local"));
    }

    #[test]
    fn every_ipc_rpc_command_has_a_desktop_dispatcher() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../tauri/src/ipc.rs");
        let source = std::fs::read_to_string(path).expect("failed to read IPC command source");
        let file = syn::parse_file(&source).expect("failed to parse IPC command source");
        let mut commands = Vec::new();
        collect_rpc_commands(&file.items, &mut commands);
        assert!(!commands.is_empty(), "expected RPC commands in ipc.rs");

        for command in commands {
            let name = command.sig.ident.to_string();
            let expanded =
                expand(command).unwrap_or_else(|error| panic!("failed to expand {name}: {error}"));
            let expanded = syn::parse2::<syn::File>(expanded)
                .unwrap_or_else(|error| panic!("failed to parse expansion for {name}: {error}"));
            let handler_name = format_ident!("__unified_tauri_{name}");
            let handler = expanded
                .items
                .iter()
                .find_map(|item| match item {
                    Item::Fn(function) if function.sig.ident == handler_name => Some(function),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("missing Tauri handler for {name}"));
            assert!(
                !handler
                    .block
                    .to_token_stream()
                    .to_string()
                    .contains("RpcError :: unsupported"),
                "Tauri command {name} was compiled into an unsupported dispatcher"
            );
        }
    }

    #[test]
    fn every_shared_command_has_a_neutral_handler_and_no_tauri_code() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../nyanpasu-rpc/src/commands.rs"
        );
        let source = std::fs::read_to_string(path).expect("failed to read shared command source");
        let file = syn::parse_file(&source).expect("failed to parse shared command source");
        let mut commands = Vec::new();
        collect_rpc_commands(&file.items, &mut commands);
        assert!(!commands.is_empty(), "expected shared RPC commands");

        for command in commands {
            let name = command.sig.ident.to_string();
            let expanded = expand(command)
                .unwrap_or_else(|error| panic!("failed to expand {name}: {error}"))
                .to_string();
            assert!(
                !expanded.contains("RpcError :: unsupported") && !expanded.contains("tauri ::"),
                "shared command {name} must expand to a neutral handler only"
            );
        }
    }

    #[test]
    fn every_ipc_tauri_command_uses_unified_rpc() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../tauri/src/ipc.rs");
        let source = std::fs::read_to_string(path).expect("failed to read IPC command source");
        let file = syn::parse_file(&source).expect("failed to parse IPC command source");
        let mut commands = Vec::new();
        collect_tauri_commands(&file.items, &mut commands);
        assert!(!commands.is_empty(), "expected Tauri commands in ipc.rs");

        let plain_commands = commands
            .into_iter()
            .filter(|command| {
                !command.attrs.iter().any(|attribute| {
                    attribute
                        .path()
                        .segments
                        .last()
                        .is_some_and(|segment| segment.ident == "rpc")
                })
            })
            .map(|command| command.sig.ident.to_string())
            .collect::<Vec<_>>();

        assert!(
            plain_commands.is_empty(),
            "Tauri application commands in ipc.rs must use #[nyanpasu_macro::rpc(...)]: {}",
            plain_commands.join(", ")
        );
    }

    fn collect_tauri_commands(items: &[Item], commands: &mut Vec<ItemFn>) {
        for item in items {
            match item {
                Item::Fn(function)
                    if function.attrs.iter().any(|attribute| {
                        attribute
                            .path()
                            .segments
                            .last()
                            .is_some_and(|segment| segment.ident == "command")
                    }) =>
                {
                    commands.push(function.clone())
                }
                Item::Mod(module) => {
                    if let Some((_, nested)) = &module.content {
                        collect_tauri_commands(nested, commands);
                    }
                }
                _ => {}
            }
        }
    }

    fn collect_rpc_commands(items: &[Item], commands: &mut Vec<ItemFn>) {
        for item in items {
            match item {
                Item::Fn(function)
                    if function.attrs.iter().any(|attribute| {
                        attribute
                            .path()
                            .segments
                            .last()
                            .is_some_and(|segment| segment.ident == "rpc")
                    }) =>
                {
                    commands.push(function.clone())
                }
                Item::Mod(module) => {
                    if let Some((_, nested)) = &module.content {
                        collect_rpc_commands(nested, commands);
                    }
                }
                _ => {}
            }
        }
    }

    use quote::{ToTokens, format_ident};
}
