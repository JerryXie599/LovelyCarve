use super::types::{GitDiffHunk, GitDiffLine};

const MAX_DIFF_LINES_PER_SIDE: usize = 3000;
const CONTEXT_LINES: usize = 3;

#[derive(Clone, Copy, PartialEq, Eq)]
enum EditOp {
    Equal(usize, usize),
    Delete(usize),
    Insert(usize),
}

pub fn compute_unified_hunks(old_text: &str, new_text: &str) -> (Vec<GitDiffHunk>, usize, usize) {
    let old_lines: Vec<&str> = split_diff_lines(old_text);
    let new_lines: Vec<&str> = split_diff_lines(new_text);

    if old_lines.len() > MAX_DIFF_LINES_PER_SIDE || new_lines.len() > MAX_DIFF_LINES_PER_SIDE {
        let old_cap = old_lines.len().min(MAX_DIFF_LINES_PER_SIDE);
        let new_cap = new_lines.len().min(MAX_DIFF_LINES_PER_SIDE);
        return build_hunks_from_slices(&old_lines[..old_cap], &new_lines[..new_cap], true);
    }

    build_hunks_from_slices(&old_lines, &new_lines, false)
}

fn split_diff_lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<&str> = text
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    if text.ends_with('\n') {
        lines.pop();
    }
    lines
}

fn build_hunks_from_slices(
    old_lines: &[&str],
    new_lines: &[&str],
    truncated: bool,
) -> (Vec<GitDiffHunk>, usize, usize) {
    let ops = lcs_diff(old_lines, new_lines);
    let mut additions = 0usize;
    let mut deletions = 0usize;
    for op in &ops {
        match op {
            EditOp::Insert(_) => additions += 1,
            EditOp::Delete(_) => deletions += 1,
            EditOp::Equal(_, _) => {}
        }
    }

    let change_indices: Vec<usize> = ops
        .iter()
        .enumerate()
        .filter_map(|(idx, op)| (!matches!(op, EditOp::Equal(_, _))).then_some(idx))
        .collect();

    if change_indices.is_empty() {
        return (Vec::new(), 0, 0);
    }

    let mut groups: Vec<(usize, usize)> = Vec::new();
    for idx in change_indices {
        let start = idx.saturating_sub(CONTEXT_LINES);
        let end = (idx + CONTEXT_LINES).min(ops.len().saturating_sub(1));
        if let Some(last) = groups.last_mut() {
            if start <= last.1 + 1 {
                last.1 = last.1.max(end);
                continue;
            }
        }
        groups.push((start, end));
    }

    let mut hunks = Vec::with_capacity(groups.len());
    for (g_idx, (start, end)) in groups.into_iter().enumerate() {
        let mut lines = Vec::new();
        let mut first_old: Option<usize> = None;
        let mut first_new: Option<usize> = None;
        let mut old_count = 0usize;
        let mut new_count = 0usize;

        for op in &ops[start..=end] {
            match *op {
                EditOp::Equal(oi, ni) => {
                    first_old.get_or_insert(oi + 1);
                    first_new.get_or_insert(ni + 1);
                    old_count += 1;
                    new_count += 1;
                    lines.push(GitDiffLine {
                        kind: "context".to_owned(),
                        old_line: Some(oi + 1),
                        new_line: Some(ni + 1),
                        content: old_lines[oi].to_owned(),
                    });
                }
                EditOp::Delete(oi) => {
                    first_old.get_or_insert(oi + 1);
                    old_count += 1;
                    lines.push(GitDiffLine {
                        kind: "delete".to_owned(),
                        old_line: Some(oi + 1),
                        new_line: None,
                        content: old_lines[oi].to_owned(),
                    });
                }
                EditOp::Insert(ni) => {
                    first_new.get_or_insert(ni + 1);
                    new_count += 1;
                    lines.push(GitDiffLine {
                        kind: "add".to_owned(),
                        old_line: None,
                        new_line: Some(ni + 1),
                        content: new_lines[ni].to_owned(),
                    });
                }
            }
        }

        let old_start = first_old.unwrap_or(0);
        let new_start = first_new.unwrap_or(0);
        let suffix = if truncated && g_idx == 0 {
            " (large file truncated)"
        } else {
            ""
        };
        let header = format!("@@ -{old_start},{old_count} +{new_start},{new_count} @@{suffix}");
        hunks.push(GitDiffHunk {
            old_start,
            old_lines: old_count,
            new_start,
            new_lines: new_count,
            header,
            lines,
        });
    }

    (hunks, additions, deletions)
}

fn lcs_diff(old_lines: &[&str], new_lines: &[&str]) -> Vec<EditOp> {
    // Strip common prefix and suffix to keep DP matrix minimal
    let mut prefix = 0usize;
    while prefix < old_lines.len()
        && prefix < new_lines.len()
        && old_lines[prefix] == new_lines[prefix]
    {
        prefix += 1;
    }

    let mut suffix = 0usize;
    while prefix + suffix < old_lines.len()
        && prefix + suffix < new_lines.len()
        && old_lines[old_lines.len() - 1 - suffix] == new_lines[new_lines.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let mid_old = &old_lines[prefix..old_lines.len() - suffix];
    let mid_new = &new_lines[prefix..new_lines.len() - suffix];
    let m = mid_old.len();
    let n = mid_new.len();

    let mut ops = Vec::with_capacity(old_lines.len() + new_lines.len());
    for i in 0..prefix {
        ops.push(EditOp::Equal(i, i));
    }

    // Fallback to simple block replacement if middle matrix exceeds memory guard (~4M cells)
    if m.saturating_mul(n) > 4_000_000 {
        for i in 0..m {
            ops.push(EditOp::Delete(prefix + i));
        }
        for j in 0..n {
            ops.push(EditOp::Insert(prefix + j));
        }
    } else if m > 0 || n > 0 {
        let cols = n + 1;
        let mut dp = vec![0u32; (m + 1) * cols];
        for i in (0..m).rev() {
            for j in (0..n).rev() {
                dp[i * cols + j] = if mid_old[i] == mid_new[j] {
                    dp[(i + 1) * cols + (j + 1)] + 1
                } else {
                    dp[(i + 1) * cols + j].max(dp[i * cols + (j + 1)])
                };
            }
        }
        let mut i = 0usize;
        let mut j = 0usize;
        while i < m && j < n {
            if mid_old[i] == mid_new[j] {
                ops.push(EditOp::Equal(prefix + i, prefix + j));
                i += 1;
                j += 1;
            } else if dp[(i + 1) * cols + j] >= dp[i * cols + (j + 1)] {
                ops.push(EditOp::Delete(prefix + i));
                i += 1;
            } else {
                ops.push(EditOp::Insert(prefix + j));
                j += 1;
            }
        }
        while i < m {
            ops.push(EditOp::Delete(prefix + i));
            i += 1;
        }
        while j < n {
            ops.push(EditOp::Insert(prefix + j));
            j += 1;
        }
    }

    let old_tail = old_lines.len() - suffix;
    let new_tail = new_lines.len() - suffix;
    for k in 0..suffix {
        ops.push(EditOp::Equal(old_tail + k, new_tail + k));
    }

    ops
}

pub fn looks_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(4096).any(|&b| b == 0)
}
