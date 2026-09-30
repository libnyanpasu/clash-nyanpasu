use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

mod builder_update;
mod enum_wrapper_combined;
mod unified_command;
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

#[proc_macro_attribute]
pub fn rpc(attr: TokenStream, item: TokenStream) -> TokenStream {
    let options = parse_macro_input!(attr as unified_command::Options);
    let item = parse_macro_input!(item as syn::ItemFn);
    match unified_command::expand_with_options(item, options) {
        Ok(tokens) => TokenStream::from(tokens),
        Err(error) => TokenStream::from(error.to_compile_error()),
    }
}
