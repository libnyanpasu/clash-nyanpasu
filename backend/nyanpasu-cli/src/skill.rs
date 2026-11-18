//! `skill path` and `skill install`: find the shipped agent skill folder and link it
//! into an agent's skill directory.
//!
//! The skill ships as a bundle resource, following the core agent skill precedent: the
//! app reports the folder and agents link it, so an app update refreshes the skill in
//! place. Linking, not copying, is what keeps it fresh; a copy is only the Windows
//! fallback when symlinks are not permitted.

use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    cli::Agent,
    error::CliError,
    placement::{Placed, copy_directory, io_failure, place_link},
};

/// The skill's folder name, which is also the CLI's executable name.
pub const SKILL_NAME: &str = "nyanpasu-cli";

/// Where each bundle keeps the skill, relative to the directory of the executable:
/// next to it on Windows and in portable archives, in `Contents/Resources` in a macOS app,
/// and in the product's `lib` directory in deb, rpm and AppImage layouts. The first folder
/// holding a `SKILL.md` wins.
fn candidates(executable_dir: &Path) -> Vec<PathBuf> {
    [
        "skills",
        "../Resources/skills",
        "../lib/Clash Nyanpasu/skills",
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
    if let Some(exec) = executable {
        if let Some(dir) = exec.parent() {
            searched.extend(candidates(dir));
        }
        if let Ok(resolved) = exec.canonicalize()
            && resolved != exec
            && let Some(dir) = resolved.parent()
        {
            searched.extend(candidates(dir));
        }
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
    fs::create_dir_all(agent_directory)
        .map_err(|error| io_failure("create", agent_directory, error))?;
    let destination = agent_directory.join(SKILL_NAME);
    match place_link(source, &destination, force)? {
        Placed::Linked => Ok(InstallOutcome::Linked(destination)),
        Placed::AlreadyLinked => Ok(InstallOutcome::AlreadyLinked(destination)),
        Placed::Refused(_) if cfg!(windows) => {
            copy_directory(source, &destination)
                .map_err(|error| io_failure("copy the skill to", &destination, error))?;
            Ok(InstallOutcome::Copied(destination))
        }
        Placed::Refused(error) => Err(io_failure("link", &destination, error)),
    }
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

    /// Splits a shell-style command example, preserving single-quoted tokens and
    /// unescaping Markdown table `\|` pipes.
    fn tokenize_command(span: &str) -> Result<Vec<String>, String> {
        let mut tokens = Vec::new();
        let mut current = String::new();
        let mut in_single_quotes = false;
        let mut chars = span.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '\'' => in_single_quotes = !in_single_quotes,
                '\\' if chars.peek() == Some(&'|') => {
                    chars.next();
                    current.push('|');
                }
                c if c.is_whitespace() && !in_single_quotes => {
                    if !current.is_empty() {
                        tokens.push(std::mem::take(&mut current));
                    }
                }
                c => current.push(c),
            }
        }
        if in_single_quotes {
            return Err(format!("`{span}`: unterminated single quote"));
        }
        if !current.is_empty() {
            tokens.push(current);
        }
        Ok(tokens)
    }

    fn expand_token(token: &str) -> Vec<String> {
        if let Some(inner) = token.strip_prefix('<').and_then(|s| s.strip_suffix('>')) {
            if inner.contains('|') {
                return inner.split('|').map(str::to_owned).collect();
            }
            if inner.to_ascii_lowercase().contains("json") {
                return vec!["{}".to_owned()];
            }
            return vec!["placeholder".to_owned()];
        }
        vec![token.to_owned()]
    }

    /// Parses a documented command with the real `clap` grammar and `Command::plan`.
    fn check_command(span: &str) -> Result<(), String> {
        use clap::Parser as _;

        let tokens = tokenize_command(span)?;
        let Some((program, args)) = tokens.split_first() else {
            return Err(format!("`{span}`: empty command"));
        };
        if program != SKILL_NAME {
            return Err(format!(
                "`{span}`: expected `{SKILL_NAME}`, found `{program}`"
            ));
        }

        let mut invocations = vec![vec![program.clone()]];
        for token in args {
            let choices = expand_token(token);
            let mut next = Vec::with_capacity(invocations.len() * choices.len());
            for prefix in &invocations {
                for choice in &choices {
                    let mut candidate = prefix.clone();
                    candidate.push(choice.clone());
                    next.push(candidate);
                }
            }
            invocations = next;
        }

        for argv in invocations {
            let parsed = crate::cli::Cli::try_parse_from(&argv)
                .map_err(|error| format!("`{span}` ({argv:?}): {error}"))?;
            parsed
                .command
                .plan()
                .map_err(|error| format!("`{span}` ({argv:?}): {error}"))?;
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
        assert!(check_command("nyanpasu-cli logs tail --nonexistent").is_err());
        assert!(check_command("nyanpasu-cli mode set invalid").is_err());
        assert!(check_command("nyanpasu-cli mode").is_err());
    }

    #[test]
    fn the_skill_covers_every_top_level_command_group() {
        use clap::CommandFactory as _;

        let text: String = SKILL_FILES.iter().map(|(_, text)| *text).collect();
        for command in crate::cli::Cli::command().get_subcommands() {
            let name = command.get_name();
            if matches!(
                name,
                "help" | "context" | "install" | "skill" | "completion"
            ) {
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
        let lower = skill.to_lowercase();
        assert!(skill.starts_with("---\nname: nyanpasu-cli\ndescription: "));
        for rule in [
            "--json",
            "status",
            "methods",
            "confirm",
            "exit code 6",
            "never create tokens",
            "remote-access settings",
            "never put a token",
        ] {
            assert!(lower.contains(rule), "SKILL.md lacks the `{rule}` rule");
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
    fn locate_finds_macos_linux_and_windows_bundle_layouts() {
        let root = tempfile::tempdir().unwrap();

        let mac_root = root.path().join("mac/Clash Nyanpasu.app/Contents");
        let mac_skill = mac_root.join("Resources/skills").join(SKILL_NAME);
        fs::create_dir_all(mac_root.join("MacOS")).unwrap();
        fs::create_dir_all(&mac_skill).unwrap();
        fs::write(mac_skill.join("SKILL.md"), "mac").unwrap();
        assert_eq!(
            locate(None, Some(&mac_root.join("MacOS/nyanpasu-cli"))).unwrap(),
            mac_skill.canonicalize().unwrap()
        );

        let linux_root = root.path().join("linux/usr");
        let linux_skill = linux_root
            .join("lib/Clash Nyanpasu/skills")
            .join(SKILL_NAME);
        fs::create_dir_all(linux_root.join("bin")).unwrap();
        fs::create_dir_all(&linux_skill).unwrap();
        fs::write(linux_skill.join("SKILL.md"), "linux").unwrap();
        assert_eq!(
            locate(None, Some(&linux_root.join("bin/nyanpasu-cli"))).unwrap(),
            linux_skill.canonicalize().unwrap()
        );

        let win_root = root.path().join("win");
        let win_skill = win_root.join("skills").join(SKILL_NAME);
        fs::create_dir_all(&win_skill).unwrap();
        fs::write(win_skill.join("SKILL.md"), "win").unwrap();
        assert_eq!(
            locate(None, Some(&win_root.join("nyanpasu-cli.exe"))).unwrap(),
            win_skill.canonicalize().unwrap()
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

    #[test]
    fn the_windows_fallback_copies_the_whole_skill_folder() {
        let root = tempfile::tempdir().unwrap();
        let source = skill_folder(root.path());
        fs::create_dir_all(source.join("references")).unwrap();
        fs::write(source.join("references/commands.md"), "commands").unwrap();

        let destination = root.path().join("agent/nyanpasu-cli");
        copy_directory(&source, &destination).unwrap();
        assert_eq!(
            fs::read_to_string(destination.join("SKILL.md")).unwrap(),
            "skill"
        );
        assert_eq!(
            fs::read_to_string(destination.join("references/commands.md")).unwrap(),
            "commands"
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

    #[cfg(unix)]
    #[test]
    fn a_link_whose_app_is_gone_is_replaced_without_force() {
        let root = tempfile::tempdir().unwrap();
        let source = skill_folder(root.path());
        let agent = root.path().join("home/.codex/skills");
        fs::create_dir_all(&agent).unwrap();
        let destination = agent.join(SKILL_NAME);
        std::os::unix::fs::symlink(root.path().join("moved/skills/nyanpasu-cli"), &destination)
            .unwrap();

        assert_eq!(
            install(&source, &agent, false).unwrap(),
            InstallOutcome::Linked(destination.clone())
        );
        assert_eq!(fs::read_link(&destination).unwrap(), source);
    }

    #[cfg(unix)]
    #[test]
    fn locate_follows_symlinked_executable() {
        let root = tempfile::tempdir().unwrap();
        let mac_root = root.path().join("Clash Nyanpasu.app/Contents");
        let mac_bin = mac_root.join("MacOS/nyanpasu-cli");
        let mac_skill = mac_root.join("Resources/skills").join(SKILL_NAME);
        fs::create_dir_all(mac_bin.parent().unwrap()).unwrap();
        fs::create_dir_all(&mac_skill).unwrap();
        fs::write(&mac_bin, "bin").unwrap();
        fs::write(mac_skill.join("SKILL.md"), "mac").unwrap();

        let usr_bin = root.path().join("usr/local/bin/nyanpasu-cli");
        fs::create_dir_all(usr_bin.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&mac_bin, &usr_bin).unwrap();

        assert_eq!(
            locate(None, Some(&usr_bin)).unwrap(),
            mac_skill.canonicalize().unwrap()
        );
    }
}
