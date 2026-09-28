use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{
    Error, FnArg, ItemFn, LitStr, Meta, Pat, ReturnType, Token, Type,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
    spanned::Spanned,
};

pub struct RpcArgs {
    name: LitStr,
    input_name: Option<LitStr>,
    output_name: Option<LitStr>,
}

impl Parse for RpcArgs {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let args = Punctuated::<Meta, Token![,]>::parse_terminated(input)?;
        let mut name = None;
        let mut input_name = None;
        let mut output_name = None;
        for arg in args {
            match arg {
                Meta::NameValue(value) if value.path.is_ident("name") => {
                    set_once(
                        &mut name,
                        parse_string(&value.value)?,
                        value.path.span(),
                        "name",
                    )?;
                }
                Meta::NameValue(value) if value.path.is_ident("input_name") => {
                    set_once(
                        &mut input_name,
                        parse_string(&value.value)?,
                        value.path.span(),
                        "input_name",
                    )?;
                }
                Meta::NameValue(value) if value.path.is_ident("output_name") => {
                    set_once(
                        &mut output_name,
                        parse_string(&value.value)?,
                        value.path.span(),
                        "output_name",
                    )?;
                }
                other => {
                    return Err(Error::new(
                        other.span(),
                        "expected name = \"domain.method\" with optional input_name / output_name",
                    ));
                }
            }
        }
        Ok(Self {
            name: name.ok_or_else(|| {
                Error::new(input.span(), "#[rpc] requires name = \"domain.method\"")
            })?,
            input_name,
            output_name,
        })
    }
}

fn parse_string(expr: &syn::Expr) -> syn::Result<LitStr> {
    match expr {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(value),
            ..
        }) => Ok(value.clone()),
        _ => Err(Error::new(expr.span(), "expected a string literal")),
    }
}

fn set_once<T>(
    slot: &mut Option<T>,
    value: T,
    span: proc_macro2::Span,
    key: &str,
) -> syn::Result<()> {
    if slot.is_some() {
        return Err(Error::new(span, format!("duplicate #[rpc] option `{key}`")));
    }
    *slot = Some(value);
    Ok(())
}

