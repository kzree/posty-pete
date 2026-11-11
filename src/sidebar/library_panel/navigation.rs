#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Direction {
    Up,
    Down,
}

/// The selectable row an arrow key moves to from `current`, wrapping at the
/// ends like gpui-component's List. With nothing (or a stale row) selected,
/// Down picks the first selectable row and Up the last. `None` when no row
/// is selectable.
pub(super) fn step(
    len: usize,
    current: Option<usize>,
    direction: Direction,
    selectable: impl Fn(usize) -> bool,
) -> Option<usize> {
    let start = match current.filter(|&row| row < len) {
        Some(row) => row,
        None if direction == Direction::Down => len.saturating_sub(1),
        None => 0,
    };
    (1..=len)
        .map(|offset| match direction {
            Direction::Down => (start + offset) % len,
            Direction::Up => (start + len - offset) % len,
        })
        .find(|&row| selectable(row))
}

#[cfg(test)]
mod tests {
    use super::{Direction::*, *};

    /// `x` marks a placeholder row.
    fn run(rows: &str, current: Option<usize>, direction: Direction) -> Option<usize> {
        let rows: Vec<bool> = rows.chars().map(|c| c != 'x').collect();
        step(rows.len(), current, direction, |row| rows[row])
    }

    #[test]
    fn moves_to_the_neighbour() {
        assert_eq!(run("ooo", Some(1), Down), Some(2));
        assert_eq!(run("ooo", Some(1), Up), Some(0));
    }

    #[test]
    fn skips_a_placeholder() {
        assert_eq!(run("oxo", Some(0), Down), Some(2));
        assert_eq!(run("oxo", Some(2), Up), Some(0));
    }

    #[test]
    fn skips_adjacent_placeholders() {
        assert_eq!(run("oxxo", Some(0), Down), Some(3));
        assert_eq!(run("oxxo", Some(3), Up), Some(0));
    }

    #[test]
    fn wraps_at_the_ends() {
        assert_eq!(run("ooo", Some(2), Down), Some(0));
        assert_eq!(run("ooo", Some(0), Up), Some(2));
    }

    #[test]
    fn placeholders_at_the_start_are_skipped_when_wrapping() {
        assert_eq!(run("xxoo", Some(3), Down), Some(2));
        assert_eq!(run("xxoo", Some(2), Up), Some(3));
        assert_eq!(run("xxoo", None, Down), Some(2));
    }

    #[test]
    fn placeholders_at_the_end_are_skipped_when_wrapping() {
        assert_eq!(run("ooxx", Some(1), Down), Some(0));
        assert_eq!(run("ooxx", Some(0), Up), Some(1));
        assert_eq!(run("ooxx", None, Up), Some(1));
    }

    #[test]
    fn library_without_collections_or_environments() {
        let rows = "oxox";
        assert_eq!(run(rows, Some(0), Down), Some(2));
        assert_eq!(run(rows, Some(2), Down), Some(0));
        assert_eq!(run(rows, Some(0), Up), Some(2));
    }

    #[test]
    fn nothing_selected_starts_at_an_end() {
        assert_eq!(run("ooo", None, Down), Some(0));
        assert_eq!(run("ooo", None, Up), Some(2));
    }

    #[test]
    fn a_single_selectable_row_stays_put() {
        assert_eq!(run("xox", Some(1), Down), Some(1));
        assert_eq!(run("xox", Some(1), Up), Some(1));
    }

    #[test]
    fn leaving_a_placeholder_row() {
        assert_eq!(run("oxo", Some(1), Down), Some(2));
        assert_eq!(run("oxo", Some(1), Up), Some(0));
    }

    #[test]
    fn stale_selection_starts_at_an_end() {
        assert_eq!(run("oxo", Some(9), Down), Some(0));
        assert_eq!(run("oxo", Some(9), Up), Some(2));
    }

    #[test]
    fn all_placeholders_or_no_rows_select_nothing() {
        assert_eq!(run("xxx", Some(1), Down), None);
        assert_eq!(run("xxx", None, Up), None);
        assert_eq!(run("", None, Down), None);
        assert_eq!(run("", Some(0), Up), None);
    }
}
