use crate::PatchBlock;
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
