use std::borrow::Cow;

use semver::Version;
use serde_yaml::{
    Mapping, Value,
    value::{Tag, TaggedValue},
};

use crate::{core::migration::Migration, utils::help};

#[derive(Debug, Clone, Copy)]
/// 将
/// ```yaml
/// type: !script javascript
/// ```
/// 展开为
/// ```yaml
/// type: script
/// script_type: javascript
/// ```
/// 其他不做特殊处理
pub struct MigrateProfileScriptNewtype;

impl Migration<'_> for MigrateProfileScriptNewtype {
    fn version(&self) -> &'static Version {
        &super::VERSION
    }

    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed("MigrateProfileScriptNewtype")
    }

    fn migrate(&self) -> std::io::Result<()> {
        let profiles_path = crate::utils::dirs::profiles_path().map_err(std::io::Error::other)?;
        if !profiles_path.exists() {
            eprintln!("profiles dir not found, skipping migration");
            return Ok(());
        }
        eprintln!("Trying to read profiles files...");
        let profiles = std::fs::read_to_string(profiles_path.clone())?;
        eprintln!("Trying to parse profiles files...");
        let profiles: Mapping = serde_yaml::from_str(&profiles)
            .map_err(|e| std::io::Error::other(format!("failed to parse profiles: {e}")))?;
        eprintln!("Trying to migrate profiles files...");
        let profiles = migrate_profile_data(profiles);
        eprintln!("Trying to write profiles files...");
        help::save_yaml(
            &profiles_path,
            &profiles,
            Some("# Profiles Config for Clash Nyanpasu"),
        )
        .map_err(std::io::Error::other)?;
        Ok(())
    }

    fn discard(&self) -> std::io::Result<()> {
        let profiles_path = crate::utils::dirs::profiles_path().map_err(std::io::Error::other)?;
        if !profiles_path.exists() {
            eprintln!("profiles dir not found, skipping discard");
            return Ok(());
        }
        eprintln!("Trying to read profiles files...");
        let profiles = std::fs::read_to_string(profiles_path.clone())?;
        eprintln!("Trying to parse profiles files...");
        let profiles: Mapping = serde_yaml::from_str(&profiles)
            .map_err(|e| std::io::Error::other(format!("failed to parse profiles: {e}")))?;
        eprintln!("Trying to discard profiles files...");
        let profiles = discard_profile_data(profiles);
        eprintln!("Trying to write profiles files...");
        help::save_yaml(
            &profiles_path,
            &profiles,
            Some("# Profiles Config for Clash Nyanpasu"),
        )
        .map_err(std::io::Error::other)?;
        Ok(())
    }
}

fn migrate_profile_data(mut mapping: serde_yaml::Mapping) -> serde_yaml::Mapping {
    // We just need to iter items
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

fn discard_profile_data(mut mapping: serde_yaml::Mapping) -> serde_yaml::Mapping {
    // We just need to iter items
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
        let original_data = serde_yaml::from_str::<serde_yaml::Mapping>(ORIGINAL_SAMPLE).unwrap();
        let migrated_data = migrate_profile_data(original_data);
        let output_data = serde_yaml::to_string(&migrated_data).unwrap();
        assert_str_eq!(output_data, MIGRATED_SAMPLE);
    }

    #[test]
    fn test_discard_existing_data() {
        let migrated_data = serde_yaml::from_str::<serde_yaml::Mapping>(MIGRATED_SAMPLE).unwrap();
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
