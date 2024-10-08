use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{DeriveInput, Fields};

pub fn enum_combined_wrapper(input: DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;
    let data = &input.data;

    let mut expanded = quote! {};
    let mut ty_assert_and_as = quote! {};

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

                        let is_ty = format_ident!("is_{}", variant_name.to_string().to_lowercase());
                        let as_ty = format_ident!("as_{}", variant_name.to_string().to_lowercase());
                        let as_mut_ty =
                            format_ident!("as_{}_mut", variant_name.to_string().to_lowercase());

                        ty_assert_and_as.extend(quote! {
                            pub fn #is_ty(&self) -> bool {
                                matches!(self, Self::#variant_name(_))
                            }

                            pub fn #as_ty(&self) -> Option<&#field_ty> {
                                if let Self::#variant_name(value) = self {
                                    Some(value)
                                } else {
                                    None
                                }
                            }

                            pub fn #as_mut_ty(&mut self) -> Option<&mut #field_ty> {
                                if let Self::#variant_name(value) = self {
                                    Some(value)
                                } else {
                                    None
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

    expanded.extend(quote! {
        impl #name {
            #ty_assert_and_as
        }
    });

    Ok(expanded)
}
