use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Error, Ident, LitStr, Meta, Result, spanned::Spanned};

pub fn verge_patch(input: DeriveInput) -> Result<TokenStream> {
    let name = &input.ident;
    let mut patch_fn: Option<Ident> = None;
    let mut patch_pointer: Option<Ident> = None; // default is self
    let mut patch_type: Option<Ident> = None; // default is Self
    for attr in &input.attrs {
        if attr.path().is_ident("verge") {
            match &attr.meta {
                Meta::List(list) => {
                    list.parse_nested_meta(|meta| {
                        match &meta.path {
                            path if path.is_ident("patch_fn") => {
                                let value = meta.value()?;
                                let lit_str: LitStr = value.parse()?;
                                patch_fn = Some(lit_str.parse()?);
                            }
                            path if path.is_ident("patch_pointer") => {
                                let value = meta.value()?;
                                let lit_str: LitStr = value.parse()?;
                                patch_pointer = Some(lit_str.parse()?);
                            }
                            path if path.is_ident("patch_type") => {
                                let value = meta.value()?;
                                let lit_str: LitStr = value.parse()?;
                                patch_type = Some(lit_str.parse()?);
                            }
                            _ => {
                                return Err(meta.error("Unknown attribute"));
                            }
                        }
                        Ok(())
                    })?;
                }
                _ => {
                    return Err(Error::new(attr.span(), "Only #[verge(...)] is supported"));
                }
            }
        }
    }

    let patch_fn = match patch_fn {
        Some(fn_name) => fn_name,
        None => format_ident!("patch_{}", name),
    };
    let patch_pointer = match patch_pointer {
        Some(pointer) => pointer,
        None => format_ident!("self"),
    };
    let patch_type = match patch_type {
        Some(ty) => ty,
        None => format_ident!("{}", name),
    };

    let mut patch_fields = quote! {};

    match input.data {
        Data::Struct(ref data) => {
            if let syn::Fields::Named(ref fields) = data.fields {
                for field in &fields.named {
                    let field_name = field.ident.as_ref().unwrap();
                    patch_fields.extend(quote! {
                        if patch.#field_name.is_some() {
                            #patch_pointer.#field_name = patch.#field_name;
                        }
                    });
                }
            }
        }
        _ => {
            return Err(Error::new(input.span(), "Only struct is supported"));
        }
    }

    let expanded = quote! {
        impl #name {
            pub fn #patch_fn(&mut self, patch: #patch_type) {
                #patch_fields
            }
        }
    };

    Ok(expanded)
}
