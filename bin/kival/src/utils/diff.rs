//! Unified text-diff generation for immutable object bodies.

use std::{collections::HashMap, ops::Range};

/// Maximum frontier states and line comparisons explored by the bounded Myers fallback.
///
/// The stored Myers trace contains at most roughly this many `usize` entries as well, keeping the
/// fallback's additional memory bounded while still handling large regions with a small edit
/// distance efficiently.
const MYERS_WORK_LIMIT: usize = 4_194_304;

/// A single logical text line with its terminating-newline state preserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct TextLine<'a> {
    /// Line contents excluding the terminating line-feed byte.
    text: &'a str,
    /// Whether the source line ended with a line-feed byte.
    terminated: bool,
}

/// A line-level edit operation in a generated diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditOp {
    /// A line shared by both inputs.
    Equal {
        /// Line index in the old input.
        old: usize,
        /// Line index in the new input.
        new: usize,
    },
    /// A line present only in the old input.
    Delete {
        /// Line index in the old input.
        old: usize,
    },
    /// A line present only in the new input.
    Insert {
        /// Line index in the new input.
        new: usize,
    },
}

impl EditOp {
    /// Returns the number of old-input lines consumed by this operation.
    const fn old_count(self) -> usize {
        match self {
            Self::Equal { .. } | Self::Delete { .. } => 1,
            Self::Insert { .. } => 0,
        }
    }

    /// Returns the number of new-input lines consumed by this operation.
    const fn new_count(self) -> usize {
        match self {
            Self::Equal { .. } | Self::Insert { .. } => 1,
            Self::Delete { .. } => 0,
        }
    }

    /// Returns whether this operation represents a changed line.
    const fn is_change(self) -> bool {
        !matches!(self, Self::Equal { .. })
    }
}

/// Unique-line occurrence information used to select patience-diff anchors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Occurrence {
    /// Unique line index, or `None` once the line occurs more than once.
    index: Option<usize>,
}

/// Builds a standards-compatible unified diff for two UTF-8 text bodies.
///
/// The returned string is empty when the bodies are identical. Changed output uses Git-style
/// `diff --git`, `---`, and `+++` headers followed by ordinary unified-diff hunks. `old_path` and
/// `new_path` are emitted verbatim and should therefore be caller-controlled synthetic paths.
#[must_use]
pub fn unified_diff(
    old: &str,
    new: &str,
    old_path: &str,
    new_path: &str,
    context: usize,
) -> String {
    let old_lines = split_lines(old);
    let new_lines = split_lines(new);
    let operations = build_edit_script(&old_lines, &new_lines);

    if !operations.iter().any(|operation| operation.is_change()) {
        return String::new();
    }

    let positions = operation_positions(&operations);
    let hunks = hunk_ranges(&operations, context);
    let mut output = String::new();
    output.push_str("diff --git ");
    output.push_str(old_path);
    output.push(' ');
    output.push_str(new_path);
    output.push('\n');
    output.push_str("--- ");
    output.push_str(old_path);
    output.push('\n');
    output.push_str("+++ ");
    output.push_str(new_path);
    output.push('\n');

    for hunk in &hunks {
        push_hunk(&mut output, &operations, &positions, &old_lines, &new_lines, hunk);
    }

    output
}

/// Splits text into logical lines while preserving whether each line ended in `\n`.
fn split_lines(input: &str) -> Vec<TextLine<'_>> {
    if input.is_empty() {
        return Vec::new();
    }

    input
        .split_inclusive('\n')
        .map(|line| {
            line.strip_suffix('\n').map_or(TextLine { text: line, terminated: false }, |text| {
                TextLine { text, terminated: true }
            })
        })
        .collect()
}

/// Produces a deterministic line-level edit script using patience anchors and bounded Myers for
/// anchorless regions.
fn build_edit_script(old: &[TextLine<'_>], new: &[TextLine<'_>]) -> Vec<EditOp> {
    let mut operations = Vec::new();
    diff_range(old, 0, old.len(), new, 0, new.len(), &mut operations);
    operations
}

