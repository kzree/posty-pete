use std::collections::BTreeSet;

/// A kind of warning that's counted, once per distinct key (an operation, a
/// `$ref`...), and reported as one line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Counted {
    ExternalRef,
    BrokenRef,
    CookieParam,
    FileField,
    UnsupportedOperationAuth(String),
    CombinedSchemes,
    SkippedMethod(String),
    TruncatedExample,
    OversizedExample,
    UnsupportedBody(String),
}

/// Warnings, deduplicated, in the order first seen.
#[derive(Debug, Default)]
pub(super) struct Warnings {
    counted: Vec<(Counted, BTreeSet<String>)>,
    notes: Vec<String>,
}

impl Warnings {
    pub fn count(&mut self, kind: Counted, key: impl Into<String>) {
        let key = key.into();
        match self.counted.iter_mut().find(|(seen, _)| *seen == kind) {
            Some((_, keys)) => {
                keys.insert(key);
            }
            None => self.counted.push((kind, BTreeSet::from([key]))),
        }
    }

    pub fn note(&mut self, message: impl Into<String>) {
        let message = message.into();
        if !self.notes.contains(&message) {
            self.notes.push(message);
        }
    }

    pub fn into_messages(self) -> Vec<String> {
        let mut messages = self.notes;
        messages.extend(
            self.counted
                .into_iter()
                .map(|(kind, keys)| message(&kind, keys.len())),
        );
        messages
    }
}

fn message(kind: &Counted, n: usize) -> String {
    let one = n == 1;
    let s = if one { "" } else { "s" };
    let were = if one { "was" } else { "were" };
    match kind {
        Counted::ExternalRef => {
            format!("{n} $ref{s} to other files or URLs {were} not followed.")
        }
        Counted::BrokenRef => format!("{n} $ref{s} couldn't be resolved."),
        Counted::CookieParam => format!("{n} cookie parameter{s} {were} skipped."),
        Counted::FileField => format!("{n} file field{s} became empty text field{s}."),
        Counted::UnsupportedOperationAuth(label) => {
            let (uses, their) = if one {
                ("uses", "its")
            } else {
                ("use", "their")
            };
            format!(
                "{n} operation{s} {uses} {label}, which isn't supported; {their} auth was left empty."
            )
        }
        Counted::CombinedSchemes => {
            let (requires, its) = if one {
                ("requires", "its")
            } else {
                ("require", "their")
            };
            format!(
                "{n} operation{s} {requires} several security schemes together; only {its} first was used."
            )
        }
        Counted::SkippedMethod(method) => format!("{n} {method} operation{s} {were} skipped."),
        Counted::TruncatedExample => {
            let body = if one { "body" } else { "bodies" };
            format!("{n} generated example {body} {were} cut short.")
        }
        Counted::OversizedExample => {
            let mb = super::example::MAX_BODY_BYTES >> 20;
            format!("{n} operation{s} had example values cut short (over {mb} MB).")
        }
        Counted::UnsupportedBody(media) => {
            let body = if one { "body" } else { "bodies" };
            format!("{n} request {body} of type {media} {were} left empty.")
        }
    }
}
