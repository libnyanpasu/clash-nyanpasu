use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum UpdateSource {
    Nyanpasu,
    Github,
    Ghfast,
    Sourceforge,
}

pub fn default_update_sources() -> Vec<UpdateSource> {
    vec![UpdateSource::Nyanpasu, UpdateSource::Github]
}

pub fn validate_update_sources(sources: &[UpdateSource]) -> Result<(), &'static str> {
    if sources.is_empty() {
        return Err("at least one update source must be selected");
    }
    for (index, source) in sources.iter().enumerate() {
        if sources[..index].contains(source) {
            return Err("update sources must not contain duplicates");
        }
    }
    Ok(())
}
