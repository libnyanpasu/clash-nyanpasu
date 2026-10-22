use nyanpasu_config::application::ClashCore;
use nyanpasu_paths::PathResolver;
use serde::Serialize;

/// An agent skill a core's release archive ships. The core updater keeps it in the
/// core's data directory, beside the executable it drives.
#[derive(Debug, Serialize, specta::Type)]
pub struct CoreAgentSkill {
    /// The skill's name, which is also the name of its folder and of its executable.
    pub name: String,
    /// The skill's folder, which agents link so that core updates refresh it in place.
    pub dir: String,
    /// Whether a core update has installed the skill.
    pub installed: bool,
}

/// The agent skills `core` ships, and whether a core update has installed each one.
pub(super) async fn core_agent_skills(
    paths: &PathResolver,
    core: ClashCore,
) -> Vec<CoreAgentSkill> {
    let mut skills = Vec::new();
    for name in core.agent_skills() {
        let dir = paths
            .core_data_dir(core.binary_name())
            .join("skills")
            .join(name);
        let installed = tokio::fs::try_exists(dir.join("SKILL.md"))
            .await
            .unwrap_or(false);
        skills.push(CoreAgentSkill {
            name: (*name).to_owned(),
            dir: dir.into_string(),
            installed,
        });
    }
    skills
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(data_dir: &std::path::Path) -> PathResolver {
        let data_dir = camino::Utf8PathBuf::from_path_buf(data_dir.to_path_buf()).unwrap();
        PathResolver::with_base_dirs(data_dir.join("config"), data_dir)
    }

    #[tokio::test]
    async fn a_core_without_skills_lists_none() {
        let dir = tempfile::TempDir::new().unwrap();

        assert!(
            core_agent_skills(&paths(dir.path()), ClashCore::Mihomo)
                .await
                .is_empty()
        );
    }

    #[tokio::test]
    async fn each_meow_core_lists_its_own_installed_skill() {
        let dir = tempfile::TempDir::new().unwrap();
        let paths = paths(dir.path());
        let alpha = paths
            .core_data_dir("meow-alpha")
            .join("skills")
            .join("meow-cli");
        std::fs::create_dir_all(&alpha).unwrap();
        std::fs::write(alpha.join("SKILL.md"), "skill").unwrap();

        let [stable] = &core_agent_skills(&paths, ClashCore::Meow).await[..] else {
            panic!("Meow ships one skill");
        };
        assert_eq!(stable.name, "meow-cli");
        assert!(!stable.installed);
        let [alpha_skill] = &core_agent_skills(&paths, ClashCore::MeowAlpha).await[..] else {
            panic!("Meow Alpha ships one skill");
        };
        assert_eq!(alpha_skill.dir, alpha.as_str());
        assert!(alpha_skill.installed);
    }
}
