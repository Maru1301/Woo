use crate::{
    diff::{DiffFile, DiffLineKind},
    error::AppError,
};
use serde::Deserialize;
use std::collections::BTreeSet;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartialSelection {
    pub revision: String,
    pub hunk_index: usize,
    /// None selects the complete hunk. Some selects changed-line positions.
    pub line_indices: Option<Vec<usize>>,
}

fn invalid_selection() -> AppError {
    AppError::new(
        "invalid_selection",
        "Choose a current hunk or changed lines.",
    )
}

/// Preserve Git's file headers and transform only one selected hunk. The caller
/// must compare the full patch revision under the repository mutation lock.
pub fn build_patch(
    raw: &[u8],
    diff: &DiffFile,
    selection: &PartialSelection,
    reverse: bool,
) -> Result<Vec<u8>, AppError> {
    if diff.revision != selection.revision {
        return Err(AppError::new(
            "stale_diff",
            "The diff changed. Review the refreshed changes and try again.",
        ));
    }
    if diff.is_binary || selection.hunk_index >= diff.hunks.len() {
        return Err(invalid_selection());
    }
    let source = std::str::from_utf8(raw).map_err(|_| invalid_selection())?;
    let lines: Vec<&str> = source.split_inclusive('\n').collect();
    let starts: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter_map(|(i, line)| line.starts_with("@@ ").then_some(i))
        .collect();
    if starts.len() != diff.hunks.len() {
        return Err(invalid_selection());
    }
    let hunk_start = starts[selection.hunk_index];
    let hunk_end = starts
        .get(selection.hunk_index + 1)
        .copied()
        .unwrap_or(lines.len());
    let mut patch = lines[..starts[0]].concat();
    if let Some(indices) = &selection.line_indices {
        let hunk = &diff.hunks[selection.hunk_index];
        let chosen: BTreeSet<usize> = indices.iter().copied().collect();
        if chosen.is_empty()
            || chosen.len() != indices.len()
            || chosen.iter().any(|index| {
                !matches!(
                    hunk.lines.get(*index).map(|line| &line.kind),
                    Some(DiffLineKind::Addition | DiffLineKind::Deletion)
                )
            })
        {
            return Err(invalid_selection());
        }
        let mut body = String::new();
        let (mut old_count, mut new_count) = (0u32, 0u32);
        let mut cursor = 0;
        while cursor < hunk.lines.len() {
            if matches!(hunk.lines[cursor].kind, DiffLineKind::Context) {
                body.push(' ');
                body.push_str(&hunk.lines[cursor].content);
                body.push('\n');
                old_count += 1;
                new_count += 1;
                cursor += 1;
                continue;
            }
            let mut deleted = Vec::new();
            let mut added = Vec::new();
            while cursor < hunk.lines.len()
                && !matches!(hunk.lines[cursor].kind, DiffLineKind::Context)
            {
                match hunk.lines[cursor].kind {
                    DiffLineKind::Deletion => deleted.push((cursor, false)),
                    DiffLineKind::Addition => added.push((cursor, false)),
                    DiffLineKind::NoNewline => {
                        if let Some(last) = added.last_mut().or_else(|| deleted.last_mut()) {
                            last.1 = true;
                        }
                    }
                    DiffLineKind::Context => unreachable!(),
                }
                cursor += 1;
            }
            // Git groups deletions before additions. Pair positions in that
            // block so a selected replacement stays before later old lines.
            for position in 0..deleted.len().max(added.len()) {
                if let Some((index, no_newline)) = deleted.get(position) {
                    let selected = chosen.contains(index);
                    if selected || !reverse {
                        body.push(if selected { '-' } else { ' ' });
                        body.push_str(&hunk.lines[*index].content);
                        body.push('\n');
                        if *no_newline {
                            body.push_str("\\ No newline at end of file\n");
                        }
                        old_count += 1;
                        if !selected {
                            new_count += 1;
                        }
                    }
                }
                if let Some((index, no_newline)) = added.get(position) {
                    let selected = chosen.contains(index);
                    if selected || reverse {
                        body.push(if selected { '+' } else { ' ' });
                        body.push_str(&hunk.lines[*index].content);
                        body.push('\n');
                        if *no_newline {
                            body.push_str("\\ No newline at end of file\n");
                        }
                        new_count += 1;
                        if !selected {
                            old_count += 1;
                        }
                    }
                }
            }
        }
        patch.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            hunk.old_start, old_count, hunk.new_start, new_count
        ));
        patch.push_str(&body);
    } else {
        patch.push_str(&lines[hunk_start..hunk_end].concat());
    }
    Ok(patch.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        diff::parse_patch,
        status::{ChangeKind, FileChange},
    };

    #[test]
    fn transforms_mixed_lines_and_preserves_whitespace() {
        let raw = b"diff --git a/a b/a\nindex 123..456 100644\n--- a/a\n+++ b/a\n@@ -1,3 +1,4 @@\n before\n-old\n+new\n+  spaced\t\n after\n";
        let diff = parse_patch(
            raw,
            FileChange {
                path: "a".into(),
                old_path: None,
                kind: ChangeKind::Modified,
            },
        )
        .unwrap();
        let selected = PartialSelection {
            revision: diff.revision.clone(),
            hunk_index: 0,
            line_indices: Some(vec![1, 3]),
        };
        let patch = String::from_utf8(build_patch(raw, &diff, &selected, false).unwrap()).unwrap();
        assert!(
            patch.contains("@@ -1,3 +1,3 @@\n before\n-old\n+  spaced\t\n after\n"),
            "{patch:?}"
        );
    }

    #[test]
    fn rejects_stale_duplicate_and_context_selection() {
        let raw = b"diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +1 @@\n-old\n+new\n";
        let diff = parse_patch(
            raw,
            FileChange {
                path: "a".into(),
                old_path: None,
                kind: ChangeKind::Modified,
            },
        )
        .unwrap();
        let mut selected = PartialSelection {
            revision: "old".into(),
            hunk_index: 0,
            line_indices: None,
        };
        assert_eq!(
            build_patch(raw, &diff, &selected, false).unwrap_err().code,
            "stale_diff"
        );
        selected.revision = diff.revision.clone();
        selected.line_indices = Some(vec![0, 0]);
        assert_eq!(
            build_patch(raw, &diff, &selected, false).unwrap_err().code,
            "invalid_selection"
        );
    }
}
