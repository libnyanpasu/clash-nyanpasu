use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

mod builder_update;
mod enum_wrapper_combined;
mod verge_patch;

#[proc_macro_derive(BuilderUpdate, attributes(builder_update))]
pub fn builder_update(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match builder_update::builder_update(input) {
        Ok(token_stream) => TokenStream::from(token_stream),
        Err(e) => TokenStream::from(e.to_compile_error()),
    }
}

#[proc_macro_derive(VergePatch, attributes(verge))]
pub fn verge_patch(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match verge_patch::verge_patch(input) {
        Ok(token_stream) => TokenStream::from(token_stream),
        Err(e) => TokenStream::from(e.to_compile_error()),
    }
}

#[proc_macro_derive(EnumWrapperCombined)]
pub fn enum_wrapper_from(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match enum_wrapper_combined::enum_combined_wrapper(input) {
        Ok(token_stream) => TokenStream::from(token_stream),
        Err(e) => TokenStream::from(e.to_compile_error()),
    }
}
