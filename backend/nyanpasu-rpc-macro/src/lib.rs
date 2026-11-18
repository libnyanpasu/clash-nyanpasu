//! The `#[rpc]` attribute, re-exported as `nyanpasu_rpc::rpc`, and the development-only
//! generator of the command catalog from the production declarations.

use proc_macro::TokenStream;
use syn::parse_macro_input;

#[cfg(test)]
mod command_catalog;
mod unified_command;

#[proc_macro_attribute]
pub fn rpc(attr: TokenStream, item: TokenStream) -> TokenStream {
    let options = parse_macro_input!(attr as unified_command::Options);
    let item = parse_macro_input!(item as syn::ItemFn);
    match unified_command::expand_with_options(item, options) {
        Ok(tokens) => TokenStream::from(tokens),
        Err(error) => TokenStream::from(error.to_compile_error()),
    }
}
