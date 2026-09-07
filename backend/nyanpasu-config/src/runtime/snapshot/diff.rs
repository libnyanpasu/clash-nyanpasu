//! Pure, bounded-effort YAML line comparisons for snapshot inspection.
use std::time::Duration;

use serde::Serialize;
use similar::{Algorithm, ChangeTag, TextDiff};

use super::ConfigSnapshot;

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct SnapshotDiffHunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    /// Unified diff lines, including their space, plus, or minus prefix.
    pub lines: Vec<String>,
}

impl ConfigSnapshot {
    /// Use this snapshot as the baseline; callers can reuse the target YAML.
    pub fn diff_yaml_to(
        &self,
        current_yaml: &str,
    ) -> Result<Vec<SnapshotDiffHunk>, serde_yaml_ng::Error> {
        Ok(yaml_diff(
            &serde_yaml_ng::to_string(&self.config)?,
            current_yaml,
        ))
    }
}

fn yaml_diff(before: &str, after: &str) -> Vec<SnapshotDiffHunk> {
    // A deadline bounds Myers' search effort on highly dissimilar inputs. It
    // can yield a coarser (still complete) diff; serialization/output is linear.
    let diff = TextDiff::configure()
        .algorithm(Algorithm::Myers)
        .timeout(Duration::from_millis(200))
        .diff_lines(before, after);
    diff.grouped_ops(3)
        .into_iter()
        .map(|ops| {
            let old = ops.first().unwrap().old_range().start..ops.last().unwrap().old_range().end;
            let new = ops.first().unwrap().new_range().start..ops.last().unwrap().new_range().end;
            SnapshotDiffHunk {
                old_start: (old.start + usize::from(!old.is_empty())) as u32,
                old_lines: old.len() as u32,
                new_start: (new.start + usize::from(!new.is_empty())) as u32,
                new_lines: new.len() as u32,
                lines: ops
                    .iter()
                    .flat_map(|op| diff.iter_changes(op))
                    .map(|change| {
                        let sign = match change.tag() {
                            ChangeTag::Equal => ' ',
                            ChangeTag::Delete => '-',
                            ChangeTag::Insert => '+',
                        };
                        format!("{sign}{}", change.value().trim_end_matches('\n'))
                    })
                    .collect(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_replacements_empty_inputs_and_unchanged_yaml() {
        assert!(yaml_diff("mode: rule\n", "mode: rule\n").is_empty());
        let replacement = yaml_diff("mode: rule\n", "mode: global\n");
        assert_eq!(replacement[0].lines, ["-mode: rule", "+mode: global"]);
        assert_eq!((replacement[0].old_start, replacement[0].new_start), (1, 1));
        let addition = yaml_diff("", "rules: []\n");
        assert_eq!((addition[0].old_start, addition[0].old_lines), (0, 0));
        assert_eq!(addition[0].lines, ["+rules: []"]);
        let deletion = yaml_diff("rules: []\n", "");
        assert_eq!((deletion[0].new_start, deletion[0].new_lines), (0, 0));
        assert_eq!(deletion[0].lines, ["-rules: []"]);
    }

    #[test]
    fn large_diff_preserves_all_changed_lines_and_context_offsets() {
        let before: String = (0..10_000)
            .map(|n| format!("- DOMAIN,host{n},DIRECT\n"))
            .collect();
        let after = before
            .replace("host10,", "changed10,")
            .replace("host9000,", "changed9000,");
        let start = std::time::Instant::now();
        let hunks = yaml_diff(&before, &after);
        eprintln!("10,000 lines, two edits: {:?}", start.elapsed());
        assert_eq!(hunks.len(), 2);
        assert_eq!((hunks[0].old_start, hunks[1].old_start), (8, 8998));
        assert_eq!(hunks[0].old_lines, 7);
        assert_eq!(hunks[0].new_lines, 7);
        let unrelated: String = (0..10_000)
            .map(|n| format!("- IP-CIDR,network{n},REJECT\n"))
            .collect();
        let start = std::time::Instant::now();
        let hunks = yaml_diff(&before, &unrelated);
        eprintln!("10,000 entirely changed lines: {:?}", start.elapsed());
        let removed: String = hunks
            .iter()
            .flat_map(|h| &h.lines)
            .filter_map(|l| l.strip_prefix('-'))
            .map(|l| format!("{l}\n"))
            .collect();
        let added: String = hunks
            .iter()
            .flat_map(|h| &h.lines)
            .filter_map(|l| l.strip_prefix('+'))
            .map(|l| format!("{l}\n"))
            .collect();
        assert_eq!(removed, before);
        assert_eq!(added, unrelated);
    }
}
