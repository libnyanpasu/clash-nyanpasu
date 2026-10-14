pub mod error;
pub mod ports;
pub mod profile_file;

use crate::{format::Format, migration::modules::profiles::ProfilesFormat};
use anyhow::Context;
use nyanpasu_config::profile::{ConfigDefinition, ProfileDefinition, ProfileId, Profiles};
use std::fs;

pub fn current_closure(profiles: &Profiles) -> indexmap::IndexSet<ProfileId> {
    let mut closure: indexmap::IndexSet<ProfileId> =
        profiles.global_transforms.iter().cloned().collect();
    let Some(current) = &profiles.current else {
        return closure;
    };

    closure.insert(current.clone());
    let mut configs = vec![current.clone()];
    if let Some(item) = profiles.items.get(current)
        && let ProfileDefinition::Config {
            config: ConfigDefinition::Composition(composition),
        } = &item.definition
    {
        if let Some(base) = &composition.base {
            closure.insert(base.clone());
            configs.push(base.clone());
        }
        for member in &composition.extend_proxies_from {
            closure.insert(member.clone());
            configs.push(member.clone());
        }
    }

    for config in configs {
        if let Some(item) = profiles.items.get(&config)
            && let ProfileDefinition::Config { config } = &item.definition
        {
            for transform in config.transforms() {
                closure.insert(transform.clone());
            }
        }
    }

    closure
}

/// Initialize the default profiles document if its path does not exist.
pub fn init_config(paths: &nyanpasu_paths::PathResolver) -> anyhow::Result<()> {
    let path = paths.profiles_path();
    if !path.exists() {
        // Stamped like every later write, since the app refuses to load
        // an unstamped profiles.yaml.
        let mut content = Vec::new();
        ProfilesFormat::default().serialize(
            &mut content,
            &nyanpasu_config::profile::Profiles::default(),
            Some("# Clash Nyanpasu"),
        )?;
        fs::write(&path, content).with_context(|| format!("failed to save file \"{path}\""))?;
    }

    Ok(())
}

#[cfg(test)]
mod init_config_tests;
