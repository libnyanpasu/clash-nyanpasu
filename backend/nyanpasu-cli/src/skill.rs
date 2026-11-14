//! `skill path` and `skill install`: find the shipped agent skill folder and link it
//! into an agent's skill directory.
//!
//! The skill ships as a bundle resource, following the core agent skill precedent: the
//! app reports the folder and agents link it, so an app update refreshes the skill in
//! place. Linking, not copying, is what keeps it fresh; a copy is only the Windows
//! fallback when symlinks are not permitted.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use crate::{cli::Agent, error::CliError};

/// The skill's folder name, which is also the CLI's executable name.
pub const SKILL_NAME: &str = "nyanpasu-cli";

/// Where the bundle may keep resources, relative to the executable's directory. The
/// packaging change fixes the real layout; the first folder holding a `SKILL.md` wins.
fn candidates(executable_dir: &Path) -> Vec<PathBuf> {
    [
        "skills",
        "resources/skills",
        "../Resources/skills",
        "../lib/clash-nyanpasu/skills",
        "../share/clash-nyanpasu/skills",
    ]
    .iter()
    .map(|relative| executable_dir.join(relative).join(SKILL_NAME))
    .collect()
}

/// Finds the skill folder: an explicit override, else next to the executable, else (in
/// development builds) the repository's `skills/` folder.
///
/// # Errors
/// Fails when no candidate holds a `SKILL.md`.
pub fn locate(override_dir: Option<&Path>, executable: Option<&Path>) -> Result<PathBuf, CliError> {
    let mut searched = Vec::new();
    if let Some(dir) = override_dir {
        searched.push(dir.to_owned());
    }
    if let Some(dir) = executable.and_then(Path::parent) {
        searched.extend(candidates(dir));
    }
    #[cfg(debug_assertions)]
    searched.push(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../skills")
            .join(SKILL_NAME),
    );
    searched
        .iter()
        .find(|dir| dir.join("SKILL.md").is_file())
        .map(|dir| dir.canonicalize().unwrap_or_else(|_| dir.clone()))
        .ok_or_else(|| {
            CliError::failed(format!(
                "the agent skill was not found; looked in: {}",
                searched
                    .iter()
                    .map(|dir| dir.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })
}

/// The skill directory of an agent, below the user's home directory.
pub fn agent_directory(home: &Path, agent: Agent) -> PathBuf {
    match agent {
        Agent::Claude => home.join(".claude").join("skills"),
        Agent::Codex => home.join(".codex").join("skills"),
        Agent::Agents => home.join(".agents").join("skills"),
    }
}

/// What `install` did.
#[derive(Debug, PartialEq, Eq)]
pub enum InstallOutcome {
    /// A new link now points at the skill folder.
    Linked(PathBuf),
    /// The link already pointed there.
    AlreadyLinked(PathBuf),
    /// Symlinks are not permitted, so the folder was copied and will not refresh itself.
    Copied(PathBuf),
}

#[cfg(unix)]
fn link(source: &Path, destination: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(source, destination)
}

#[cfg(windows)]
fn link(source: &Path, destination: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_dir(source, destination)
}

fn copy_directory(source: &Path, destination: &Path) -> io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_directory(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// Links `source` into `agent_directory` under the skill's name.
///
/// An existing entry is never overwritten, except that `force` replaces a symlink.
///
/// # Errors
/// Fails on a conflicting entry or a filesystem error.
pub fn install(
    source: &Path,
    agent_directory: &Path,
    force: bool,
) -> Result<InstallOutcome, CliError> {
    let io_failure = |action: &str, path: &Path, error: io::Error| {
        CliError::failed(format!("cannot {action} {}: {error}", path.display()))
    };
    fs::create_dir_all(agent_directory)
        .map_err(|error| io_failure("create", agent_directory, error))?;
    let destination = agent_directory.join(SKILL_NAME);
    match fs::symlink_metadata(&destination) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            if fs::read_link(&destination).is_ok_and(|current| current == source) {
                return Ok(InstallOutcome::AlreadyLinked(destination));
            }
            if !force {
                return Err(CliError::failed(format!(
                    "{} is a link to somewhere else; pass --force to replace it",
                    destination.display()
                )));
            }
            remove_link(&destination)
                .map_err(|error| io_failure("replace", &destination, error))?;
        }
        Ok(_) => {
            return Err(CliError::failed(format!(
                "{} already exists and is not a link; remove it first",
                destination.display()
            )));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_failure("inspect", &destination, error)),
    }
    match link(source, &destination) {
        Ok(()) => Ok(InstallOutcome::Linked(destination)),
        Err(_) if cfg!(windows) => {
            copy_directory(source, &destination)
                .map_err(|error| io_failure("copy the skill to", &destination, error))?;
            Ok(InstallOutcome::Copied(destination))
        }
        Err(error) => Err(io_failure("link", &destination, error)),
    }
}

#[cfg(unix)]
fn remove_link(path: &Path) -> io::Result<()> {
    fs::remove_file(path)
}

#[cfg(windows)]
fn remove_link(path: &Path) -> io::Result<()> {
    // A directory symlink is removed as a directory on Windows.
    fs::remove_dir(path).or_else(|_| fs::remove_file(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SKILL_FILES: [(&str, &str); 3] = [
        (
            "SKILL.md",
            include_str!("../../../skills/nyanpasu-cli/SKILL.md"),
        ),
        (
            "references/commands.md",
            include_str!("../../../skills/nyanpasu-cli/references/commands.md"),
        ),
        (
            "references/troubleshooting.md",
            include_str!("../../../skills/nyanpasu-cli/references/troubleshooting.md"),
        ),
    ];

    /// Inline code spans that start with the CLI's name.
    fn command_spans(markdown: &str) -> Vec<&str> {
        markdown
            .split('`')
            .skip(1)
            .step_by(2)
            .filter(|span| {
                let mut words = span.split_whitespace();
                words.next() == Some(SKILL_NAME) && words.next().is_some()
            })
            .collect()
    }

    /// Walks a documented command through the real grammar.
    fn check_command(span: &str) -> Result<(), String> {
        use clap::CommandFactory as _;

        let root = crate::cli::Cli::command();
        let value_flags: Vec<String> = root
            .get_arguments()
            .filter(|argument| argument.get_action().takes_values())
            .filter_map(|argument| argument.get_long().map(|long| format!("--{long}")))
            .collect();
        let mut current = &root;
        let mut tokens = span.split_whitespace().skip(1);
        while let Some(token) = tokens.next() {
            if token.starts_with('-') {
                if value_flags.iter().any(|flag| flag == token) {
                    tokens.next();
                }
                continue;
            }
            if !current.has_subcommands() {
                break;
            }
            if token.starts_with(['<', '\'', '[']) {
                return Err(format!("`{span}`: expected a subcommand, found `{token}`"));
            }
            current = current.find_subcommand(token).ok_or_else(|| {
                format!(
                    "`{span}`: `{token}` is not a subcommand of `{}`",
                    current.get_name()
                )
            })?;
        }
        if current.has_subcommands() {
            return Err(format!(
                "`{span}`: incomplete, `{}` needs a subcommand",
                current.get_name()
            ));
        }
        Ok(())
    }

    #[test]
    fn every_command_the_skill_names_exists_in_the_cli() {
        let mut checked = 0;
        for (file, text) in SKILL_FILES {
            for span in command_spans(text) {
                check_command(span).unwrap_or_else(|error| panic!("{file}: {error}"));
                checked += 1;
            }
        }
        assert!(checked >= 25, "the skill documents only {checked} commands");
    }

    #[test]
    fn the_skill_covers_every_top_level_command_group() {
        use clap::CommandFactory as _;

        let text: String = SKILL_FILES.iter().map(|(_, text)| *text).collect();
        for command in crate::cli::Cli::command().get_subcommands() {
            let name = command.get_name();
            if matches!(name, "help" | "mcp" | "context" | "skill" | "completion") {
                continue;
            }
            assert!(
                text.contains(&format!("{SKILL_NAME} {name}"))
                    || text.contains(&format!("{SKILL_NAME} --json {name}")),
                "the skill never mentions `{name}`"
            );
        }
    }

    #[test]
    fn the_skill_has_its_front_matter_and_the_rules_agents_need() {
        let skill = SKILL_FILES[0].1;
        assert!(skill.starts_with("---\nname: nyanpasu-cli\ndescription: "));
        for rule in ["--json", "confirm", "exit code 6", "never put a token"] {
            assert!(
                skill.to_lowercase().contains(rule),
                "SKILL.md lacks the `{rule}` rule"
            );
        }
        for (file, text) in SKILL_FILES {
            for code in 0..=6 {
                if file == "SKILL.md" {
                    assert!(
                        text.contains(&format!("| {code}    |")),
                        "exit code {code} is not documented"
                    );
                }
            }
        }
    }

    #[test]
    fn the_skill_folder_is_the_one_a_debug_build_finds() {
        let folder = locate(None, None).expect("the repository ships the skill");
        assert!(folder.join("SKILL.md").is_file());
        assert!(folder.join("references").is_dir());
    }

    fn skill_folder(root: &Path) -> PathBuf {
        let folder = root.join("bundle").join("skills").join(SKILL_NAME);
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("SKILL.md"), "skill").unwrap();
        folder
    }

    #[test]
    fn locate_prefers_the_override_then_the_bundle_next_to_the_executable() {
        let root = tempfile::tempdir().unwrap();
        let folder = skill_folder(root.path());
        let executable = root.path().join("bundle").join("nyanpasu-cli");

        assert_eq!(
            locate(None, Some(&executable)).unwrap(),
            folder.canonicalize().unwrap()
        );
        assert_eq!(
            locate(Some(&folder), None).unwrap(),
            folder.canonicalize().unwrap()
        );
    }

    #[test]
    fn locate_reports_every_place_it_looked() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("nowhere");
        let error = locate(Some(&missing), Some(&root.path().join("bin/nyanpasu-cli")));
        // Development builds also find the repository's own skill folder.
        if cfg!(debug_assertions) {
            assert!(
                error.is_ok(),
                "the repository skill is found in debug builds"
            );
        } else {
            assert!(error.unwrap_err().to_string().contains("nowhere"));
        }
    }

    #[test]
    fn agent_directories_live_under_home() {
        let home = Path::new("/home/u");
        assert_eq!(
            agent_directory(home, Agent::Claude),
            Path::new("/home/u/.claude/skills")
        );
        assert_eq!(
            agent_directory(home, Agent::Codex),
            Path::new("/home/u/.codex/skills")
        );
    }

    #[cfg(unix)]
    #[test]
    fn install_links_once_and_never_overwrites_foreign_entries() {
        let root = tempfile::tempdir().unwrap();
        let source = skill_folder(root.path());
        let agent = root.path().join("home").join(".claude").join("skills");

        let destination = agent.join(SKILL_NAME);
        assert_eq!(
            install(&source, &agent, false).unwrap(),
            InstallOutcome::Linked(destination.clone())
        );
        assert_eq!(fs::read_link(&destination).unwrap(), source);
        assert_eq!(
            install(&source, &agent, false).unwrap(),
            InstallOutcome::AlreadyLinked(destination.clone())
        );

        let other = root.path().join("other");
        fs::create_dir_all(&other).unwrap();
        fs::remove_file(&destination).unwrap();
        std::os::unix::fs::symlink(&other, &destination).unwrap();
        assert!(install(&source, &agent, false).is_err());
        assert_eq!(
            install(&source, &agent, true).unwrap(),
            InstallOutcome::Linked(destination.clone())
        );

        fs::remove_file(&destination).unwrap();
        fs::create_dir(&destination).unwrap();
        assert!(
            install(&source, &agent, true).is_err(),
            "a real directory is never replaced, even with --force"
        );
    }
}