/// Returns whether the selected old and new lines are identical.
fn lines_equal(
    old: &[TextLine<'_>],
    old_index: usize,
    new: &[TextLine<'_>],
    new_index: usize,
) -> bool {
    old[old_index] == new[new_index]
}

/// Diffs one pair of line ranges and appends operations in source order.
fn diff_range(
    old: &[TextLine<'_>],
    mut old_start: usize,
    old_end: usize,
    new: &[TextLine<'_>],
    mut new_start: usize,
    new_end: usize,
    operations: &mut Vec<EditOp>,
) {
    while old_start < old_end && new_start < new_end && lines_equal(old, old_start, new, new_start)
    {
        operations.push(EditOp::Equal { old: old_start, new: new_start });
        old_start += 1;
        new_start += 1;
    }

    let mut suffix = 0;
    while old_start + suffix < old_end
        && new_start + suffix < new_end
        && lines_equal(old, old_end - suffix - 1, new, new_end - suffix - 1)
    {
        suffix += 1;
    }

    let old_middle_end = old_end - suffix;
    let new_middle_end = new_end - suffix;

    if old_start == old_middle_end {
        for index in new_start..new_middle_end {
            operations.push(EditOp::Insert { new: index });
        }
    } else if new_start == new_middle_end {
        for index in old_start..old_middle_end {
            operations.push(EditOp::Delete { old: index });
        }
    } else {
        let anchors =
            patience_anchors(old, old_start, old_middle_end, new, new_start, new_middle_end);

        if anchors.is_empty() {
            fallback_diff_range(
                old,
                old_start,
                old_middle_end,
                new,
                new_start,
                new_middle_end,
                operations,
            );
        } else {
            let mut previous_old = old_start;
            let mut previous_new = new_start;

            for (old_anchor, new_anchor) in anchors {
                diff_range(
                    old,
                    previous_old,
                    old_anchor,
                    new,
                    previous_new,
                    new_anchor,
                    operations,
                );
                operations.push(EditOp::Equal { old: old_anchor, new: new_anchor });
                previous_old = old_anchor + 1;
                previous_new = new_anchor + 1;
            }

            diff_range(
                old,
                previous_old,
                old_middle_end,
                new,
                previous_new,
                new_middle_end,
                operations,
            );
        }
    }

    for offset in 0..suffix {
        operations
            .push(EditOp::Equal { old: old_middle_end + offset, new: new_middle_end + offset });
    }
}

/// Chooses ordered unique-line matches to use as patience-diff anchors.
fn patience_anchors(
    old: &[TextLine<'_>],
    old_start: usize,
    old_end: usize,
    new: &[TextLine<'_>],
    new_start: usize,
    new_end: usize,
) -> Vec<(usize, usize)> {
    let old_occurrences = line_occurrences(old, old_start, old_end);
    let new_occurrences = line_occurrences(new, new_start, new_end);
    let mut pairs = Vec::new();

    for (old_index, line) in old.iter().copied().enumerate().take(old_end).skip(old_start) {
        let Some(unique_old_index) = old_occurrences.get(&line).and_then(|entry| entry.index)
        else {
            continue;
        };
        if unique_old_index != old_index {
            continue;
        }
        let Some(new_index) = new_occurrences.get(&line).and_then(|entry| entry.index) else {
            continue;
        };
        pairs.push((old_index, new_index));
    }

    longest_increasing_pairs(&pairs)
}

/// Counts line occurrences within one diff range, retaining an index only for unique lines.
fn line_occurrences<'a>(
    lines: &[TextLine<'a>],
    start: usize,
    end: usize,
) -> HashMap<TextLine<'a>, Occurrence> {
    let mut occurrences = HashMap::new();

    for (index, line) in lines.iter().copied().enumerate().take(end).skip(start) {
        occurrences
            .entry(line)
            .and_modify(|occurrence: &mut Occurrence| occurrence.index = None)
            .or_insert(Occurrence { index: Some(index) });
    }

    occurrences
}

/// Returns the longest subsequence of pairs whose new-input indexes are strictly increasing.
fn longest_increasing_pairs(pairs: &[(usize, usize)]) -> Vec<(usize, usize)> {
    if pairs.is_empty() {
        return Vec::new();
    }

    let mut tails = Vec::<usize>::new();
    let mut previous = vec![None; pairs.len()];

    for (pair_index, &(_, new_index)) in pairs.iter().enumerate() {
        let position = tails.partition_point(|&tail_index| pairs[tail_index].1 < new_index);
        if position > 0 {
            previous[pair_index] = Some(tails[position - 1]);
        }
        if position == tails.len() {
            tails.push(pair_index);
        } else {
            tails[position] = pair_index;
        }
    }

    let mut selected = Vec::with_capacity(tails.len());
    let mut cursor = tails.last().copied();
    while let Some(index) = cursor {
        selected.push(pairs[index]);
        cursor = previous[index];
    }
    selected.reverse();
    selected
}

/// Diffs an anchorless range with bounded Myers, falling back to a whole-range replacement when
/// the work budget is exhausted.
fn fallback_diff_range(
    old: &[TextLine<'_>],
    old_start: usize,
    old_end: usize,
    new: &[TextLine<'_>],
    new_start: usize,
    new_end: usize,
    operations: &mut Vec<EditOp>,
) {
    if let Some(script) =
        myers_edit_script(old, old_start, old_end, new, new_start, new_end, MYERS_WORK_LIMIT)
    {
        operations.extend(script);
        return;
    }

    // Preserve a hard complexity bound for pathological inputs. A whole-range replacement is
    // always correct even when producing a less useful human-readable diff.
    for index in old_start..old_end {
        operations.push(EditOp::Delete { old: index });
    }
    for index in new_start..new_end {
        operations.push(EditOp::Insert { new: index });
    }
}

/// Produces a shortest line-level edit script with a bounded Myers search.
///
/// The frontier for edit distance `d` contains only its `d + 1` reachable diagonals. Retaining
/// those compact frontiers is sufficient to backtrack the shortest path and avoids allocating an
/// `old_len * new_len` matrix. The work budget bounds both retained frontier cells and line
/// comparisons; exceeding it returns `None` without exposing a partial script.
fn myers_edit_script(
    old: &[TextLine<'_>],
    old_start: usize,
    old_end: usize,
    new: &[TextLine<'_>],
    new_start: usize,
    new_end: usize,
    work_limit: usize,
) -> Option<Vec<EditOp>> {
    let old_len = old_end.checked_sub(old_start)?;
    let new_len = new_end.checked_sub(new_start)?;
    let max_distance = old_len.checked_add(new_len)?;
    let mut trace = Vec::<Vec<usize>>::new();
    let mut work = 0_usize;
    let mut found_distance = None;

    for distance in 0..=max_distance {
        let frontier_len = distance.checked_add(1)?;
        work = work.checked_add(frontier_len)?;
        if work > work_limit {
            return None;
        }

        let distance_isize = isize::try_from(distance).ok()?;
        let mut frontier = vec![0_usize; frontier_len];
        let previous = trace.last();
        let mut reached_end = false;

        for (diagonal_index, frontier_entry) in frontier.iter_mut().enumerate() {
            let diagonal_index_isize = isize::try_from(diagonal_index).ok()?;
            let diagonal = -distance_isize + 2 * diagonal_index_isize;
            let mut x = if distance == 0 {
                0
            } else {
                let previous = previous?;
                let previous_distance = distance - 1;

                if diagonal == -distance_isize {
                    frontier_value(previous, previous_distance, diagonal + 1)
                } else if diagonal == distance_isize {
                    frontier_value(previous, previous_distance, diagonal - 1).checked_add(1)?
                } else {
                    let delete_x = frontier_value(previous, previous_distance, diagonal - 1);
                    let insert_x = frontier_value(previous, previous_distance, diagonal + 1);
                    if delete_x < insert_x { insert_x } else { delete_x.checked_add(1)? }
                }
            };
            let x_isize = isize::try_from(x).ok()?;
            let y_isize = x_isize.checked_sub(diagonal)?;
            let mut y = usize::try_from(y_isize).ok()?;

            while x < old_len && y < new_len {
                work = work.checked_add(1)?;
                if work > work_limit {
                    return None;
                }
                if old[old_start + x] != new[new_start + y] {
                    break;
                }
                x += 1;
                y += 1;
            }

            *frontier_entry = x;
            if x == old_len && y == new_len {
                reached_end = true;
                break;
            }
        }

        trace.push(frontier);
        if reached_end {
            found_distance = Some(distance);
            break;
        }
    }

    let found_distance = found_distance?;
    let mut x = old_len;
    let mut y = new_len;
    let mut reversed = Vec::<EditOp>::with_capacity(old_len.saturating_add(new_len));

    for distance in (1..=found_distance).rev() {
        let distance_isize = isize::try_from(distance).ok()?;
        let x_isize = isize::try_from(x).ok()?;
        let y_isize = isize::try_from(y).ok()?;
        let diagonal = x_isize.checked_sub(y_isize)?;
        let previous_distance = distance - 1;
        let previous = trace.get(previous_distance)?;

        let previous_diagonal = if diagonal == -distance_isize {
            diagonal + 1
        } else if diagonal == distance_isize {
            diagonal - 1
        } else {
            let delete_x = frontier_value(previous, previous_distance, diagonal - 1);
            let insert_x = frontier_value(previous, previous_distance, diagonal + 1);
            if delete_x < insert_x { diagonal + 1 } else { diagonal - 1 }
        };
        let previous_x = frontier_value(previous, previous_distance, previous_diagonal);
        let previous_x_isize = isize::try_from(previous_x).ok()?;
        let previous_y_isize = previous_x_isize.checked_sub(previous_diagonal)?;
        let previous_y = usize::try_from(previous_y_isize).ok()?;

        while x > previous_x && y > previous_y {
            x -= 1;
            y -= 1;
            if old[old_start + x] != new[new_start + y] {
                return None;
            }
            reversed.push(EditOp::Equal { old: old_start + x, new: new_start + y });
        }

        if x == previous_x {
            y = y.checked_sub(1)?;
            reversed.push(EditOp::Insert { new: new_start + y });
        } else {
            x = x.checked_sub(1)?;
            reversed.push(EditOp::Delete { old: old_start + x });
        }
    }

    while x > 0 && y > 0 {
        x -= 1;
        y -= 1;
        if old[old_start + x] != new[new_start + y] {
            return None;
        }
        reversed.push(EditOp::Equal { old: old_start + x, new: new_start + y });
    }
    if x != 0 || y != 0 {
        return None;
    }

    reversed.reverse();
    Some(reversed)
}

/// Reads one reachable diagonal from a compact Myers frontier.
fn frontier_value(frontier: &[usize], distance: usize, diagonal: isize) -> usize {
    let distance_isize = isize::try_from(distance).expect("Myers distance fits in isize");
    debug_assert!(diagonal >= -distance_isize && diagonal <= distance_isize);
    debug_assert_eq!((diagonal + distance_isize) % 2, 0);
    let index = usize::try_from((diagonal + distance_isize) / 2)
        .expect("reachable Myers diagonal has a non-negative index");
    frontier[index]
}

/// Computes old/new line positions before each edit operation.
fn operation_positions(operations: &[EditOp]) -> Vec<(usize, usize)> {
    let mut positions = Vec::with_capacity(operations.len() + 1);
    let mut old_position = 0;
    let mut new_position = 0;
    positions.push((old_position, new_position));

    for operation in operations {
        old_position += operation.old_count();
        new_position += operation.new_count();
        positions.push((old_position, new_position));
    }

    positions
}

/// Groups changed operations into unified-diff hunks with the requested context.
fn hunk_ranges(operations: &[EditOp], context: usize) -> Vec<Range<usize>> {
    let mut ranges = Vec::<Range<usize>>::new();

    for (index, operation) in operations.iter().enumerate() {
        if !operation.is_change() {
            continue;
        }

        let start = index.saturating_sub(context);
        let end = index.saturating_add(context).saturating_add(1).min(operations.len());
        match ranges.last_mut() {
            Some(last) if start <= last.end => last.end = last.end.max(end),
            _ => ranges.push(start..end),
        }
    }

    ranges
}

/// Appends one unified-diff hunk to the output buffer.
fn push_hunk(
    output: &mut String,
    operations: &[EditOp],
    positions: &[(usize, usize)],
    old_lines: &[TextLine<'_>],
    new_lines: &[TextLine<'_>],
    range: &Range<usize>,
) {
    let (old_before, new_before) = positions[range.start];
    let (old_after, new_after) = positions[range.end];
    let old_count = old_after - old_before;
    let new_count = new_after - new_before;
    output.push_str("@@ -");
    output.push_str(&format_hunk_side(old_before, old_count));
    output.push_str(" +");
    output.push_str(&format_hunk_side(new_before, new_count));
    output.push_str(" @@\n");

    for operation in &operations[range.start..range.end] {
        match *operation {
            EditOp::Equal { old, .. } => push_diff_line(output, ' ', old_lines[old]),
            EditOp::Delete { old } => push_diff_line(output, '-', old_lines[old]),
            EditOp::Insert { new } => push_diff_line(output, '+', new_lines[new]),
        }
    }
}

/// Formats one `start,count` component of a unified-diff hunk header.
fn format_hunk_side(lines_before: usize, count: usize) -> String {
    let start = if count == 0 { lines_before } else { lines_before + 1 };
    if count == 1 { start.to_string() } else { format!("{start},{count}") }
}

/// Appends one prefixed patch line and the standard missing-final-newline marker when required.
fn push_diff_line(output: &mut String, prefix: char, line: TextLine<'_>) {
    output.push(prefix);
    output.push_str(line.text);
    output.push('\n');
    if !line.terminated {
        output.push_str("\\ No newline at end of file\n");
    }
}

#[cfg(test)]
mod tests {
    use super::{myers_edit_script, split_lines, unified_diff};

    /// Verifies identical bodies emit no patch bytes.
    #[test]
    fn identical_bodies_have_empty_diff() {
        assert_eq!(unified_diff("same\n", "same\n", "a/object.md", "b/object.md", 3), "");
    }

    /// Verifies ordinary replacements use standard Git/unified-diff headers and hunks.
    #[test]
    fn replacement_is_standard_unified_diff() {
        let diff = unified_diff(
            "# Plan\n\nLaunch in September.\n",
            "# Plan\n\nLaunch in October.\n",
            "a/object.md",
            "b/object.md",
            3,
        );

        assert_eq!(
            diff,
            "diff --git a/object.md b/object.md\n--- a/object.md\n+++ b/object.md\n@@ -1,3 +1,3 @@\n # Plan\n \n-Launch in September.\n+Launch in October.\n"
        );
    }

    /// Verifies absent final newlines use the conventional patch marker on both sides.
    #[test]
    fn missing_final_newline_is_reported() {
        let diff = unified_diff("old", "new", "a/object.md", "b/object.md", 3);

        assert_eq!(
            diff,
            "diff --git a/object.md b/object.md\n--- a/object.md\n+++ b/object.md\n@@ -1 +1 @@\n-old\n\\ No newline at end of file\n+new\n\\ No newline at end of file\n"
        );
    }

    /// Verifies adding only a final newline remains an observable text change.
    #[test]
    fn final_newline_change_is_diffed() {
        let diff = unified_diff("body", "body\n", "a/object.md", "b/object.md", 0);

        assert_eq!(
            diff,
            "diff --git a/object.md b/object.md\n--- a/object.md\n+++ b/object.md\n@@ -1 +1 @@\n-body\n\\ No newline at end of file\n+body\n"
        );
    }

    /// Verifies adding content to an empty body uses the conventional zero-length old range.
    #[test]
    fn empty_to_nonempty_uses_standard_insertion_range() {
        let diff = unified_diff("", "first\nsecond\n", "a/object.md", "b/object.md", 3);

        assert!(diff.contains("@@ -0,0 +1,2 @@\n+first\n+second\n"));
    }

    /// Verifies deleting all content uses the conventional zero-length new range.
    #[test]
    fn nonempty_to_empty_uses_standard_deletion_range() {
        let diff = unified_diff("first\nsecond\n", "", "a/object.md", "b/object.md", 3);

        assert!(diff.contains("@@ -1,2 +0,0 @@\n-first\n-second\n"));
    }

    /// Verifies trailing whitespace remains significant and is emitted unchanged in patch lines.
    #[test]
    fn trailing_whitespace_is_preserved() {
        let diff = unified_diff("body  \n", "body \n", "a/object.md", "b/object.md", 0);

        assert!(diff.contains("-body  \n+body \n"));
    }

    /// Verifies Unicode text is compared and emitted without byte loss or escaping.
    #[test]
    fn unicode_is_preserved() {
        let diff = unified_diff("café\n", "caffè\n", "a/object.md", "b/object.md", 0);

        assert!(diff.contains("-café\n+caffè\n"));
    }

    /// Verifies distant changes are split into separate hunks with bounded context.
    #[test]
    fn distant_changes_form_separate_hunks() {
        let old = "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\n";
        let new = "ONE\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nNINE\n";
        let diff = unified_diff(old, new, "a/object.md", "b/object.md", 1);

        assert!(diff.contains("@@ -1,2 +1,2 @@"));
        assert!(diff.contains("@@ -8,2 +8,2 @@"));
        assert_eq!(diff.matches("@@ ").count(), 2);
    }

    /// Verifies repeated lines still produce a useful minimal Myers diff.
    #[test]
    fn repeated_lines_use_myers() {
        let diff = unified_diff(
            "item\nitem\nold\nitem\n",
            "item\nitem\nnew\nitem\n",
            "a/object.md",
            "b/object.md",
            1,
        );

        assert!(diff.contains("-old\n+new\n"));
        assert!(!diff.contains("-item\n-item\n"));
    }

    /// Verifies a large anchorless repeated region keeps a minimal human-readable diff.
    #[test]
    fn large_repeated_region_uses_myers() {
        let old =
            (0..300).map(|index| if index % 2 == 0 { "A\n" } else { "B\n" }).collect::<String>();
        let new = format!("{}A\n", &old[2..]);
        let diff = unified_diff(&old, &new, "a/object.md", "b/object.md", 0);
        let deletions =
            diff.lines().filter(|line| line.starts_with('-') && !line.starts_with("---")).count();
        let insertions =
            diff.lines().filter(|line| line.starts_with('+') && !line.starts_with("+++")).count();

        assert_eq!(deletions, 1);
        assert_eq!(insertions, 1);
    }

    /// Verifies bounded Myers abandons work cleanly instead of returning a partial edit script.
    #[test]
    fn myers_work_budget_aborts_without_partial_script() {
        let old = split_lines("A\nB\nA\nB\n");
        let new = split_lines("B\nA\nB\nA\n");

        assert!(myers_edit_script(&old, 0, old.len(), &new, 0, new.len(), 1).is_none());
    }

    /// Verifies CRLF bytes remain observable instead of being normalized before comparison.
    #[test]
    fn crlf_is_not_normalized() {
        let diff = unified_diff("body\r\n", "body\n", "a/object.md", "b/object.md", 0);

        assert!(diff.contains("-body\r\n"));
        assert!(diff.contains("+body\n"));
    }
}
