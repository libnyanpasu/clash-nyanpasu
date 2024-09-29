use proc_macro2::TokenStream;
use quote::quote;
use syn::{DeriveInput, Fields};

pub fn enum_wrapper_from(input: DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;
    let data = &input.data;

    let mut expanded = quote! {};

    match data {
        syn::Data::Enum(e) => {
            for variant in e.variants.iter() {
                let variant_name = &variant.ident;
                match &variant.fields {
                    Fields::Unnamed(fields) => {
                        if fields.unnamed.len() != 1 {
                            return Err(syn::Error::new_spanned(
                                input,
                                "EnumWrapperFrom only supports enums with a single field",
                            ));
                        }
                        let field = fields.unnamed.first().unwrap();
                        let field_ty = &field.ty;
                        expanded.extend(quote! {
                            impl From<#field_ty> for #name {
                                fn from(value: #field_ty) -> Self {
                                    Self::#variant_name(value)
                                }
                            }
                        });
                    }
                    _ => {
                        return Err(syn::Error::new_spanned(
                            input,
                            "EnumWrapperFrom only supports unnamed fields",
                        ));
                    }
                }
            }
        }
        _ => {
            return Err(syn::Error::new_spanned(
                input,
                "EnumWrapperFrom only supports enums",
            ));
        }
    }

    Ok(expanded)
}
