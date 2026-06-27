use super::super::{Ctx, MigrationStep, ModuleMigrator};
use once_cell::sync::Lazy;
use semver::Version;
use serde_yaml::{
    Mapping, Value,
    value::{Tag, TaggedValue},
};

pub static MIGRATOR: ProfilesMigrator = ProfilesMigrator;

static VERSION_2_0_0: Lazy<Version> = Lazy::new(|| Version::parse("2.0.0").unwrap());
static NULL_VALUE: MigrateProfilesNullValue = MigrateProfilesNullValue;
static SCRIPT_NEWTYPE: MigrateProfileScriptNewtype = MigrateProfileScriptNewtype;
static STEPS: [&dyn MigrationStep; 2] = [&NULL_VALUE, &SCRIPT_NEWTYPE];

pub struct ProfilesMigrator;

impl ModuleMigrator for ProfilesMigrator {
    fn module(&self) -> &'static str {
        "profiles"
    }

    fn detect_baseline(&self, ctx: &Ctx) -> anyhow::Result<u64> {
        let profiles_path = ctx.profiles_path();
        if !profiles_path.exists() {
            return Ok(current_revision());
        }

        let raw = std::fs::read_to_string(&profiles_path)?;
        let profiles: Mapping = serde_yaml::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("failed to parse profiles: {e}"))?;
        if needs_null_value_migration(&profiles) || needs_script_newtype_migration(&profiles) {
            Ok(0)
        } else {
            Ok(current_revision())
        }
    }

    fn steps(&self) -> &'static [&'static dyn MigrationStep] {
        &STEPS
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MigrateProfilesNullValue;

impl MigrationStep for MigrateProfilesNullValue {
    fn id(&self) -> &'static str {
        "profiles/null_value"
    }

    fn module(&self) -> &'static str {
        "profiles"
    }

    fn revision(&self) -> u64 {
        1
    }

    fn introduced_in(&self) -> &'static Version {
        &VERSION_2_0_0
    }

    fn name(&self) -> &'static str {
        "MigrateProfilesNullValue"
    }

    fn run(&self, ctx: &mut Ctx) -> anyhow::Result<()> {
        let profiles_path = ctx.profiles_path();
        if !profiles_path.exists() {
            return Ok(());
        }
        let profiles = std::fs::read_to_string(profiles_path.clone())?;
        let mut profiles: Mapping = serde_yaml::from_str(&profiles)
            .map_err(|e| anyhow::anyhow!("failed to parse profiles: {e}"))?;

        profiles.iter_mut().for_each(|(key, value)| {
            if value.is_null() {
                println!("detected null value in profiles {key:?} should be migrated");
                *value = serde_yaml::Value::Sequence(Vec::new());
            }
        });
        write_profiles_atomic(&profiles_path, &profiles, None)?;
        Ok(())
    }

    fn rollback(&self, ctx: &mut Ctx) -> anyhow::Result<()> {
        let profiles_path = ctx.profiles_path();
        if !profiles_path.exists() {
            return Ok(());
        }
        let profiles = std::fs::read_to_string(profiles_path.clone())?;
        let mut profiles: Mapping = serde_yaml::from_str(&profiles)
            .map_err(|e| anyhow::anyhow!("failed to parse profiles: {e}"))?;

        profiles.iter_mut().for_each(|(key, value)| {
            if key.is_string() && key.as_str().unwrap() == "chain" && value.is_sequence() {
                println!("detected sequence value in profiles {key:?} should be migrated");
                *value = serde_yaml::Value::Null;
            }
            if key.is_string() && key.as_str().unwrap() == "current" && value.is_sequence() {
                println!("detected sequence value in profiles {key:?} should be migrated");
                *value = serde_yaml::Value::Null;
            }
        });
        write_profiles_atomic(&profiles_path, &profiles, None)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MigrateProfileScriptNewtype;

impl MigrationStep for MigrateProfileScriptNewtype {
    fn id(&self) -> &'static str {
        "profiles/script_newtype"
    }

    fn module(&self) -> &'static str {
        "profiles"
    }

    fn revision(&self) -> u64 {
        2
    }

    fn introduced_in(&self) -> &'static Version {
        &VERSION_2_0_0
    }

    fn name(&self) -> &'static str {
        "MigrateProfileScriptNewtype"
    }

    fn run(&self, ctx: &mut Ctx) -> anyhow::Result<()> {
        let profiles_path = ctx.profiles_path();
        if !profiles_path.exists() {
            eprintln!("profiles dir not found, skipping migration");
            return Ok(());
        }
        eprintln!("Trying to read profiles files...");
        let profiles = std::fs::read_to_string(profiles_path.clone())?;
        eprintln!("Trying to parse profiles files...");
        let profiles: Mapping = serde_yaml::from_str(&profiles)
            .map_err(|e| anyhow::anyhow!("failed to parse profiles: {e}"))?;
        eprintln!("Trying to migrate profiles files...");
        let profiles = migrate_profile_data(profiles);
        eprintln!("Trying to write profiles files...");
        write_profiles_atomic(
            &profiles_path,
            &profiles,
            Some("# Profiles Config for Clash Nyanpasu"),
        )?;
        Ok(())
    }

    fn rollback(&self, ctx: &mut Ctx) -> anyhow::Result<()> {
        let profiles_path = ctx.profiles_path();
        if !profiles_path.exists() {
            eprintln!("profiles dir not found, skipping discard");
            return Ok(());
        }
        eprintln!("Trying to read profiles files...");
        let profiles = std::fs::read_to_string(profiles_path.clone())?;
        eprintln!("Trying to parse profiles files...");
        let profiles: Mapping = serde_yaml::from_str(&profiles)
            .map_err(|e| anyhow::anyhow!("failed to parse profiles: {e}"))?;
        eprintln!("Trying to discard profiles files...");
        let profiles = discard_profile_data(profiles);
        eprintln!("Trying to write profiles files...");
        write_profiles_atomic(
            &profiles_path,
            &profiles,
            Some("# Profiles Config for Clash Nyanpasu"),
        )?;
        Ok(())
    }
}

