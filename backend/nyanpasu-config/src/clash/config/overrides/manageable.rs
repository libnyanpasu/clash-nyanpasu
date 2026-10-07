use serde::{Deserialize, Serialize};

/// A runtime config field that Nyanpasu may take over from the profiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, specta::Type)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ManageableField<T> {
    /// Written into the runtime config over whatever the profiles and
    /// transforms produced.
    Managed(T),
    /// Left to the profiles and transforms, subject to the field filter.
    Unmanaged,
}

impl<T> ManageableField<T> {
    /// The value Nyanpasu writes, if it manages the field.
    pub fn managed(&self) -> Option<&T> {
        match self {
            Self::Managed(value) => Some(value),
            Self::Unmanaged => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wire shape is adjacently tagged, so the frontend can switch on
    /// `kind` alone.
    #[test]
    fn serializes_adjacently_tagged() {
        assert_eq!(
            serde_json::to_value(ManageableField::Managed(false)).unwrap(),
            serde_json::json!({ "kind": "managed", "value": false })
        );
        assert_eq!(
            serde_json::to_value(ManageableField::<bool>::Unmanaged).unwrap(),
            serde_json::json!({ "kind": "unmanaged" })
        );
        assert_eq!(
            serde_json::from_value::<ManageableField<bool>>(
                serde_json::json!({ "kind": "unmanaged" })
            )
            .unwrap(),
            ManageableField::Unmanaged
        );
    }
}
