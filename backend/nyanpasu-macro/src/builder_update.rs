use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{spanned::Spanned, DeriveInput, Error, Ident, LitStr, Meta};

pub fn builder_update(input: DeriveInput) -> syn::Result<TokenStream> {
    let name = format_ident!("{}", input.ident);
    // search #[builder_update(ty = "T")]
    let mut partial_ty: Option<Ident> = None;
    // search #[builder_update(patch_fn = "fn_name")]
    let mut patch_fn: Option<Ident> = None;
    for attr in &input.attrs {
        if let Some(attr_meta_name) = attr.path().get_ident() {
            if attr_meta_name == "builder_update" {
                let meta = &attr.meta;
                match meta {
                    Meta::List(list) => {
                        list.parse_nested_meta(|meta| {
                            let path = &meta.path;
                            match path {
                                path if path.is_ident("ty") => {
                                    let value = meta.value()?;
                                    let lit_str: LitStr = value.parse()?;
                                    partial_ty = Some(lit_str.parse()?);
                                }
                                path if path.is_ident("patch_fn") => {
                                    let value = meta.value()?;
                                    let lit_str: LitStr = value.parse()?;
                                    patch_fn = Some(lit_str.parse()?);
                                }
                                _ => {
                                    return Err(meta
                                        .error("Only #[builder_update(ty = \"T\")] is supported"))
                                }
                            }
                            Ok(())
                        })?;
                    }
                    _ => {
                        return Err(Error::new(
                            attr.span(),
                            "Only #[builder_update(ty = \"T\")] is supported",
                        ));
                    }
                }
            }
        }
    }
    let partial_ty = match partial_ty {
        Some(ty) => ty,
        None => format_ident!("{}Builder", name),
    };
    let patch_fn = match patch_fn {
        Some(fn_name) => fn_name,
        None => format_ident!("update"),
    };

    let mut patch_fields = quote! {};

    match input.data {
        syn::Data::Struct(ref data) => {
            if let syn::Fields::Named(ref fields) = data.fields {
                for field in &fields.named {
                    let field_name = field.ident.as_ref().unwrap();
                    // check whether the field has #[update(nest)]
                    let mut nested = false;
                    for attr in &field.attrs {
                        if attr.path().is_ident("builder_update") {
                            if let Meta::List(ref list) = attr.meta {
                                list.parse_nested_meta(|meta| {
                                    let path = &meta.path;
                                    match path {
                                        path if path.is_ident("nested") => {
                                            nested = true;
                                        }
                                        _ => {
                                            return Err(meta.error(
                                                "Only #[builder_update(nested)] is supported",
                                            ));
                                        }
                                    }
                                    Ok(())
                                })?;
                            }
                        }
                    }

                    patch_fields.extend(if nested {
                        quote! {
                            self.#field_name.#patch_fn(partial.#field_name);
                        }
                    } else {
                        quote! {
                            if let Some(value) = partial.#field_name {
                                self.#field_name = value;
                            }
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
            pub fn #patch_fn(&mut self, partial: #partial_ty) {
                #patch_fields
            }
        }
    };

    Ok(expanded)
}