/// Atomically persist a profiles mapping, mirroring [`crate::utils::help::save_yaml`]
/// but writing through a temp file + rename so a crash mid-write can never
/// truncate the user's `profiles.yaml`.
fn write_profiles_atomic(
    path: &std::path::Path,
    profiles: &Mapping,
    prefix: Option<&str>,
) -> anyhow::Result<()> {
    let body = serde_yaml::to_string(profiles)
        .map_err(|e| anyhow::anyhow!("failed to serialize profiles: {e}"))?;
    let content = match prefix {
        Some(prefix) => format!("{prefix}\n\n{body}"),
        None => body,
    };
    crate::core::migration::fs::atomic_write(path, content.as_bytes())
}

fn current_revision() -> u64 {
    STEPS.last().map(|step| step.revision()).unwrap_or_default()
}

fn needs_null_value_migration(mapping: &Mapping) -> bool {
    mapping.values().any(Value::is_null)
}

fn needs_script_newtype_migration(mapping: &Mapping) -> bool {
    mapping
        .get("items")
        .and_then(Value::as_sequence)
        .is_some_and(|items| {
            items.iter().any(|item| {
                item.as_mapping()
                    .and_then(|item| item.get("type"))
                    .is_some_and(|ty| matches!(ty, Value::Tagged(tag) if tag.tag == "script"))
            })
        })
}

fn migrate_profile_data(mut mapping: Mapping) -> Mapping {
    if let Some(items) = mapping.get_mut("items")
        && let Some(items) = items.as_sequence_mut()
    {
        for item in items {
            if let Some(item) = item.as_mapping_mut()
                && let Some(ty) = item.get("type").cloned()
                && let Value::Tagged(tag) = ty
                && tag.tag == "script"
                && let Some(script_kind) = tag.value.as_str()
            {
                item.insert(
                    "type".into(),
                    serde_yaml::Value::String("script".to_string()),
                );
                item.insert(
                    "script_type".into(),
                    serde_yaml::Value::String(script_kind.to_string()),
                );
            }
        }
    }

    mapping
}

fn discard_profile_data(mut mapping: Mapping) -> Mapping {
    if let Some(items) = mapping.get_mut("items")
        && let Some(items) = items.as_sequence_mut()
    {
        for item in items {
            if let Some(item) = item.as_mapping_mut()
                && let Some(ty) = item.get("type").cloned()
                && let Value::String(ty) = ty
                && ty == "script"
                && let Some(script_kind) = item.get("script_type").cloned()
            {
                item.insert(
                    "type".into(),
                    serde_yaml::Value::Tagged(Box::new(TaggedValue {
                        tag: Tag::new("script"),
                        value: script_kind,
                    })),
                );
                item.remove("script_type");
            }
        }
    }

    mapping
}

#[cfg(test)]
mod tests {
    use crate::config::Profiles;

    use super::*;
    use pretty_assertions::assert_str_eq;

