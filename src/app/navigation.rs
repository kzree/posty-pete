/// Matches gpui-router's history limit, so both drop the same old entries.
const MAX_ENTRIES: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Direction {
    Back,
    Forward,
}

/// A back/forward stack with a current entry.
#[derive(Debug)]
pub(super) struct NavHistory<T> {
    entries: Vec<T>,
    index: usize,
}

impl<T: PartialEq> NavHistory<T> {
    pub(super) fn new(initial: T) -> Self {
        Self {
            entries: vec![initial],
            index: 0,
        }
    }

    pub(super) fn current(&self) -> &T {
        &self.entries[self.index]
    }

    /// Makes `entry` current and drops the forward entries. Returns `false`
    /// (and changes nothing) if `entry` already is the current one.
    pub(super) fn push(&mut self, entry: T) -> bool {
        if *self.current() == entry {
            return false;
        }
        self.entries.truncate(self.index + 1);
        self.entries.push(entry);
        if self.entries.len() > MAX_ENTRIES {
            let excess = self.entries.len() - MAX_ENTRIES;
            self.entries.drain(..excess);
        }
        self.index = self.entries.len() - 1;
        true
    }

    pub(super) fn replace_current(&mut self, entry: T) {
        self.entries[self.index] = entry;
    }

    /// Finds the nearest entry in `direction` that is `usable` and differs from
    /// the current one, and returns how many steps away it is.
    pub(super) fn peek(
        &self,
        direction: Direction,
        usable: impl Fn(&T) -> bool,
    ) -> Option<(usize, &T)> {
        let current = self.current();
        let is_target = |entry: &&T| *entry != current && usable(entry);
        match direction {
            Direction::Back => self.entries[..self.index]
                .iter()
                .rev()
                .enumerate()
                .find(|(_, entry)| is_target(entry))
                .map(|(i, entry)| (i + 1, entry)),
            Direction::Forward => self.entries[self.index + 1..]
                .iter()
                .enumerate()
                .find(|(_, entry)| is_target(entry))
                .map(|(i, entry)| (i + 1, entry)),
        }
    }

    pub(super) fn can_go(&self, direction: Direction, usable: impl Fn(&T) -> bool) -> bool {
        self.peek(direction, usable).is_some()
    }

    /// Moves `steps` entries in `direction`, stopping at either end.
    pub(super) fn go(&mut self, direction: Direction, steps: usize) {
        self.index = match direction {
            Direction::Back => self.index.saturating_sub(steps),
            Direction::Forward => (self.index + steps).min(self.entries.len() - 1),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn any(_: &&str) -> bool {
        true
    }

    fn back(history: &mut NavHistory<&'static str>) -> Option<&'static str> {
        let (steps, _) = history.peek(Direction::Back, any)?;
        history.go(Direction::Back, steps);
        Some(*history.current())
    }

    fn forward(history: &mut NavHistory<&'static str>) -> Option<&'static str> {
        let (steps, _) = history.peek(Direction::Forward, any)?;
        history.go(Direction::Forward, steps);
        Some(*history.current())
    }

    #[test]
    fn starts_with_nowhere_to_go() {
        let history = NavHistory::new("/");
        assert_eq!(*history.current(), "/");
        assert!(!history.can_go(Direction::Back, any));
        assert!(!history.can_go(Direction::Forward, any));
    }

    #[test]
    fn back_and_forward_walk_the_pushed_entries() {
        let mut history = NavHistory::new("/");
        assert!(history.push("a"));
        assert!(history.push("b"));

        assert_eq!(back(&mut history), Some("a"));
        assert_eq!(back(&mut history), Some("/"));
        assert_eq!(back(&mut history), None);
        assert_eq!(*history.current(), "/");

        assert_eq!(forward(&mut history), Some("a"));
        assert_eq!(forward(&mut history), Some("b"));
        assert_eq!(forward(&mut history), None);
        assert!(history.can_go(Direction::Back, any));
        assert!(!history.can_go(Direction::Forward, any));
    }

    #[test]
    fn push_drops_forward_entries() {
        let mut history = NavHistory::new("/");
        history.push("a");
        history.push("b");
        back(&mut history);
        back(&mut history);

        assert!(history.push("c"));
        assert!(!history.can_go(Direction::Forward, any));
        assert_eq!(back(&mut history), Some("/"));
        assert_eq!(forward(&mut history), Some("c"));
    }

    #[test]
    fn push_of_the_current_entry_is_ignored() {
        let mut history = NavHistory::new("/");
        history.push("a");
        back(&mut history);

        assert!(!history.push("/"));
        assert_eq!(forward(&mut history), Some("a"));
        assert!(!history.push("a"));
        assert_eq!(back(&mut history), Some("/"));
        assert_eq!(back(&mut history), None);
    }

    #[test]
    fn unusable_entries_are_skipped() {
        let mut history = NavHistory::new("/");
        history.push("gone");
        history.push("a");
        history.push("also-gone");
        history.push("b");
        let usable = |entry: &&str| !entry.ends_with("gone");

        assert_eq!(history.peek(Direction::Back, usable), Some((2, &"a")));
        history.go(Direction::Back, 2);
        assert_eq!(history.peek(Direction::Back, usable), Some((2, &"/")));
        history.go(Direction::Back, 2);
        assert_eq!(history.peek(Direction::Back, usable), None);
        assert_eq!(history.peek(Direction::Forward, usable), Some((2, &"a")));
    }

    #[test]
    fn nothing_usable_left_means_cannot_go() {
        let mut history = NavHistory::new("gone");
        history.push("a");
        assert!(!history.can_go(Direction::Back, |entry| *entry != "gone"));
    }

    #[test]
    fn entries_equal_to_the_current_one_are_skipped() {
        let mut history = NavHistory::new("/");
        history.push("a");
        history.replace_current("/");

        assert_eq!(*history.current(), "/");
        assert!(!history.can_go(Direction::Back, any));
    }

    #[test]
    fn oldest_entries_are_dropped_past_the_limit() {
        let mut history = NavHistory::new(0);
        for i in 1..=MAX_ENTRIES {
            history.push(i);
        }
        let mut steps = 0;
        while let Some((n, _)) = history.peek(Direction::Back, |_| true) {
            history.go(Direction::Back, n);
            steps += n;
        }
        assert_eq!(steps, MAX_ENTRIES - 1);
        assert_eq!(*history.current(), 1);
    }
}
