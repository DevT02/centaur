use crate::PatchBlock;
use crate::patch::MemoryPatchPlan;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchFileSummary {
    pub file_path: String,
    pub block_count: usize,
    pub removed_lines: usize,
    pub added_lines: usize,
    pub creates_file: bool,
}

pub fn summarize_patch_blocks(blocks: &[PatchBlock]) -> Vec<PatchFileSummary> {
    let mut grouped: BTreeMap<String, PatchFileSummary> = BTreeMap::new();

    for block in blocks {
        let summary = grouped
            .entry(block.file_path.clone())
            .or_insert_with(|| PatchFileSummary {
                file_path: block.file_path.clone(),
                block_count: 0,
                removed_lines: 0,
                added_lines: 0,
                creates_file: false,
            });

        summary.block_count += 1;
        summary.removed_lines += block.search.lines().count();
        summary.added_lines += block.replace.lines().count();
        summary.creates_file |= block.search.is_empty();
    }

    grouped.into_values().collect()
}

#[derive(Clone, Copy)]
enum DiffLine<'a> {
    Context(&'a str),
    Removed(&'a str),
    Added(&'a str),
}

/// Render the exact in-memory result that will be handed to the transactional
/// writer. The line diff is computed after trimming the common prefix/suffix,
/// keeping the dynamic-programming table proportional to the changed region.
pub fn render_patch_plan(plans: &[MemoryPatchPlan]) -> String {
    let mut out = String::new();
    for plan in plans {
        let before = plan.original_content.as_deref().unwrap_or("");
        let path = &plan.block.file_path;
        out.push_str(&format!("diff --centaur {}\n", path));
        if plan.is_new_file {
            out.push_str("--- /dev/null\n");
        } else {
            out.push_str(&format!("--- a/{}\n", path));
        }
        out.push_str(&format!("+++ b/{}\n", path));
        render_line_hunks(&mut out, before, &plan.new_content);
        render_text_metadata(&mut out, before, &plan.new_content);
    }
    out
}

fn render_text_metadata(out: &mut String, before: &str, after: &str) {
    let before_bom = before.starts_with('\u{FEFF}');
    let after_bom = after.starts_with('\u{FEFF}');
    let before_end = before.ends_with('\n');
    let after_end = after.ends_with('\n');
    let before_endings = line_ending_style(before);
    let after_endings = line_ending_style(after);

    if before_bom != after_bom || before_end != after_end || before_endings != after_endings {
        out.push_str("@@ text metadata @@\n");
    }
    if before_bom != after_bom {
        out.push_str(&format!(
            "-UTF-8 BOM: {}\n+UTF-8 BOM: {}\n",
            if before_bom { "present" } else { "absent" },
            if after_bom { "present" } else { "absent" }
        ));
    }
    if before_endings != after_endings {
        out.push_str(&format!(
            "-Line endings: {}\n+Line endings: {}\n",
            before_endings, after_endings
        ));
    }
    if before_end != after_end {
        out.push_str(&format!(
            "-Final newline: {}\n+Final newline: {}\n",
            if before_end { "present" } else { "absent" },
            if after_end { "present" } else { "absent" }
        ));
    }
}

fn line_ending_style(content: &str) -> &'static str {
    let crlf = content.matches("\r\n").count();
    let lf = content.matches('\n').count();
    match (crlf, lf.saturating_sub(crlf)) {
        (0, 0) => "none",
        (0, _) => "LF",
        (_, 0) => "CRLF",
        _ => "mixed",
    }
}

fn render_line_hunks(out: &mut String, before: &str, after: &str) {
    let before_lines: Vec<&str> = before.lines().collect();
    let after_lines: Vec<&str> = after.lines().collect();

    let mut prefix = 0;
    while prefix < before_lines.len()
        && prefix < after_lines.len()
        && before_lines[prefix] == after_lines[prefix]
    {
        prefix += 1;
    }

    let mut suffix = 0;
    while suffix < before_lines.len().saturating_sub(prefix)
        && suffix < after_lines.len().saturating_sub(prefix)
        && before_lines[before_lines.len() - 1 - suffix]
            == after_lines[after_lines.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let before_middle = &before_lines[prefix..before_lines.len() - suffix];
    let after_middle = &after_lines[prefix..after_lines.len() - suffix];
    let mut operations = Vec::with_capacity(before_lines.len() + after_lines.len());
    operations.extend(
        before_lines[..prefix]
            .iter()
            .copied()
            .map(DiffLine::Context),
    );
    operations.extend(diff_middle(before_middle, after_middle));
    operations.extend(
        before_lines[before_lines.len() - suffix..]
            .iter()
            .copied()
            .map(DiffLine::Context),
    );

    let changed: Vec<usize> = operations
        .iter()
        .enumerate()
        .filter_map(|(index, line)| (!matches!(line, DiffLine::Context(_))).then_some(index))
        .collect();
    if changed.is_empty() {
        out.push_str("(no textual change)\n");
        return;
    }

    let mut old_positions = Vec::with_capacity(operations.len());
    let mut new_positions = Vec::with_capacity(operations.len());
    let (mut old_line, mut new_line) = (1_usize, 1_usize);
    for operation in &operations {
        old_positions.push(old_line);
        new_positions.push(new_line);
        match operation {
            DiffLine::Context(_) => {
                old_line += 1;
                new_line += 1;
            }
            DiffLine::Removed(_) => old_line += 1,
            DiffLine::Added(_) => new_line += 1,
        }
    }

    let mut groups = Vec::new();
    let mut first_change = changed[0];
    let mut last_change = changed[0];
    for &index in &changed[1..] {
        if index > last_change + 7 {
            groups.push((
                first_change.saturating_sub(3),
                (last_change + 4).min(operations.len()),
            ));
            first_change = index;
        }
        last_change = index;
    }
    groups.push((
        first_change.saturating_sub(3),
        (last_change + 4).min(operations.len()),
    ));

    for (start, end) in groups {
        let hunk = &operations[start..end];
        let old_count = hunk
            .iter()
            .filter(|line| !matches!(line, DiffLine::Added(_)))
            .count();
        let new_count = hunk
            .iter()
            .filter(|line| !matches!(line, DiffLine::Removed(_)))
            .count();
        let old_start = if old_count == 0 {
            old_positions[start].saturating_sub(1)
        } else {
            old_positions[start]
        };
        let new_start = if new_count == 0 {
            new_positions[start].saturating_sub(1)
        } else {
            new_positions[start]
        };
        out.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            old_start, old_count, new_start, new_count
        ));
        for line in hunk {
            match line {
                DiffLine::Context(text) => out.push_str(&format!(" {}\n", text)),
                DiffLine::Removed(text) => out.push_str(&format!("-{}\n", text)),
                DiffLine::Added(text) => out.push_str(&format!("+{}\n", text)),
            }
        }
    }
}