    const ORIGINAL_SAMPLE: &str = r#"current:
- rIWXPHuafvEM
chain: []
valid:
- dns
- unified-delay
- tcp-concurrent
- tun
- profile
items:
- uid: rIWXPHuafvEM
  type: remote
  name: 🌸云
  file: rIWXPHuafvEM.yaml
  desc: null
  updated: 1758110672
  url: https://example.com
  extra:
    upload: 3641183914
    download: 39111158992
    total: 42946719600
    expire: 1769123200
  option:
    with_proxy: false
    self_proxy: true
    update_interval: 1440
  chain:
  - siL1cvjnvLB6
  - sxI0dHKeqSNg
- uid: siL1cvjnvLB6
  type: !script javascript
  name: 花☁️处理
  file: siL1cvjnvLB6.js
  desc: ''
  updated: 1720954186
- uid: sxI0dHKeqSNg
  type: !script javascript
  name: 🌸☁️图标
  file: sxI0dHKeqSNg.js
  desc: ''
  updated: 1722656540
- uid: sZYZe33w7RKV
  type: !script lua
  name: 图标
  file: sZYZe33w7RKV.lua
  desc: ''
  updated: 1724082226
- uid: lkvV5JXfzO34
  type: local
  name: New Profile
  file: lkvV5JXfzO34.yaml
  desc: ''
  updated: 1725587682
  chain: []
- uid: lJynXCoMMIUd
  type: local
  name: New Profile
  file: lJynXCoMMIUd.yaml
  desc: ''
  updated: 1726252304
  chain: []
- uid: lBtaVEaMAR97
  type: local
  name: Test
  file: lBtaVEaMAR97.yaml
  desc: ''
  updated: 1727621893
  chain: []
"#;

    const MIGRATED_SAMPLE: &str = r#"current:
- rIWXPHuafvEM
chain: []
valid:
- dns
- unified-delay
- tcp-concurrent
- tun
- profile
items:
- uid: rIWXPHuafvEM
  type: remote
  name: 🌸云
  file: rIWXPHuafvEM.yaml
  desc: null
  updated: 1758110672
  url: https://example.com
  extra:
    upload: 3641183914
    download: 39111158992
    total: 42946719600
    expire: 1769123200
  option:
    with_proxy: false
    self_proxy: true
    update_interval: 1440
  chain:
  - siL1cvjnvLB6
  - sxI0dHKeqSNg
- uid: siL1cvjnvLB6
  type: script
  name: 花☁️处理
  file: siL1cvjnvLB6.js
  desc: ''
  updated: 1720954186
  script_type: javascript
- uid: sxI0dHKeqSNg
  type: script
  name: 🌸☁️图标
  file: sxI0dHKeqSNg.js
  desc: ''
  updated: 1722656540
  script_type: javascript
- uid: sZYZe33w7RKV
  type: script
  name: 图标
  file: sZYZe33w7RKV.lua
  desc: ''
  updated: 1724082226
  script_type: lua
- uid: lkvV5JXfzO34
  type: local
  name: New Profile
  file: lkvV5JXfzO34.yaml
  desc: ''
  updated: 1725587682
  chain: []
- uid: lJynXCoMMIUd
  type: local
  name: New Profile
  file: lJynXCoMMIUd.yaml
  desc: ''
  updated: 1726252304
  chain: []
- uid: lBtaVEaMAR97
  type: local
  name: Test
  file: lBtaVEaMAR97.yaml
  desc: ''
  updated: 1727621893
  chain: []
"#;

    #[test]
    fn test_migrate_existing_data() {
        let original_data = serde_yaml::from_str::<Mapping>(ORIGINAL_SAMPLE).unwrap();
        let migrated_data = migrate_profile_data(original_data);
        let output_data = serde_yaml::to_string(&migrated_data).unwrap();
        assert_str_eq!(output_data, MIGRATED_SAMPLE);
    }

    #[test]
    fn test_discard_existing_data() {
        let migrated_data = serde_yaml::from_str::<Mapping>(MIGRATED_SAMPLE).unwrap();
        let original_data = discard_profile_data(migrated_data);
        let output_data = serde_yaml::to_string(&original_data).unwrap();
        assert_str_eq!(output_data, ORIGINAL_SAMPLE);
    }

    #[test]
    #[ignore]
    fn test_profile_parse_migrated_data() {
        let profiles = serde_yaml::from_str::<Profiles>(MIGRATED_SAMPLE).unwrap();
        eprintln!("{profiles:#?}");
    }
}