pub fn expand(args: RpcArgs, item: ItemFn) -> syn::Result<TokenStream> {
    let sig = &item.sig;
    if sig.asyncness.is_none() {
        return Err(Error::new(
            sig.span(),
            "#[rpc] handlers must be async functions",
        ));
    }
    if !sig.generics.params.is_empty() {
        return Err(Error::new(
            sig.generics.span(),
            "generic #[rpc] handlers are not supported",
        ));
    }

    let mut inputs = sig.inputs.iter();
    let context = inputs
        .next()
        .ok_or_else(|| Error::new(sig.span(), "handler must take Arc<ApiContext> first"))?;
    let context_ty = match context {
        FnArg::Typed(argument) => argument.ty.as_ref(),
        FnArg::Receiver(receiver) => {
            return Err(Error::new(
                receiver.span(),
                "#[rpc] handlers cannot have a self receiver",
            ));
        }
    };
    if !is_arc(context_ty) {
        return Err(Error::new(
            context_ty.span(),
            "first argument must be Arc<ApiContext>",
        ));
    }

    let input = inputs.next();
    if inputs.next().is_some() {
        return Err(Error::new(
            sig.inputs.span(),
            "#[rpc] supports at most one typed input DTO",
        ));
    }
    let (input_ty, input_binding) = match input {
        None => (syn::parse_quote!(()), None),
        Some(FnArg::Typed(argument)) => {
            let binding = match argument.pat.as_ref() {
                Pat::Ident(binding) => binding.ident.clone(),
                pattern => {
                    return Err(Error::new(
                        pattern.span(),
                        "input parameter must use a simple identifier",
                    ));
                }
            };
            (argument.ty.as_ref().clone(), Some(binding))
        }
        Some(FnArg::Receiver(receiver)) => {
            return Err(Error::new(
                receiver.span(),
                "#[rpc] handlers cannot have a self receiver",
            ));
        }
    };

    let output_ty = match &sig.output {
        ReturnType::Type(_, output) => result_ok_type(output)?,
        ReturnType::Default => {
            return Err(Error::new(
                sig.span(),
                "handler must return Result<OutputDto, ApiError>",
            ));
        }
    };

    let function_name = &sig.ident;
    let call_name = format_ident!("__rpc_call_{}", function_name);
    let register_name = format_ident!("__rpc_register_types_{}", function_name);
    let name = &args.name;
    let input_type_name = args
        .input_name
        .map(|name| quote!(#name))
        .unwrap_or_else(|| quote!(stringify!(#input_ty)));
    let output_type_name = args
        .output_name
        .map(|name| quote!(#name))
        .unwrap_or_else(|| quote!(stringify!(#output_ty)));
    let typed_call = match input_binding {
        Some(binding) => quote! {
            let #binding: #input_ty = ::serde_json::from_value(params)
                .map_err(|error| crate::application_api::ApiError::invalid_params(error.to_string()))?;
            let output: #output_ty = #function_name(context, #binding).await?;
        },
        None => quote! {
            match params {
                ::serde_json::Value::Null => {},
                ::serde_json::Value::Object(object) if object.is_empty() => {},
                _ => return Err(crate::application_api::ApiError::invalid_params("expected an empty input object")),
            }
            let output: #output_ty = #function_name(context).await?;
        },
    };

    Ok(quote! {
        #item

        fn #call_name(
            context: ::std::sync::Arc<crate::application_api::ApiContext>,
            params: ::serde_json::Value,
        ) -> ::std::pin::Pin<Box<dyn ::std::future::Future<
            Output = ::std::result::Result<::serde_json::Value, crate::application_api::ApiError>
        > + ::std::marker::Send + 'static>> {
            ::std::boxed::Box::pin(async move {
                #typed_call
                ::serde_json::to_value(output)
                    .map_err(|error| crate::application_api::ApiError::application(error.to_string()))
            })
        }

        fn #register_name(types: &mut ::specta::Types) {
            types.register_mut::<#input_ty>();
            types.register_mut::<#output_ty>();
        }

        ::inventory::submit! {
            crate::application_api::Procedure {
                fn_name: #name,
                input_type: #input_type_name,
                output_type: #output_type_name,
                kind: crate::application_api::ProcedureKind::Unary,
                call: Some(#call_name),
                event_stream: None,
                register_types: #register_name,
            }
        }
    })
}

fn is_arc(ty: &Type) -> bool {
    matches!(ty, Type::Path(path) if path.path.segments.last().is_some_and(|segment| segment.ident == "Arc"))
}

fn result_ok_type(ty: &Type) -> syn::Result<Type> {
    let Type::Path(path) = ty else {
        return Err(Error::new(
            ty.span(),
            "handler must return Result<OutputDto, ApiError>",
        ));
    };
    let Some(segment) = path.path.segments.last() else {
        return Err(Error::new(
            ty.span(),
            "handler must return Result<OutputDto, ApiError>",
        ));
    };
    if segment.ident != "Result" {
        return Err(Error::new(
            ty.span(),
            "handler must return Result<OutputDto, ApiError>",
        ));
    }
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return Err(Error::new(
            ty.span(),
            "handler must return Result<OutputDto, ApiError>",
        ));
    };
    if args.args.len() != 2 {
        return Err(Error::new(
            ty.span(),
            "handler must return Result<OutputDto, ApiError>",
        ));
    }
    let Some(syn::GenericArgument::Type(output)) = args.args.first() else {
        return Err(Error::new(
            ty.span(),
            "handler must return Result<OutputDto, ApiError>",
        ));
    };
    let Some(syn::GenericArgument::Type(error)) = args.args.iter().nth(1) else {
        return Err(Error::new(
            ty.span(),
            "handler must return Result<OutputDto, ApiError>",
        ));
    };
    if !matches!(error, Type::Path(path) if path.path.segments.last().is_some_and(|segment| segment.ident == "ApiError"))
    {
        return Err(Error::new(
            error.span(),
            "#[rpc] handlers must use ApiError as their error type",
        ));
    }
    Ok(output.clone())
}
