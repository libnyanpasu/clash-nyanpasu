use proc_macro::TokenStream;
use syn::{parse_macro_input, DeriveInput};

mod builder_update;

#[proc_macro_derive(BuilderUpdate, attributes(update))]
pub fn builder_update(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match builder_update::builder_update(input) {
        Ok(token_stream) => TokenStream::from(token_stream),
        Err(e) => TokenStream::from(e.to_compile_error()),
    }
}
