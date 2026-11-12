use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Error, FnArg, ItemFn, Pat, ReturnType, Type, spanned::Spanned};

#[derive(Default)]
pub struct Options {
    http: bool,
    local: bool,
    capabilities: Vec<syn::Ident>,
    optional_capabilities: Vec<syn::Ident>,
    result: bool,
    owner: bool,
    operation: Option<syn::Path>,
}

impl syn::parse::Parse for Options {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut options = Self::default();
        while !input.is_empty() {
            let option: syn::Ident = input.parse()?;
            match option.to_string().as_str() {
                "http" => options.http = true,
                "local" => options.local = true,
                "capabilities" | "optional_capabilities" => {
                    let list;
                    syn::parenthesized!(list in input);
                    let values = list.parse_terminated(syn::Ident::parse, syn::Token![,])?;
                    if option == "capabilities" {
                        options.capabilities.extend(values);
                    } else {
                        options.optional_capabilities.extend(values);
                    }
                }
                "result" => options.result = true,
                "owner" => options.owner = true,
                "operation" => {
                    input.parse::<syn::Token![=]>()?;
                    if options.operation.is_some() {
                        return Err(Error::new(option.span(), "duplicate operation binding"));
                    }
                    options.operation = Some(input.parse()?);
                }
                _ => {
                    return Err(Error::new(
                        option.span(),
                        "expected http, local, result, owner, operation = path, capabilities(...) or optional_capabilities(...)",
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

#[cfg(test)]
pub(super) fn options(item: &ItemFn) -> syn::Result<Options> {
    item.attrs
        .iter()
        .find(|attr| {
            attr.path()
                .segments
                .last()
                .is_some_and(|s| s.ident == "rpc")
        })
        .filter(|attr| matches!(attr.meta, syn::Meta::List(_)))
        .map(|attr| attr.parse_args())
        .transpose()
        .map(Option::unwrap_or_default)
}

#[cfg(test)]
pub fn expand(item: ItemFn) -> syn::Result<TokenStream> {
    let options = options(&item)?;
    expand_with_options(item, options)
}

pub(super) fn is_native(item: &ItemFn) -> bool {
    item.attrs.iter().any(|attr| {
        let path = &attr.path().segments;
        path.len() == 2 && path[0].ident == "tauri" && path[1].ident == "command"
    })
}

pub(super) fn metadata(item: &ItemFn, options: &Options, protocol: TokenStream) -> TokenStream {
    let name = item.sig.ident.to_string();
    let kind = if is_native(item) {
        quote!(Native)
    } else {
        quote!(Shared)
    };
    let mut targets = vec![quote!(#protocol::RpcTarget::Desktop)];
    if options.http {
        targets.push(quote!(#protocol::RpcTarget::Http));
    }
    if options.local {
        targets.push(quote!(#protocol::RpcTarget::Local));
    }
    let capabilities = |values: &[syn::Ident]| {
        values
            .iter()
            .map(|value| {
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
                let variant = format_ident!("{}", name, span = value.span());
                quote!(#protocol::ShellCapability::#variant)
            })
            .collect::<Vec<_>>()
    };
    let required = capabilities(&options.capabilities);
    let optional = capabilities(&options.optional_capabilities);
    quote! {
        #protocol::CommandMetadata {
            name: #name,
            kind: #protocol::CommandKind::#kind,
            targets: &[#(#targets),*],
            capabilities: &[#(#required),*],
            optional_capabilities: &[#(#optional),*],
        }
    }
}

pub(super) fn validate(item: &ItemFn, options: &Options) -> syn::Result<()> {
    let original = &item.sig;
    let native = is_native(item);
    let neutral_compatible = can_share_with_neutral(original, options.owner);
    if !native && (!neutral_compatible || original.inputs.iter().any(|argument| {
        matches!(argument, FnArg::Typed(argument) if (is_state(&argument.ty) && !is_context_type(&argument.ty, "RpcState")) || is_tauri_context(&argument.ty))
    })) {
        return Err(Error::new(original.span(), "a shared command requires a transport-neutral signature"));
    }
    if options.http && !neutral_compatible {
        return Err(Error::new(
            original.span(),
            "rpc(http) requires a transport-neutral signature",
        ));
    }
    if options.local && !neutral_compatible {
        return Err(Error::new(
            original.span(),
            "rpc(local) requires a transport-neutral signature",
        ));
    }
    Ok(())
}

pub fn expand_with_options(item: ItemFn, options: Options) -> syn::Result<TokenStream> {
    validate(&item, &options)?;
    let original = &item.sig;
    let name = &original.ident;
    let implementation_name = format_ident!("__unified_impl_{}", name);
    let neutral_handler = format_ident!("__unified_rpc_{}", name);
    let native = is_native(&item);
    let neutral_compatible = can_share_with_neutral(original, options.owner);
    let metadata = metadata(&item, &options, quote!(::nyanpasu_rpc_protocol));
    let operation = options.operation.as_ref();
    let operation_trait = quote!(::nyanpasu_rpc_protocol::operation::RpcOperation);
    let name_check = operation.map(|operation| {
        quote! {
            const _: () = {
                let actual = <#operation as #operation_trait>::NAME.as_bytes();
                let expected = stringify!(#name).as_bytes();
                assert!(actual.len() == expected.len(), "RPC operation name mismatch");
                let mut index = 0;
                while index < actual.len() {
                    assert!(actual[index] == expected[index], "RPC operation name mismatch");
                    index += 1;
                }
            };
        }
    });
    let tauri_handler = format_ident!("__unified_tauri_{}", name);

    let mut implementation = item.clone();
    implementation.attrs = item
        .attrs
        .iter()
        .filter(|attribute| {
            attribute.path().is_ident("cfg") || attribute.path().is_ident("cfg_attr")
        })
        .cloned()
        .collect();
    implementation.vis = syn::Visibility::Inherited;
    implementation.sig.ident = implementation_name.clone();

    let mut wrapper_args = Vec::new();
    let mut neutral_args = Vec::new();
    let mut dependency_bindings = Vec::new();
    let mut fields = Vec::new();
    let mut field_names = Vec::new();
    let mut tauri_fields = Vec::new();
    let mut tauri_field_names = Vec::new();
    let mut neutral_supported = neutral_compatible && (options.http || options.local || !native);
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
            } else if state_name.as_deref() == Some("HostEnvironment") {
                neutral_args.push(quote!(&dependencies.host_environment));
            } else if let Some(slot) = host_port_slot(state_name.as_deref()) {
                let slot = format_ident!("{slot}");
                dependency_bindings.push(quote! {
                    let #ident = dependencies.#slot.clone().ok_or_else(||
                        ::nyanpasu_rpc_protocol::RpcError::unsupported(stringify!(#name)))?;
                });
                neutral_args.push(quote!(#ident.as_ref()));
            } else {
                neutral_supported = false;
            }
        } else if options.owner && is_context_type(&argument.ty, "RpcCaller") {
            *argument.ty = syn::parse_quote!(::nyanpasu_rpc_protocol::RpcOwner);
            wrapper_args.push(quote!(#ident.into_owner()));
            neutral_args.push(quote!(owner.clone()));
        } else if is_context_type(&argument.ty, "AppHandle") {
            wrapper_args.push(quote!(#ident));
            neutral_supported = false;
        } else if options.owner && is_context_type(&argument.ty, "Window") {
            *argument.ty = syn::parse_quote!(::nyanpasu_rpc_protocol::RpcOwner);
            wrapper_args.push(quote!(::nyanpasu_rpc_protocol::RpcOwner::desktop(#ident.label())));
            neutral_args.push(quote!(owner.clone()));
        } else if is_context_type(&argument.ty, "Window")
            || is_context_type(&argument.ty, "Webview")
            || is_context_type(&argument.ty, "WebviewWindow")
        {
            wrapper_args.push(quote!(#ident));
            neutral_supported = false;
        } else if is_tauri_channel(&argument.ty) {
            // Decode the frontend descriptor, then bind it to the invoking webview.
            // Channel commands remain unsupported over HTTP.
            neutral_supported = false;
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
    let name_check = name_check.map(|check| {
        quote! {
            #(#cfg_attrs)*
            #check
        }
    });
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

    let params_parser = parser(&fields, &field_names, operation);
    let neutral_parse = params_parser;
    let neutral_call = call(
        &implementation_name,
        &neutral_args,
        original.asyncness.is_some(),
        None,
    );
    let neutral_handler_body = if neutral_supported
        && !matches!(original.output, ReturnType::Default)
    {
        let output = output_value(&neutral_call, &original.output, options.result);
        let response_type =
            operation.map(|operation| quote!(: <#operation as #operation_trait>::Response));
        let completion = completion_summary(&original.output, options.result);
        quote! {
            #(#dependency_bindings)*
            #neutral_parse
            Ok(::std::boxed::Box::pin(async move {
                match #output {
                    Ok(output) => {
                        let output #response_type = output;
                        let completion = #completion;
                        ::nyanpasu_rpc::registry::RpcCompletion {
                            result: ::serde_json::to_value(output)
                                .map_err(::nyanpasu_rpc::error::application_error),
                            completion,
                        }
                    }
                    Err(error) => ::nyanpasu_rpc::registry::RpcCompletion::unreported(Err(error)),
                }
            }))
        }
    } else {
        quote! { Err(::nyanpasu_rpc_protocol::RpcError::unsupported(stringify!(#name))) }
    };

    let tauri_handler_definition = if !native {
        quote!()
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
            } else if options.owner && is_context_type(&argument.ty, "Window") {
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
        let output = output_value(&tauri_call, &original.output, options.result);
        let parser = parser(&tauri_fields, &tauri_field_names, operation);
        let response_type =
            operation.map(|operation| quote!(: <#operation as #operation_trait>::Response));
        let completion = completion_summary(&original.output, options.result);
        quote! {
            #(#cfg_attrs)*
            fn #tauri_handler(
                app: ::tauri::AppHandle,
                window: ::tauri::Window,
                webview: ::tauri::Webview,
                params: ::serde_json::Value,
            ) -> ::std::result::Result<::nyanpasu_rpc::registry::RpcFuture, ::nyanpasu_rpc_protocol::RpcError> {
                #parser
                Ok(::std::boxed::Box::pin(async move {
                    #(#state_bindings)*
                    match #output {
                        Ok(output) => {
                            let output #response_type = output;
                            let completion = #completion;
                            ::nyanpasu_rpc::registry::RpcCompletion {
                                result: ::serde_json::to_value(output)
                                    .map_err(::nyanpasu_rpc::error::application_error),
                                completion,
                            }
                        }
                        Err(error) => ::nyanpasu_rpc::registry::RpcCompletion::unreported(Err(error)),
                    }
                }))
            }
        }
    } else {
        quote! {
            #(#cfg_attrs)*
            fn #tauri_handler(
                _app: ::tauri::AppHandle,
                _window: ::tauri::Window,
                _webview: ::tauri::Webview,
                _params: ::serde_json::Value,
            ) -> ::std::result::Result<::nyanpasu_rpc::registry::RpcFuture, ::nyanpasu_rpc_protocol::RpcError> {
                Err(::nyanpasu_rpc_protocol::RpcError::unsupported(stringify!(#name)))
            }
        }
    };

    let native_entry = native.then(|| {
        quote! {
            #(#cfg_attrs)*
            ::inventory::submit! {
                crate::unified_rpc::native::TauriCommandEntry {
                    name: stringify!(#name),
                    handler: #tauri_handler,
                }
            }
        }
    });

    Ok(quote! {
        #implementation
        #wrapper

        #name_check

        #(#cfg_attrs)*
        fn #neutral_handler(
            dependencies: ::std::sync::Arc<::nyanpasu_rpc::registry::RpcDependencies>,
            owner: ::nyanpasu_rpc_protocol::RpcOwner,
            params: ::serde_json::Value,
        ) -> ::std::result::Result<::nyanpasu_rpc::registry::RpcFuture, ::nyanpasu_rpc_protocol::RpcError> {
            #neutral_handler_body
        }

        #tauri_handler_definition
        #native_entry

        #(#cfg_attrs)*
        ::inventory::submit! {
            ::nyanpasu_rpc::registry::CommandEntry {
                metadata: #metadata,
                handler: #neutral_handler,
            }
        }
    })
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

fn output_value(call: &TokenStream, output: &ReturnType, result: bool) -> TokenStream {
    match output {
        ReturnType::Type(_, ty) if result || is_result(ty) => {
            quote!(#call.map_err(::nyanpasu_rpc::error::application_error))
        }
        ReturnType::Type(_, _) => quote!(Ok(#call)),
        ReturnType::Default => quote!(Ok(#call)),
    }
}

fn completion_summary(output: &ReturnType, result: bool) -> TokenStream {
    let success = match output {
        ReturnType::Type(_, ty) if result || is_result(ty) => {
            let Type::Path(path) = ty.as_ref() else {
                return quote!(::nyanpasu_rpc::audit::CompletionSummary::NotReported);
            };
            let segment = path
                .path
                .segments
                .last()
                .expect("a type path has a segment");
            match &segment.arguments {
                syn::PathArguments::AngleBracketed(args) => args.args.iter().find_map(|arg| {
                    if let syn::GenericArgument::Type(ty) = arg {
                        Some(ty)
                    } else {
                        None
                    }
                }),
                _ => None,
            }
        }
        ReturnType::Type(_, ty) => Some(ty.as_ref()),
        ReturnType::Default => None,
    };
    if success.is_some_and(|ty| is_context_type(ty, "MutationOutcome")) {
        quote!(::nyanpasu_rpc::audit::summarize_mutation(&output))
    } else {
        quote!(::nyanpasu_rpc::audit::CompletionSummary::NotReported)
    }
}

fn parser(
    fields: &[TokenStream],
    names: &[syn::Ident],
    operation: Option<&syn::Path>,
) -> TokenStream {
    let operation_trait = quote!(::nyanpasu_rpc_protocol::operation::RpcOperation);
    if fields.is_empty() {
        let unit_check = operation.map(|operation| {
            quote! {
                let (): <#operation as #operation_trait>::Request = ();
            }
        });
        quote! {
            if !params.is_null() && !matches!(&params, ::serde_json::Value::Object(map) if map.is_empty()) {
                return Err(::nyanpasu_rpc_protocol::RpcError::invalid_params("expected an empty input object"));
            }
            #unit_check
        }
    } else {
        let args = if let Some(operation) = operation {
            quote!(type Args = <#operation as #operation_trait>::Request;)
        } else {
            quote! {
                #[derive(::serde::Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct Args { #(#fields,)* }
            }
        };
        quote! {
            #args
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
            if ![
                "NyanpasuClient",
                "Storage",
                "PathResolver",
                "HostEnvironment",
                "HostIconFiles",
                "CoreDirectoryResolver",
                "ServiceInstallPrompt",
                "AccentColorQuery",
            ]
            .iter()
            .any(|name| is_state_of(&argument.ty, name))
            {
                return false;
            }
        } else if owner && is_context_type(&argument.ty, "RpcCaller") {
            continue;
        } else if is_context_type(&argument.ty, "AppHandle") {
            return false;
        } else if owner && is_context_type(&argument.ty, "Window") {
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
    if segment.ident != "State" && segment.ident != "RpcState" {
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
    state_inner_type(ty).is_some_and(|inner| type_name(&inner).as_deref() == Some(expected))
}

fn host_port_slot(name: Option<&str>) -> Option<&'static str> {
    match name? {
        "HostIconFiles" => Some("icon_files"),
        "CoreDirectoryResolver" => Some("core_directory"),
        "ServiceInstallPrompt" => Some("service_prompt"),
        "AccentColorQuery" => Some("accent_color"),
        _ => None,
    }
}

fn type_name(ty: &Type) -> Option<String> {
    match ty {
        Type::TraitObject(object) => object.bounds.iter().find_map(|bound| match bound {
            syn::TypeParamBound::Trait(bound) => bound
                .path
                .segments
                .last()
                .map(|segment| segment.ident.to_string()),
            _ => None,
        }),
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
    use std::collections::BTreeMap;

    use syn::{Item, ItemFn};

    use super::expand;

    #[test]
    fn mutation_summary_requires_an_explicit_success_type() {
        for (output, reports) in [
            (quote::quote!(MutationOutcome<()>), true),
            (quote::quote!(Result<values::MutationOutcome<String>>), true),
            (quote::quote!(CommandResult<MutationOutcome<()>>), true),
            (quote::quote!(MutationAlias), false),
            (quote::quote!(Result<MutationAlias>), false),
            (quote::quote!(Result<()>), false),
            (quote::quote!(CommitReceipt), false),
        ] {
            let item = syn::parse2(quote::quote! {
                #[nyanpasu_macro::rpc(result)]
                #[tauri::command]
                fn test() -> #output { todo!() }
            })
            .unwrap();
            // The explicit Result hint applies only to the result-shaped signatures.
            let mut options = super::options(&item).unwrap();
            options.result = output.to_string().starts_with("CommandResult");
            let expanded = super::expand_with_options(item, options)
                .unwrap()
                .to_string();
            assert_eq!(expanded.contains("summarize_mutation"), reports, "{output}");
        }
    }

    #[test]
    fn production_mutation_summary_commands_are_fixed() {
        let mut reported = crate::command_catalog::production_declarations()
            .into_iter()
            .filter(|item| {
                expand(item.clone())
                    .unwrap()
                    .to_string()
                    .contains("summarize_mutation")
            })
            .map(|item| item.sig.ident.to_string())
            .collect::<Vec<_>>();
        reported.sort();
        assert_eq!(
            reported,
            [
                "activate_profile",
                "clear_proxy_fixed",
                "create_profile",
                "create_profile_folder",
                "create_proxy_group_folder",
                "delete_profile",
                "import_profile",
                "patch_app_config",
                "patch_clash_config",
                "patch_profile_metadata",
                "patch_remote_profile_options",
                "patch_runtime_overrides",
                "reorder_profile",
                "reorder_profiles_by_list",
                "replace_profile_definition",
                "save_profile_file",
                "select_proxy",
                "set_global_transforms",
                "set_hotkeys",
                "set_profile_layout",
                "set_profile_valid_fields",
                "set_proxy_group_layout",
                "set_release_channel",
                "set_service_mode",
                "update_profile",
            ]
        );
    }

    #[test]
    fn operation_bindings_use_the_existing_generator() {
        let expanded = expand(syn::parse_quote! {
            #[nyanpasu_macro::rpc(local, http, owner, result, operation = contracts::OpenLogSession)]
            fn open_log_session(caller: RpcCaller, client: RpcState<'_, NyanpasuClient>, source: Source, request: OpenLogs) -> LogResult<LogSession> { todo!() }
        }).unwrap();
        syn::parse2::<syn::File>(expanded.clone()).unwrap();
        let expanded = expanded.to_string();
        assert!(expanded.contains("type Args = < contracts :: OpenLogSession as"));
        assert!(expanded.contains("let Args { source , request , }"));
        assert!(!expanded.contains("struct Args"));
        assert!(expanded.contains("owner . clone () , & dependencies . client , source , request"));
        assert!(expanded.contains(":: Response ="));
        assert!(expanded.contains(":: NAME . as_bytes ()"));
        assert_eq!(expanded.matches("CommandEntry {").count(), 1);
        assert_eq!(expanded.matches("inventory :: submit !").count(), 1);
        assert!(!expanded.contains("TauriCommandEntry"));
    }

    #[test]
    fn zero_argument_contracts_check_unit_without_changing_the_parser() {
        let expanded = expand(syn::parse_quote! {
            #[nyanpasu_macro::rpc(local, operation = contracts::GetProfiles)]
            fn get_profiles(client: RpcState<'_, NyanpasuClient>) -> Result<Profiles> { todo!() }
        })
        .unwrap()
        .to_string();
        assert!(expanded.contains("params . is_null ()"));
        assert!(expanded.contains("map . is_empty ()"));
        assert!(expanded.contains("let () : < contracts :: GetProfiles as"));
        assert!(!expanded.contains("from_value (params)"));
    }

    #[test]
    fn explicit_result_alias_is_unwrapped_and_owner_is_injected() {
        let command = syn::parse_quote! {
            #[nyanpasu_macro::rpc(http, result, owner)]
            #[tauri::command]
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
            #[tauri::command]
            pub fn is_tray_icon_set(paths: State<'_, PathResolver>, mode: TrayIcon) -> Result<bool> { todo!() }
        };
        let expanded = expand(command).unwrap().to_string();

        assert!(expanded.contains("& dependencies . paths"));
        assert!(!expanded.contains("RpcError :: unsupported"));
    }

    #[test]
    fn channel_parameters_are_rebuilt_for_desktop_but_unsupported_over_http() {
        let command = syn::parse_quote! {
            #[nyanpasu_macro::rpc(owner)]
            #[tauri::command]
            pub fn stream(window: tauri::Window, on_chunk: tauri::ipc::Channel<String>) -> Result<()> { todo!() }
        };
        let expanded = expand(command).unwrap().to_string();

        assert!(expanded.contains("on_chunk : :: tauri :: ipc :: JavaScriptChannelId"));
        assert!(expanded.contains("on_chunk . channel_on (webview . clone ())"));
        assert!(expanded.contains("RpcOwner :: desktop (window . label ())"));
        assert!(!expanded.contains("struct Args { window"));
        assert!(expanded.contains("RpcError :: unsupported (stringify ! (stream))"));
    }

    #[test]
    fn channel_parameter_json_parser_uses_camel_case_fields() {
        let command = syn::parse_quote! {
            #[nyanpasu_macro::rpc]
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

    #[test]
    fn http_requires_opt_in_and_accepts_only_lifetime_generics() {
        for (source, supported) in [
            (
                "#[tauri::command] pub fn open_that(path: String) -> Result<()> { todo!() }",
                false,
            ),
            (
                "#[tauri::command] #[nyanpasu_macro::rpc(http)] pub fn collect_envs<'a>() -> Result<EnvInfo<'a>> { todo!() }",
                true,
            ),
            (
                "#[tauri::command] #[nyanpasu_macro::rpc(http)] pub fn generic<T>() -> Result<T> { todo!() }",
                false,
            ),
            (
                "#[tauri::command] #[nyanpasu_macro::rpc(http)] pub fn generic<const N: usize>() -> Result<usize> { todo!() }",
                false,
            ),
            (
                "#[tauri::command] #[nyanpasu_macro::rpc(http)] pub fn window(window: tauri::Window) -> Result<()> { todo!() }",
                false,
            ),
            (
                "#[tauri::command] #[nyanpasu_macro::rpc(http)] pub fn desktop(app: tauri::AppHandle) -> Result<()> { todo!() }",
                false,
            ),
        ] {
            let command: ItemFn = syn::parse_str(source).unwrap();
            if !supported && source.contains("rpc(http)") {
                assert_eq!(
                    expand(command).unwrap_err().to_string(),
                    "rpc(http) requires a transport-neutral signature",
                    "{source}"
                );
                continue;
            }
            let expanded = expand(command).unwrap().to_string();
            assert_eq!(
                !expanded.contains("RpcError :: unsupported"),
                supported,
                "{source}"
            );
        }
    }

    #[test]
    fn http_declaration_rejects_native_only_arguments() {
        for (arguments, options) in [
            (
                quote::quote!(on_chunk: tauri::ipc::Channel<String>),
                quote::quote!(http),
            ),
            (
                quote::quote!(window: tauri::Window, on_chunk: tauri::ipc::Channel<String>),
                quote::quote!(http, owner),
            ),
            (quote::quote!(app: tauri::AppHandle), quote::quote!(http)),
            (quote::quote!(window: tauri::Window), quote::quote!(http)),
            (quote::quote!(webview: tauri::Webview), quote::quote!(http)),
            (
                quote::quote!(window: tauri::WebviewWindow),
                quote::quote!(http),
            ),
            (
                quote::quote!(state: tauri::State<'_, NativeState>),
                quote::quote!(http),
            ),
            (quote::quote!(value: &str), quote::quote!(http)),
        ] {
            let command = syn::parse_quote! {
                #[nyanpasu_macro::rpc(#options)]
                #[tauri::command]
                fn native(#arguments) -> Result<()> { todo!() }
            };
            let error = expand(command).unwrap_err();
            assert_eq!(
                error.to_string(),
                "rpc(http) requires a transport-neutral signature",
                "{arguments}"
            );
        }
    }

    #[test]
    fn desktop_update_core_dispatch_remains_available() {
        let commands = crate::command_catalog::production_declarations();
        let command = commands
            .into_iter()
            .find(|command| command.sig.ident == "update_core")
            .expect("update_core must remain an RPC command");
        let expanded = expand(command).expect("failed to expand update_core");
        assert!(
            expanded
                .to_string()
                .contains("handler : __unified_tauri_update_core"),
            "update_core must register its generated desktop dispatcher"
        );
        let expanded = syn::parse2::<syn::File>(expanded).unwrap();
        let handler = |name: &str| {
            expanded
                .items
                .iter()
                .find_map(|item| match item {
                    Item::Fn(function) if function.sig.ident == name => Some(function),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("missing generated handler {name}"))
                .block
                .to_token_stream()
                .to_string()
        };
        let desktop = handler("__unified_tauri_update_core");
        assert!(desktop.contains("__unified_impl_update_core"));
        assert!(!desktop.contains("RpcError :: unsupported"));
        let http = handler("__unified_rpc_update_core");
        assert!(http.contains("RpcError :: unsupported (stringify ! (update_core))"));
        assert!(!http.contains("__unified_impl_update_core"));
    }

    #[test]
    fn dispatchers_cover_each_command_category() {
        let mut commands = crate::command_catalog::production_declarations();
        for (name, targets) in [
            ("shared_desktop_fixture", quote::quote!()),
            ("shared_http_fixture", quote::quote!(http)),
            ("shared_local_fixture", quote::quote!(local)),
            ("shared_dual_fixture", quote::quote!(http, local)),
        ] {
            let name = format_ident!("{name}");
            commands.push(syn::parse_quote! {
                #[nyanpasu_macro::rpc(owner, #targets)]
                fn #name(client: RpcState<'_, NyanpasuClient>, owner: RpcCaller, value: String) -> String { value }
            });
        }
        for command in commands {
            let native = super::is_native(&command);
            let needs_host_port = command.sig.inputs.iter().any(|arg| {
                let syn::FnArg::Typed(arg) = arg else {
                    return false;
                };
                super::state_inner_type(&arg.ty).is_some_and(|ty| {
                    super::host_port_slot(super::type_name(&ty).as_deref()).is_some()
                })
            });
            let options = super::options(&command).expect("command options must parse");
            let name = command.sig.ident.to_string();
            let expanded =
                expand(command).unwrap_or_else(|error| panic!("failed to expand {name}: {error}"));
            let expanded = syn::parse2::<syn::File>(expanded)
                .unwrap_or_else(|error| panic!("failed to parse expansion for {name}: {error}"));
            let registration = expanded.to_token_stream().to_string();
            assert!(registration.contains(&format!("handler : __unified_rpc_{name}")));
            assert_eq!(
                registration.contains(&format!("handler : __unified_tauri_{name}")),
                native
            );
            if !native {
                assert!(!registration.contains(":: tauri"));
                assert_eq!(
                    registration.contains("RpcTarget :: Http"),
                    options.http,
                    "{name}"
                );
                assert_eq!(
                    registration.contains("RpcTarget :: Local"),
                    options.local,
                    "{name}"
                );
                assert!(needs_host_port || !registration.contains("RpcError :: unsupported"));
            }
            assert!(
                !registration.contains("dispatch_audited"),
                "handlers must not audit twice"
            );
            assert!(
                !registration.contains("RpcAudit"),
                "the registry owns auditing"
            );
            let handler_name = if native {
                format_ident!("__unified_tauri_{name}")
            } else {
                format_ident!("__unified_rpc_{name}")
            };
            let handler = expanded
                .items
                .iter()
                .find_map(|item| match item {
                    Item::Fn(function) if function.sig.ident == handler_name => Some(function),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("missing Tauri handler for {name}"));
            assert!(
                handler
                    .block
                    .to_token_stream()
                    .to_string()
                    .contains(&format!("__unified_impl_{name}")),
                "{name} must call its implementation even when a host port can be absent"
            );
            assert!(
                needs_host_port
                    || !handler
                        .block
                        .to_token_stream()
                        .to_string()
                        .contains("RpcError :: unsupported"),
                "Tauri command {name} was compiled into an unsupported dispatcher"
            );
        }
    }

    #[test]
    fn every_ipc_tauri_command_uses_unified_rpc() {
        let commands = crate::command_catalog::production_declarations();
        assert!(
            commands.iter().any(super::is_native),
            "expected native commands"
        );

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
        let source = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tauri/src/unified_rpc/native.rs"
        ))
        .unwrap();
        let file = syn::parse_file(&source).unwrap();
        let transport = file
            .items
            .iter()
            .find_map(|item| match item {
                Item::Fn(function) if function.sig.ident == "call_rpc" => Some(function),
                _ => None,
            })
            .unwrap();
        let body = transport.block.to_token_stream().to_string();
        assert!(body.contains("DesktopRpc"));
        assert!(body.contains("rpc . call"));
        assert!(
            !body.contains("handler"),
            "call_rpc must enter the registry dispatcher"
        );
    }

    #[test]
    fn transport_builder_registers_only_call_rpc() {
        let source = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tauri/src/specta_export.rs"
        ))
        .unwrap();
        let file = syn::parse_file(&source).unwrap();
        let builder = file
            .items
            .iter()
            .find_map(|item| match item {
                Item::Fn(function) if function.sig.ident == "build_transport_builder" => {
                    Some(function)
                }
                _ => None,
            })
            .unwrap();
        let body = builder.block.to_token_stream().to_string();
        assert_eq!(body.matches(". commands (").count(), 1);
        assert_eq!(body.matches("collect_commands !").count(), 1);
        assert!(
            body.contains(". commands (collect_commands ! [unified_rpc :: native :: call_rpc])"),
            "only call_rpc may be registered on the runtime transport: {body}"
        );
        let source =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../tauri/src/lib.rs"))
                .unwrap();
        let body = syn::parse_file(&source)
            .unwrap()
            .to_token_stream()
            .to_string();
        // Two matches: the builder call and the nested `transport_builder.invoke_handler()`.
        assert_eq!(body.matches(". invoke_handler (").count(), 2);
        assert!(body.contains(". invoke_handler (transport_builder . invoke_handler ())"));
        assert!(
            body.contains("let transport_builder = specta_export :: build_transport_builder ()")
        );
        assert!(!body.contains("generate_handler !"));
    }

    #[test]
    fn neutral_expansion_contains_no_tauri_types() {
        let command = syn::parse_quote! {
            #[nyanpasu_macro::rpc(http, local, owner, result)]
            #[specta::specta]
            pub async fn shared(
                client: RpcState<'_, NyanpasuClient>, storage: RpcState<'_, Storage>, paths: RpcState<'_, PathResolver>,
                caller: RpcCaller, page_size: usize,
            ) -> SharedResult<String> { todo!() }
        };
        let expanded = expand(command).unwrap().to_string();
        for forbidden in [
            "tauri",
            "AppHandle",
            "JavaScriptChannelId",
            "TauriCommandEntry",
        ] {
            assert!(
                !expanded.contains(forbidden),
                "neutral expansion contains {forbidden}"
            );
        }
        assert!(expanded.contains("specta :: specta"));
        assert!(expanded.contains("& dependencies . client"));
        assert!(expanded.contains("& dependencies . storage"));
        assert!(expanded.contains("& dependencies . paths"));
        assert!(expanded.contains("owner . clone ()"));
        assert!(!expanded.contains("caller : RpcCaller , page_size : usize , }"));
        assert!(expanded.contains("rename_all = \"camelCase\""));
        assert!(expanded.contains("map_err (:: nyanpasu_rpc :: error :: application_error)"));
        assert!(!expanded.contains("deny_unknown_fields"));
        let empty = expand(syn::parse_quote! {
            #[nyanpasu_macro::rpc(http)] fn empty() -> bool { true }
        })
        .unwrap()
        .to_string();
        assert!(empty.contains("params . is_null ()"));
        assert!(empty.contains("map . is_empty ()"));
    }

    #[test]
    fn target_and_capability_metadata_is_generated_once() {
        assert_production_capability_inventory();
        let expanded = expand(syn::parse_quote! {
            #[nyanpasu_macro::rpc(http, local, capabilities(clipboard), optional_capabilities(elevation))]
            #[tauri::command]
            fn copy(value: String) -> String { value }
        }).unwrap().to_string();
        assert_eq!(expanded.matches("CommandMetadata {").count(), 1);
        for value in ["Desktop", "Http", "Local"] {
            assert_eq!(
                expanded.matches(&format!("RpcTarget :: {value}")).count(),
                1
            );
        }
        for value in ["Clipboard", "Elevation"] {
            assert_eq!(
                expanded
                    .matches(&format!("ShellCapability :: {value}"))
                    .count(),
                1
            );
        }
        assert!(expanded.contains("TauriCommandEntry"));
        assert!(expanded.contains("CommandKind :: Native"));
    }

    fn production_capability_inventory(commands: &[ItemFn]) -> String {
        let mut inventories = Vec::new();
        for platform in ["windows", "linux", "macos"] {
            let mut rows = BTreeMap::new();
            for command in commands {
                let enabled = command
                    .attrs
                    .iter()
                    .filter(|attr| attr.path().is_ident("cfg") || attr.path().is_ident("cfg_attr"))
                    .all(|attr| match attr.to_token_stream().to_string().as_str() {
                        "# [cfg (windows)]" => platform == "windows",
                        "# [cfg (not (windows))]" => platform != "windows",
                        guard => panic!("extend the platform inventory for {guard}"),
                    });
                if !enabled {
                    continue;
                }
                let options = super::options(command).unwrap();
                super::validate(command, &options).expect("RPC declaration must be valid");
                let mut targets = vec!["desktop"];
                if options.http {
                    targets.push("http");
                }
                if options.local {
                    targets.push("local");
                }
                let names = |values: &[syn::Ident]| {
                    if values.is_empty() {
                        "-".to_owned()
                    } else {
                        values
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(",")
                    }
                };
                let name = command.sig.ident.to_string();
                let row = format!(
                    "{name} | {} | {} | {}",
                    targets.join(","),
                    names(&options.capabilities),
                    names(&options.optional_capabilities)
                );
                assert!(
                    rows.insert(name, row).is_none(),
                    "duplicate {platform} command"
                );
            }
            assert_eq!(rows.len(), 159, "{platform} command inventory");
            inventories.push((platform, rows));
        }
        render_capability_inventory(&inventories)
    }

    fn render_capability_inventory(inventories: &[(&str, BTreeMap<String, String>)]) -> String {
        // Linux supplies the baseline; every other platform is compared in full.
        let baseline = &inventories
            .iter()
            .find(|(platform, _)| *platform == "linux")
            .unwrap()
            .1;
        let mut snapshot = String::from(
            "# name | targets | required | optional\n# [baseline] uses Linux rows; platform differences: + row adds/replaces, - name removes.\n\n[baseline]\n",
        );
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

    #[test]
    #[ignore = "explicit generator; freshness checks never write"]
    fn export_capability_inventory() {
        std::fs::write(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/capability-inventory.txt"
            ),
            production_capability_inventory(&crate::command_catalog::production_declarations()),
        )
        .expect("capability inventory must be writable");
    }

    #[test]
    fn capability_inventory_records_platform_additions_changes_and_removals() {
        let rows = |values: &[&str]| -> BTreeMap<String, String> {
            values
                .iter()
                .map(|row| {
                    (
                        row.split(" | ").next().unwrap().to_string(),
                        row.to_string(),
                    )
                })
                .collect()
        };
        let snapshot = render_capability_inventory(&[
            (
                "windows",
                rows(&["added | desktop | - | -", "changed | desktop | window | -"]),
            ),
            (
                "linux",
                rows(&["changed | desktop | - | -", "removed | desktop | - | -"]),
            ),
            (
                "macos",
                rows(&["changed | desktop | - | -", "removed | desktop | - | -"]),
            ),
        ]);
        assert_eq!(
            snapshot,
            "# name | targets | required | optional\n# [baseline] uses Linux rows; platform differences: + row adds/replaces, - name removes.\n\n[baseline]\nchanged | desktop | - | -\nremoved | desktop | - | -\n\n[windows]\n- removed\n+ added | desktop | - | -\n+ changed | desktop | window | -\n\n[linux]\n\n[macos]\n"
        );
    }

    fn assert_production_capability_inventory() {
        let commands = crate::command_catalog::production_declarations();
        assert_eq!(
            production_capability_inventory(&commands),
            include_str!("../tests/fixtures/capability-inventory.txt").replace("\r\n", "\n"),
            "run export_capability_inventory explicitly after declaration changes"
        );

        let metadata = |name: &str| {
            super::options(
                commands
                    .iter()
                    .find(|command| command.sig.ident == name)
                    .unwrap(),
            )
            .unwrap()
        };
        // Profile commits and lifecycle completion refresh the desktop tray indirectly.
        for name in ["reorder_profile", "install_service", "select_proxy"] {
            let options = metadata(name);
            assert!(
                options
                    .optional_capabilities
                    .iter()
                    .any(|capability| capability == "presentation"),
                "{name} reaches an optional tray refresh"
            );
            assert!(
                !options
                    .capabilities
                    .iter()
                    .any(|capability| capability == "presentation"),
                "{name} must remain available without presentation"
            );
        }
        // Direct privileged operations require a facility. Conditional runtime
        // switches, compensation and binary-copy fallback only describe reachability.
        for name in [
            "flush_system_dns_cache",
            "install_service",
            "uninstall_service",
            "start_service",
            "stop_service",
            "restart_service",
        ] {
            assert!(
                metadata(name)
                    .capabilities
                    .iter()
                    .any(|capability| capability == "elevation"),
                "{name}"
            );
        }
        for name in [
            "patch_app_config",
            "patch_clash_config",
            "patch_runtime_overrides",
            "retry_configuration_runtime",
            "change_clash_core",
            "restart_sidecar",
            "enhance_profiles",
            "import_profile",
            "create_profile",
            "update_profile",
            "delete_profile",
            "activate_profile",
            "set_global_transforms",
            "set_profile_valid_fields",
            "patch_remote_profile_options",
            "replace_profile_definition",
            "save_profile_file",
            "update_core",
        ] {
            let options = metadata(name);
            assert!(
                !options
                    .capabilities
                    .iter()
                    .any(|capability| capability == "elevation"),
                "{name}"
            );
            assert!(
                options
                    .optional_capabilities
                    .iter()
                    .any(|capability| capability == "elevation"),
                "{name}"
            );
        }
        for name in [
            "open_log_session",
            "query_logs",
            "close_log_session",
            "report_frontend_events",
        ] {
            let options = metadata(name);
            assert!(options.owner, "{name} needs neutral caller identity");
            assert!(
                options.capabilities.is_empty(),
                "{name} does not need a native webview"
            );
        }
        for name in [
            "is_tray_icon_set",
            "is_widget_icon_set",
            "get_release_channel",
            "set_release_channel",
            "get_app_update_changelog",
            "get_service_install_prompt",
            "get_core_dir",
        ] {
            assert!(
                metadata(name).capabilities.is_empty(),
                "{name} only reads/formats values or edits configuration"
            );
        }
    }

    #[test]
    fn local_optional_elevation_expands_and_registers() {
        let expanded = expand(syn::parse_quote! {
            #[nyanpasu_macro::rpc(local, optional_capabilities(elevation))]
            fn profile_mutation(value: String) -> String { value }
        })
        .unwrap()
        .to_string();
        assert!(expanded.contains("RpcTarget :: Local"));
        assert!(expanded.contains(
            "optional_capabilities : & [:: nyanpasu_rpc_protocol :: ShellCapability :: Elevation]"
        ));
        assert!(expanded.contains("handler : __unified_rpc_profile_mutation"));
        assert!(!expanded.contains("RpcError :: unsupported"));
    }

    use quote::{ToTokens, format_ident};
}