fn diff_middle<'a>(before: &[&'a str], after: &[&'a str]) -> Vec<DiffLine<'a>> {
    const MAX_LCS_CELLS: usize = 4_000_000;
    if before.len().saturating_mul(after.len()) > MAX_LCS_CELLS {
        return before
            .iter()
            .copied()
            .map(DiffLine::Removed)
            .chain(after.iter().copied().map(DiffLine::Added))
            .collect();
    }

    let columns = after.len() + 1;
    let mut lengths = vec![0_u32; (before.len() + 1) * columns];
    for old in (0..before.len()).rev() {
        for new in (0..after.len()).rev() {
            let index = old * columns + new;
            lengths[index] = if before[old] == after[new] {
                lengths[(old + 1) * columns + new + 1] + 1
            } else {
                lengths[(old + 1) * columns + new].max(lengths[old * columns + new + 1])
            };
        }
    }

    let (mut old, mut new) = (0, 0);
    let mut operations = Vec::with_capacity(before.len() + after.len());
    while old < before.len() && new < after.len() {
        if before[old] == after[new] {
            operations.push(DiffLine::Context(before[old]));
            old += 1;
            new += 1;
        } else if lengths[(old + 1) * columns + new] >= lengths[old * columns + new + 1] {
            operations.push(DiffLine::Removed(before[old]));
            old += 1;
        } else {
            operations.push(DiffLine::Added(after[new]));
            new += 1;
        }
    }
    operations.extend(before[old..].iter().copied().map(DiffLine::Removed));
    operations.extend(after[new..].iter().copied().map(DiffLine::Added));
    operations
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patch::plan_blocks_transactional;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn groups_multiple_blocks_for_the_same_file() {
        let blocks = vec![
            PatchBlock {
                file_path: "src/main.rs".to_string(),
                search: "old line".to_string(),
                replace: "new line".to_string(),
            },
            PatchBlock {
                file_path: "src/main.rs".to_string(),
                search: "old one\nold two".to_string(),
                replace: "new one\nnew two\nnew three".to_string(),
            },
            PatchBlock {
                file_path: "src/new.rs".to_string(),
                search: String::new(),
                replace: "pub fn created() {}".to_string(),
            },
        ];

        let summaries = summarize_patch_blocks(&blocks);

        assert_eq!(summaries.len(), 2);
        assert_eq!(
            summaries[0],
            PatchFileSummary {
                file_path: "src/main.rs".to_string(),
                block_count: 2,
                removed_lines: 3,
                added_lines: 4,
                creates_file: false,
            }
        );
        assert!(summaries[1].creates_file);
        assert_eq!(summaries[1].added_lines, 1);
    }

    #[test]
    fn renders_the_exact_planned_before_and_after_lines() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("code.rs"), "one\nold\nthree\n").unwrap();
        let blocks = vec![PatchBlock {
            file_path: "code.rs".to_string(),
            search: "old".to_string(),
            replace: "new".to_string(),
        }];

        let plans = plan_blocks_transactional(dir.path(), &blocks).unwrap();
        let diff = render_patch_plan(&plans);

        assert!(diff.contains("--- a/code.rs\n+++ b/code.rs"), "{diff}");
        assert!(diff.contains("-old\n+new"), "{diff}");
        assert!(diff.contains(" one\n"), "{diff}");
    }

    #[test]
    fn renders_line_ending_and_final_newline_changes() {
        let mut out = String::new();
        render_line_hunks(&mut out, "same\r\n", "same");
        render_text_metadata(&mut out, "same\r\n", "same");

        assert!(out.contains("Line endings: CRLF"), "{out}");
        assert!(out.contains("Line endings: none"), "{out}");
        assert!(out.contains("Final newline: absent"), "{out}");
    }
}
