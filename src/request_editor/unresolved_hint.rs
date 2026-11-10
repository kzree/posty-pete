/// The line under the URL bar for `names`, the unresolved variables, or
/// `None` if there are none. `linked` is whether the editor holds a saved
/// request; otherwise no variables apply.
pub fn unresolved_hint(names: &[String], linked: bool) -> Option<String> {
    if names.is_empty() {
        return None;
    }
    let list = names.join(", ");
    Some(match (linked, names.len()) {
        (false, _) => format!("Variables only work in saved requests: {list}"),
        (true, 1) => format!("Unresolved variable: {list}"),
        (true, _) => format!("Unresolved variables: {list}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn nothing_unresolved_has_no_hint() {
        assert_eq!(unresolved_hint(&[], true), None);
        assert_eq!(unresolved_hint(&[], false), None);
    }

    #[test]
    fn linked_lists_unresolved_names() {
        assert_eq!(
            unresolved_hint(&names(&["base", "token"]), true).as_deref(),
            Some("Unresolved variables: base, token")
        );
        assert_eq!(
            unresolved_hint(&names(&["base"]), true).as_deref(),
            Some("Unresolved variable: base")
        );
    }

    #[test]
    fn unlinked_says_why() {
        assert_eq!(
            unresolved_hint(&names(&["base"]), false).as_deref(),
            Some("Variables only work in saved requests: base")
        );
        assert_eq!(
            unresolved_hint(&names(&["base", "id"]), false).as_deref(),
            Some("Variables only work in saved requests: base, id")
        );
    }
}
