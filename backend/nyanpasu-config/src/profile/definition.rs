use serde::{Deserialize, Serialize};
use specta::Type;

use super::*;

/// Top-level semantic split.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProfileDefinition {
    Config { config: ConfigDefinition },
    Transform { transform: TransformDefinition },
}

/// A profile that can produce a complete config and can be selected by current.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConfigDefinition {
    /// A config parsed from a locally materialized file.
    File(FileConfig),

    /// A config composed from an optional full base and proxy contributors.
    Composition(CompositionConfig),
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct FileConfig {
    pub source: ProfileSource,

    /// Scoped post-processing transforms.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transforms: Vec<ProfileId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CompositionConfig {
    /// Optional full config seed.
    ///
    /// - Some(id): inherit a full scoped config from `id`.
    /// - None: start from a clean config seed and only extend proxies/nodes from
    ///   `extend_proxies_from` before running `transforms`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<ProfileId>,

    /// Members that only contribute their scoped-result proxies/nodes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extend_proxies_from: Vec<ProfileId>,

    /// Post-processing transforms after proxies have been extended.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transforms: Vec<ProfileId>,
}

/// A named config transformer. Transform profiles are reusable but not activatable.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TransformDefinition {
    /// Declarative YAML overlay/patch. This is the new name for legacy Merge.
    Overlay(OverlayTransform),

    /// Imperative JS/Lua transform.
    Script(ScriptTransform),
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct OverlayTransform {
    pub source: ProfileSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ScriptTransform {
    pub source: ProfileSource,
    pub runtime: ScriptRuntime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ProfileCategory {
    Config,
    Transform,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TransformKind {
    Overlay,
    Script { runtime: ScriptRuntime },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ScriptRuntime {
    #[default]
    #[serde(rename = "javascript")]
    JavaScript,
    Lua,
}

impl ProfileDefinition {
    pub fn category(&self) -> ProfileCategory {
        match self {
            Self::Config { .. } => ProfileCategory::Config,
            Self::Transform { .. } => ProfileCategory::Transform,
        }
    }

    pub fn is_config(&self) -> bool {
        matches!(self, Self::Config { .. })
    }

    pub fn is_transform(&self) -> bool {
        matches!(self, Self::Transform { .. })
    }

    pub fn is_direct_file_config(&self) -> bool {
        matches!(
            self,
            Self::Config {
                config: ConfigDefinition::File(_)
            }
        )
    }

    pub fn source(&self) -> Option<&ProfileSource> {
        match self {
            Self::Config {
                config: ConfigDefinition::File(file),
            } => Some(&file.source),
            Self::Config {
                config: ConfigDefinition::Composition(_),
            } => None,
            Self::Transform { transform } => Some(transform.source()),
        }
    }

    pub fn source_mut(&mut self) -> Option<&mut ProfileSource> {
        match self {
            Self::Config {
                config: ConfigDefinition::File(file),
            } => Some(&mut file.source),
            Self::Config {
                config: ConfigDefinition::Composition(_),
            } => None,
            Self::Transform { transform } => Some(transform.source_mut()),
        }
    }
}

impl ConfigDefinition {
    pub fn transforms(&self) -> &[ProfileId] {
        match self {
            Self::File(file) => &file.transforms,
            Self::Composition(composition) => &composition.transforms,
        }
    }

    pub fn transforms_mut(&mut self) -> &mut Vec<ProfileId> {
        match self {
            Self::File(file) => &mut file.transforms,
            Self::Composition(composition) => &mut composition.transforms,
        }
    }

    pub fn source(&self) -> Option<&ProfileSource> {
        match self {
            Self::File(file) => Some(&file.source),
            Self::Composition(_) => None,
        }
    }
}

impl TransformDefinition {
    pub fn kind(&self) -> TransformKind {
        match self {
            Self::Overlay(_) => TransformKind::Overlay,
            Self::Script(script) => TransformKind::Script {
                runtime: script.runtime,
            },
        }
    }

    pub fn source(&self) -> &ProfileSource {
        match self {
            Self::Overlay(overlay) => &overlay.source,
            Self::Script(script) => &script.source,
        }
    }

    pub fn source_mut(&mut self) -> &mut ProfileSource {
        match self {
            Self::Overlay(overlay) => &mut overlay.source,
            Self::Script(script) => &mut script.source,
        }
    }
}
