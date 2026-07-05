//! Test doubles and Profiles literal builders. Pure values only (CLAUDE.md §13).

use std::collections::HashMap;

use crate::{
    profile::{
        CompositionConfig, ConfigDefinition, FileConfig, LocalBinding, ManagedProfilePath,
        MaterializedFile, OverlayTransform, ProfileDefinition, ProfileId, ProfileItem,
        ProfileMetadata, ProfileSource, Profiles, ScriptRuntime, ScriptTransform,
        TransformDefinition,
    },
    runtime::{
        executor::{PortError, ProfileContentSource, ScriptRunOutcome, ScriptRunner, StepLogEntry},
        value::ConfigValue,
    },
};

pub fn pid(value: &str) -> ProfileId {
    ProfileId(value.to_owned())
}

pub struct MapContentSource(pub HashMap<ManagedProfilePath, String>);

impl MapContentSource {
    pub fn from_pairs(pairs: &[(&str, &str)]) -> Self {
        Self(
            pairs
                .iter()
                .map(|(path, content)| {
                    (
                        ManagedProfilePath::new(*path).expect("test path must be managed"),
                        (*content).to_string(),
                    )
                })
                .collect(),
        )
    }
}

impl ProfileContentSource for MapContentSource {
    fn read(&self, path: &ManagedProfilePath) -> Result<String, PortError> {
        self.0
            .get(path)
            .cloned()
            .ok_or_else(|| format!("no content for {path}").into())
    }
}

pub enum RunReply {
    Replace(serde_json::Value, Vec<StepLogEntry>),
    Fail(String, Vec<StepLogEntry>),
}

pub enum PredicateReply {
    Fixed(bool),
    ByItem(fn(&ConfigValue) -> bool),
    Fail(String),
}

pub enum ExprReply {
    Fixed(serde_json::Value),
    Fail(String),
}

/// Replays scripted outcomes; unknown sources echo the input config, unknown
/// predicates return `true`, unknown exprs echo the item.
#[derive(Default)]
pub struct FakeScriptRunner {
    pub runs: HashMap<String, RunReply>,
    pub predicates: HashMap<String, PredicateReply>,
    pub exprs: HashMap<String, ExprReply>,
}

impl ScriptRunner for FakeScriptRunner {
    fn run(&self, _runtime: ScriptRuntime, source: &str, config: &ConfigValue) -> ScriptRunOutcome {
        match self.runs.get(source) {
            Some(RunReply::Replace(json, logs)) => ScriptRunOutcome {
                result: Ok(ConfigValue::try_from(json.clone()).expect("fake run json")),
                logs: logs.clone(),
            },
            Some(RunReply::Fail(message, logs)) => ScriptRunOutcome {
                result: Err(message.clone().into()),
                logs: logs.clone(),
            },
            None => ScriptRunOutcome {
                result: Ok(config.clone()),
                logs: Vec::new(),
            },
        }
    }

    fn eval_item_predicate(&self, expr: &str, item: &ConfigValue) -> Result<bool, PortError> {
        match self.predicates.get(expr) {
            Some(PredicateReply::Fixed(value)) => Ok(*value),
            Some(PredicateReply::ByItem(judge)) => Ok(judge(item)),
            Some(PredicateReply::Fail(message)) => Err(message.clone().into()),
            None => Ok(true),
        }
    }

    fn eval_item_expr(&self, expr: &str, item: &ConfigValue) -> Result<ConfigValue, PortError> {
        match self.exprs.get(expr) {
            Some(ExprReply::Fixed(json)) => {
                Ok(ConfigValue::try_from(json.clone()).expect("fake expr json"))
            }
            Some(ExprReply::Fail(message)) => Err(message.clone().into()),
            None => Ok(item.clone()),
        }
    }
}

pub fn managed_file_source(file: &str) -> ProfileSource {
    ProfileSource::Local {
        binding: LocalBinding::Managed {
            materialized: MaterializedFile {
                file: ManagedProfilePath::new(file).expect("test path must be managed"),
                updated_at: None,
            },
        },
    }
}

fn metadata(uid: &str) -> ProfileMetadata {
    ProfileMetadata {
        name: uid.to_owned(),
        desc: None,
    }
}

pub fn config_file_item(uid: &str, file: &str, transforms: &[&str]) -> ProfileItem {
    ProfileItem {
        uid: pid(uid),
        metadata: metadata(uid),
        definition: ProfileDefinition::Config {
            config: ConfigDefinition::File(FileConfig {
                source: managed_file_source(file),
                transforms: transforms.iter().map(|t| pid(t)).collect(),
            }),
        },
    }
}

pub fn composition_item(
    uid: &str,
    base: Option<&str>,
    extend: &[&str],
    transforms: &[&str],
) -> ProfileItem {
    ProfileItem {
        uid: pid(uid),
        metadata: metadata(uid),
        definition: ProfileDefinition::Config {
            config: ConfigDefinition::Composition(CompositionConfig {
                base: base.map(pid),
                extend_proxies_from: extend.iter().map(|m| pid(m)).collect(),
                transforms: transforms.iter().map(|t| pid(t)).collect(),
            }),
        },
    }
}

pub fn overlay_item(uid: &str, file: &str) -> ProfileItem {
    ProfileItem {
        uid: pid(uid),
        metadata: metadata(uid),
        definition: ProfileDefinition::Transform {
            transform: TransformDefinition::Overlay(OverlayTransform {
                source: managed_file_source(file),
            }),
        },
    }
}

pub fn script_item(uid: &str, file: &str, runtime: ScriptRuntime) -> ProfileItem {
    ProfileItem {
        uid: pid(uid),
        metadata: metadata(uid),
        definition: ProfileDefinition::Transform {
            transform: TransformDefinition::Script(ScriptTransform {
                source: managed_file_source(file),
                runtime,
            }),
        },
    }
}

pub fn profiles_with(
    current: Option<&str>,
    global_transforms: &[&str],
    valid: &[&str],
    items: Vec<ProfileItem>,
) -> Profiles {
    Profiles {
        current: current.map(pid),
        global_transforms: global_transforms.iter().map(|t| pid(t)).collect(),
        valid: valid.iter().map(|v| (*v).to_string()).collect(),
        items: items
            .into_iter()
            .map(|item| (item.uid.clone(), item))
            .collect(),
    }
}

#[cfg(test)]
mod smoke {
    use super::*;

    #[test]
    fn map_content_source_reads_and_misses() {
        let source = MapContentSource::from_pairs(&[("a.yaml", "proxies: []")]);
        let path = ManagedProfilePath::new("a.yaml").unwrap();
        assert_eq!(source.read(&path).unwrap(), "proxies: []");
        let missing = ManagedProfilePath::new("b.yaml").unwrap();
        assert!(source.read(&missing).is_err());
    }

    #[test]
    fn parse_config_document_applies_yaml_merge_keys() {
        let text = "base: &base\n  a: 1\nmerged:\n  <<: *base\n  b: 2\n";
        let value = crate::runtime::executor::parse_config_document(text).unwrap();
        assert_eq!(
            value.to_json(),
            serde_json::json!({ "base": { "a": 1 }, "merged": { "a": 1, "b": 2 } })
        );
    }
}
