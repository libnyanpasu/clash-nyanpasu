use crate::{
    clash::config::overrides::{LogLevel, ManageableField, Mode},
    runtime::value::ConfigValue,
};

/// How post-processing treats one runtime config key, shaped like the config.
#[derive(Debug, Clone, PartialEq)]
pub enum Ownership {
    /// Nyanpasu writes this value over whatever the profiles produced.
    Forced(ConfigValue),
    /// Nyanpasu removes the key, such as a listener the user turned off.
    Removed,
    /// Written only where the profiles left the key out.
    Default(ConfigValue),
    /// The profiles' value stays.
    Inherited,
    /// Decided key by key within the mapping at the key, which keeps the keys
    /// it does not name.
    Table {
        entries: OwnershipEntries,
        /// Whether a missing mapping is created for the entries.
        create: bool,
    },
}

/// The owned keys of one mapping, in field order.
pub type OwnershipEntries = Vec<(&'static str, Ownership)>;

/// A setting that decides one runtime config key.
pub trait OwnedField {
    fn ownership(&self) -> Ownership;
}

/// A settings struct whose fields decide the keys of one runtime config
/// mapping. Derived with `nyanpasu_macro::OwnedFields`, which also makes the
/// struct an [`OwnedField`] deciding a table it creates when missing.
pub trait OwnedFields {
    fn ownership_entries(&self) -> OwnershipEntries;
}

macro_rules! forced_value {
    ($($ty:ty),* $(,)?) => {
        $(
            impl OwnedField for $ty {
                fn ownership(&self) -> Ownership {
                    Ownership::Forced(ConfigValue::from_json_value(serde_json::json!(self)))
                }
            }
        )*
    };
}

forced_value!(bool, String, LogLevel, Mode);

/// A managed field decides its key as its value does; an unmanaged one leaves
/// the key to the profiles.
impl<T: OwnedField> OwnedField for ManageableField<T> {
    fn ownership(&self) -> Ownership {
        match self.managed() {
            Some(value) => value.ownership(),
            None => Ownership::Inherited,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde::Serialize;
    use serde_json::json;

    use super::*;

    fn forced(value: serde_json::Value) -> Ownership {
        Ownership::Forced(ConfigValue::from_json_value(value))
    }

    #[derive(Serialize, nyanpasu_macro::OwnedFields)]
    #[serde(rename_all = "kebab-case")]
    struct Inner {
        auto_route: ManageableField<bool>,
        #[serde(rename = "renamed")]
        stack: String,
    }

    #[derive(Serialize, nyanpasu_macro::OwnedFields)]
    #[serde(rename_all = "kebab-case")]
    struct Outer {
        allow_lan: bool,
        tun: Inner,
        mode: ManageableField<Mode>,
        #[cfg(any())]
        never_compiled: bool,
    }

    /// A nested struct decides a table under its serde name, and an unmanaged
    /// field leaves its key to the profiles.
    #[test]
    fn nested_structs_own_tables_keyed_by_serde_names() {
        let outer = Outer {
            allow_lan: true,
            tun: Inner {
                auto_route: ManageableField::Managed(false),
                stack: "gvisor".to_string(),
            },
            mode: ManageableField::Unmanaged,
        };

        assert_eq!(
            outer.ownership_entries(),
            vec![
                ("allow-lan", forced(json!(true))),
                (
                    "tun",
                    Ownership::Table {
                        entries: vec![
                            ("auto-route", forced(json!(false))),
                            ("renamed", forced(json!("gvisor"))),
                        ],
                        create: true,
                    }
                ),
                ("mode", Ownership::Inherited),
            ]
        );
    }

    /// A managed struct decides its table; an unmanaged one leaves the whole
    /// table to the profiles.
    #[test]
    fn a_manageable_struct_owns_its_table_only_when_managed() {
        let managed = ManageableField::Managed(Inner {
            auto_route: ManageableField::Unmanaged,
            stack: "system".to_string(),
        });

        assert_eq!(
            managed.ownership(),
            Ownership::Table {
                entries: vec![
                    ("auto-route", Ownership::Inherited),
                    ("renamed", forced(json!("system"))),
                ],
                create: true,
            }
        );
        assert_eq!(
            ManageableField::<Inner>::Unmanaged.ownership(),
            Ownership::Inherited
        );
    }
}
