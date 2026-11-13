//! Canonical serialization of profile YAML mappings.

use snafu::ResultExt as _;

use nyanpasu_runtime_values::profiles::{
    NotYamlMappingSnafu, ProfileContentError, ReserializeYamlSnafu,
};

/// Parse and reserialize a YAML mapping so editor saves and File-config reads
/// share one canonical shape (legacy `read_profile_file` normalization).
pub fn normalize_yaml_document(content: &str) -> Result<String, ProfileContentError> {
    let mapping: yaml_serde::Mapping =
        yaml_serde::from_str(content).context(NotYamlMappingSnafu)?;
    yaml_serde::to_string(&mapping).context(ReserializeYamlSnafu)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_yaml_document_round_trips_mappings_and_rejects_garbage() {
        let normalized = normalize_yaml_document("b: 2\na: 1\n").unwrap();
        let value: yaml_serde::Mapping = yaml_serde::from_str(&normalized).unwrap();
        assert_eq!(value.len(), 2);
        assert!(normalize_yaml_document(": not yaml [").is_err());
    }
}
