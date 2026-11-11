/// The line under the URL bar for `names`, the unresolved variables, or
/// `None` if there are none. `resolvable` is whether there's a collection
/// to resolve against; otherwise no variables apply.
pub fn unresolved_hint(names: &[String], resolvable: bool) -> Option<String> {
    if names.is_empty() {
        return None;
    }
    let list = names.join(", ");
    Some(match (resolvable, names.len()) {
        (false, _) => format!("Choose an environment to use variables: {list}"),
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
    fn resolvable_lists_unresolved_names() {
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
    fn without_a_collection_asks_for_an_environment() {
        assert_eq!(
            unresolved_hint(&names(&["base"]), false).as_deref(),
            Some("Choose an environment to use variables: base")
        );
        assert_eq!(
            unresolved_hint(&names(&["base", "id"]), false).as_deref(),
            Some("Choose an environment to use variables: base, id")
        );
    }
}
