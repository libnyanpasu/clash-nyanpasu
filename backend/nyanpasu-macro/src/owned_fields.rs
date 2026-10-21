use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    Attribute, Data, DeriveInput, Error, Fields, LitStr, Result, Token, ext::IdentExt,
    meta::ParseNestedMeta, spanned::Spanned,
};

/// The serde `rename_all` cases the owned keys can follow.
enum RenameAll {
    Unchanged,
    KebabCase,
}

impl RenameAll {
    fn apply(&self, field: &str) -> String {
        match self {
            Self::Unchanged => field.to_string(),
            Self::KebabCase => field.replace('_', "-"),
        }
    }
}

/// Implements `OwnedFields` and `OwnedField` for a struct with named fields.
/// Each field decides the key of its serde name: its own `rename`, else the
/// container's `rename_all`. `#[cfg]` on a field carries over to its entry.
pub fn owned_fields(input: DeriveInput) -> Result<TokenStream> {
    let name = &input.ident;
    let Data::Struct(data) = &input.data else {
        return Err(Error::new(
            input.span(),
            "OwnedFields supports structs only",
        ));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(Error::new(
            input.span(),
            "OwnedFields supports named fields only",
        ));
    };

    let rename_all = serde_rename_all(&input.attrs)?;
    let mut entries = Vec::with_capacity(fields.named.len());
    for field in &fields.named {
        let ident = field.ident.as_ref().expect("named field");
        let key = match serde_rename(&field.attrs)? {
            Some(key) => key,
            None => rename_all.apply(&ident.unraw().to_string()),
        };
        let cfgs = field
            .attrs
            .iter()
            .filter(|attr| attr.path().is_ident("cfg"));
        entries.push(quote! {
            #(#cfgs)*
            entries.push((
                #key,
                ::nyanpasu_config::runtime::post_process::OwnedField::ownership(&self.#ident),
            ));
        });
    }

    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    Ok(quote! {
        impl #impl_generics ::nyanpasu_config::runtime::post_process::OwnedFields
            for #name #ty_generics #where_clause
        {
            fn ownership_entries(
                &self,
            ) -> ::nyanpasu_config::runtime::post_process::OwnershipEntries {
                let mut entries = ::std::vec::Vec::new();
                #(#entries)*
                entries
            }
        }

        impl #impl_generics ::nyanpasu_config::runtime::post_process::OwnedField
            for #name #ty_generics #where_clause
        {
            fn ownership(&self) -> ::nyanpasu_config::runtime::post_process::Ownership {
                ::nyanpasu_config::runtime::post_process::Ownership::Table {
                    entries: ::nyanpasu_config::runtime::post_process::OwnedFields::ownership_entries(
                        self,
                    ),
                    create: true,
                }
            }
        }
    })
}

fn serde_rename_all(attrs: &[Attribute]) -> Result<RenameAll> {
    let mut rename_all = RenameAll::Unchanged;
    for_each_serde_meta(attrs, |meta| {
        if !meta.path.is_ident("rename_all") {
            return skip_meta(meta);
        }
        let case: LitStr = meta.value()?.parse()?;
        rename_all = match case.value().as_str() {
            "snake_case" => RenameAll::Unchanged,
            "kebab-case" => RenameAll::KebabCase,
            other => {
                return Err(Error::new(
                    case.span(),
                    format!("OwnedFields does not support rename_all = \"{other}\""),
                ));
            }
        };
        Ok(())
    })?;
    Ok(rename_all)
}

fn serde_rename(attrs: &[Attribute]) -> Result<Option<String>> {
    let mut rename = None;
    for_each_serde_meta(attrs, |meta| {
        if !meta.path.is_ident("rename") {
            return skip_meta(meta);
        }
        if !meta.input.peek(Token![=]) {
            return Err(meta.error("OwnedFields needs a single `rename = \"...\"`"));
        }
        let key: LitStr = meta.value()?.parse()?;
        rename = Some(key.value());
        Ok(())
    })?;
    Ok(rename)
}

fn for_each_serde_meta(
    attrs: &[Attribute],
    mut visit: impl FnMut(&ParseNestedMeta) -> Result<()>,
) -> Result<()> {
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("serde")) {
        attr.parse_nested_meta(|meta| visit(&meta))?;
    }
    Ok(())
}

/// Consumes a serde argument this derive does not read, such as `default`,
/// `with = "..."` or `bound(...)`.
fn skip_meta(meta: &ParseNestedMeta) -> Result<()> {
    if meta.input.peek(Token![=]) {
        meta.value()?.parse::<syn::Expr>()?;
    } else if meta.input.peek(syn::token::Paren) {
        let _content;
        syn::parenthesized!(_content in meta.input);
    }
    Ok(())
}
