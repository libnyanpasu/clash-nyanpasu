pub mod error;
pub mod ports;
pub mod profile_file;

use nyanpasu_config::profile::{ConfigDefinition, ProfileDefinition, ProfileId, Profiles};

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
