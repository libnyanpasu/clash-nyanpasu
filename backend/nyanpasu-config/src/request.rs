//! Shared parts of the request contracts that carve one privilege class out of a full patch.
//!
//! A full patch ([`crate::application::NyanpasuAppConfigPatch`], [`crate::clash::config::ClashConfigPatch`],
//! [`crate::clash::config::overrides::ClashGuardOverridesPatch`]) can write every field of its
//! config. Each privilege class has its own concrete request type holding only that class's
//! fields. A request type is decoded through the full patch and projected explicitly; the
//! projection destructures the full patch without `..`, so a field added to the config does not
//! compile until it is classified.

use snafu::Snafu;

/// A request carried a field of another privilege class.
#[derive(Debug, Clone, PartialEq, Eq, Snafu)]
#[snafu(display("{request} cannot write {fields:?}"))]
pub struct FieldsRejected {
    /// The request type that refused the patch.
    pub request: &'static str,
    /// The serialized names of the refused fields, in a fixed order.
    pub fields: Vec<&'static str>,
}

/// Refuses the whole patch when any listed field is present, whatever its value.
///
/// Plain fields are `Option`s: present means `Some`. A `null` field is `None` after decoding, so
/// it writes nothing and is accepted. Fields after `nested:` are nested patches: present means
/// that at least one leaf is named; an empty nested object writes nothing. A field is reported
/// under its Rust name unless `field as "serialized-name"` gives the name the wire uses.
macro_rules! reject_present {
    (@name $field:ident) => {
        stringify!($field)
    };
    (@name $field:ident, $name:literal) => {
        $name
    };
    ($request:literal; $($(#[$meta:meta])* $field:ident $(as $name:literal)?),* $(,)?) => {
        $crate::request::reject_present!($request; $($(#[$meta])* $field $(as $name)?),*; nested:)
    };
    (
        $request:literal;
        $($(#[$meta:meta])* $field:ident $(as $name:literal)?),* $(,)?;
        nested: $($nested:ident),* $(,)?
    ) => {{
        let fields: Vec<&'static str> = [
            $($(#[$meta])* (
                $field.is_some(),
                $crate::request::reject_present!(@name $field $(, $name)?),
            ),)*
            $((!::struct_patch::Status::is_empty(&$nested), stringify!($nested)),)*
        ]
        .into_iter()
        .filter_map(|(present, name)| present.then_some(name))
        .collect();
        if fields.is_empty() {
            Ok(())
        } else {
            Err($crate::request::FieldsRejected { request: $request, fields })
        }
    }};
}
pub(crate) use reject_present;

#[cfg(test)]
pub(crate) mod testing {
    use serde::de::DeserializeOwned;
    use serde_json::Value;

    /// Proves that `body` is a valid unrestricted patch, and that the narrow request refuses it
    /// naming `field`.
    ///
    /// The first step keeps a mistyped sample from passing as a refusal: only a body that the
    /// unrestricted patch itself accepts can show that the narrow request refused it on its class.
    pub(crate) fn assert_refused<Full, Narrow>(request: &str, field: &str, body: Value)
    where
        Full: DeserializeOwned,
        Narrow: DeserializeOwned,
    {
        if let Err(error) = serde_json::from_value::<Full>(body.clone()) {
            panic!("{field}: {body} is not a valid unrestricted patch: {error}");
        }
        let Err(error) = serde_json::from_value::<Narrow>(body.clone()) else {
            panic!("{field}: {body} was accepted by the {request} request");
        };
        let message = error.to_string();
        assert!(
            message.starts_with(&format!("{request} cannot write [")),
            "{field}: {body} failed for another reason: {message}"
        );
        assert!(
            message.contains(&format!("{field:?}")),
            "{field}: {body} did not name the field: {message}"
        );
    }
}
