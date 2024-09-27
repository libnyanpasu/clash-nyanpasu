use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{spanned::Spanned, DeriveInput, Error, Ident, LitStr, Meta};

pub fn builder_update(input: DeriveInput) -> syn::Result<TokenStream> {
    let name = format_ident!("{}", input.ident);

    // search #[update(ty = T)]
    let mut partial_ty: Option<Ident> = None;
    for attr in &input.attrs {
        if let Some(attr_meta_name) = attr.path().get_ident() {
            if attr_meta_name == "update" {
                let meta = &attr.meta;
                match meta {
                    Meta::List(list) => {
                        list.parse_nested_meta(|meta| {
                            if meta.path.is_ident("ty") {
                                let value = meta.value()?;
                                let lit_str: LitStr = value.parse()?;
                                partial_ty = Some(lit_str.parse()?);
                            }

                            Err(meta.error("Only #[update(ty = \"T\")] is supported"))
                        })?;
                    }
                    _ => {
                        return Err(Error::new(
                            attr.span(),
                            "Only #[update(ty = \"T\")] is supported",
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

    let mut patch_fields = quote! {};

    match input.data {
        syn::Data::Struct(ref data) => {
            if let syn::Fields::Named(ref fields) = data.fields {
                for field in &fields.named {
                    let field_name = field.ident.as_ref().unwrap();
                    // check whether the field has #[update(nest)]
                    let mut nested = false;
                    for attr in &field.attrs {
                        if attr.path().is_ident("update") {
                            if let Meta::List(ref list) = attr.meta {
                                list.parse_nested_meta(|meta| {
                                    if meta.path.is_ident("nest") {
                                        nested = true;
                                    }
                                    Err(meta.error("Only #[update(nest)] is supported"))
                                })?;
                            }
                        }
                    }

                    patch_fields = if nested {
                        quote! {
                            #patch_fields
                            self.#field_name.update(partial.#field_name);
                        }
                    } else {
                        quote! {
                            #patch_fields
                            if let Some(value) = partial.#field_name {
                                self.#field_name = value;
                            }
                        }
                    };
                }
            }
        }
        _ => {
            return Err(Error::new(input.span(), "Only struct is supported"));
        }
    }

    let expanded = quote! {
        impl #name {
            pub fn update(&mut self, partial: #partial_ty) {
                #patch_fields
            }
        }
    };

    Ok(expanded)
}
