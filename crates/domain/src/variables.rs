//! `{{name}}` references and their substitution.

use std::{borrow::Cow, collections::HashMap, ops::Range};

use super::{
    request::{Auth, Body, KeyValue, RawText, Request},
    resolved::is_active,
};

/// A `{{name}}` reference found in a text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reference<'a> {
    /// Byte range of the whole reference, braces included.
    pub range: Range<usize>,
    /// The name with surrounding whitespace trimmed.
    pub name: &'a str,
}

/// The references in `text`, in order. Unterminated or empty braces are skipped.
pub fn references(text: &str) -> impl Iterator<Item = Reference<'_>> {
    let mut from = 0;
    std::iter::from_fn(move || {
        let reference = next_reference(text, from)?;
        from = reference.range.end;
        Some(reference)
    })
}

fn next_reference(text: &str, mut from: usize) -> Option<Reference<'_>> {
    loop {
        let start = from + text[from..].find("{{")?;
        let end = start + 2 + text[start + 2..].find("}}")?;
        // Every `{{` before `end` closes at `end`, and only one right after the
        // last `{`, `}` or newline can have a clean inner text. Checking just
        // that one keeps the scan linear.
        let last_bad = start + 1 + text[start + 1..end].rfind(['{', '}', '\n', '\r'])?;
        let inner = &text[last_bad + 1..end];
        let name = inner.trim();
        if text.as_bytes()[last_bad - 1..=last_bad] == *b"{{" && !name.is_empty() {
            return Some(Reference {
                range: last_bad - 1..end + 2,
                name,
            });
        }
        from = end;
    }
}

/// Where `{{name}}` references get their values.
pub trait Lookup {
    fn value(&self, name: &str) -> Option<&str>;

    /// Replaces resolvable references in one pass (values aren't scanned
    /// again). Others stay as written and their names are added to
    /// `unresolved` unless already there.
    fn substitute<'t>(&self, text: &'t str, unresolved: &mut Vec<String>) -> Cow<'t, str> {
        let mut out: Option<String> = None;
        let mut copied = 0;
        for reference in references(text) {
            match self.value(reference.name) {
                Some(value) => {
                    let out = out.get_or_insert_with(|| String::with_capacity(text.len()));
                    out.push_str(&text[copied..reference.range.start]);
                    out.push_str(value);
                    copied = reference.range.end;
                }
                None => note(unresolved, reference.name),
            }
        }
        match out {
            Some(mut out) => {
                out.push_str(&text[copied..]);
                Cow::Owned(out)
            }
            None => Cow::Borrowed(text),
        }
    }

    fn collect_unresolved(&self, text: &str, unresolved: &mut Vec<String>) {
        collect_undefined(text, unresolved, |name| self.value(name).is_some());
    }
}

impl<L: Lookup + ?Sized> Lookup for &L {
    fn value(&self, name: &str) -> Option<&str> {
        (**self).value(name)
    }
}

/// `None` defines nothing.
impl<L: Lookup> Lookup for Option<L> {
    fn value(&self, name: &str) -> Option<&str> {
        self.as_ref()?.value(name)
    }
}

/// The trimmed key `row` defines, unless it's disabled or blank.
pub(crate) fn defined_key(row: &KeyValue) -> Option<&str> {
    defined_name(row.enabled, &row.key)
}

/// [`defined_key`] of a row given by its parts.
pub(crate) fn defined_name(enabled: bool, key: &str) -> Option<&str> {
    let key = key.trim();
    (enabled && !key.is_empty()).then_some(key)
}

/// `name`'s value in sources in priority order, as [`Variables::layered`]
/// would have it, without building a map.
pub(crate) fn first_value<'a>(
    sources: impl IntoIterator<Item = &'a [KeyValue]>,
    name: &str,
) -> Option<&'a str> {
    sources
        .into_iter()
        .flatten()
        .find(|row| defined_key(row) == Some(name))
        .map(|row| row.value.as_str())
}

/// Variable values by name, merged from sources in priority order. Borrows
/// the rows it's built from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Variables<'a> {
    values: HashMap<&'a str, &'a str>,
}

impl<'a> Variables<'a> {
    #[cfg(test)]
    pub fn from_rows(rows: &[KeyValue]) -> Variables<'static> {
        Variables::layered([&*rows.to_vec().leak()])
    }

    /// The first source wins for a name defined in several; within a source,
    /// the first enabled row wins. Keys are trimmed.
    pub fn layered(sources: impl IntoIterator<Item = &'a [KeyValue]>) -> Self {
        let mut values = HashMap::new();
        for row in sources.into_iter().flatten() {
            if let Some(key) = defined_key(row) {
                values.entry(key).or_insert(row.value.as_str());
            }
        }
        Self { values }
    }

    pub fn get(&self, name: &str) -> Option<&'a str> {
        self.values.get(name).copied()
    }
}

impl Lookup for Variables<'_> {
    fn value(&self, name: &str) -> Option<&str> {
        self.get(name)
    }
}

/// Adds the names referenced in `text` that `defined` rejects to
/// `unresolved`, unless already there.
pub(crate) fn collect_undefined(
    text: &str,
    unresolved: &mut Vec<String>,
    defined: impl Fn(&str) -> bool,
) {
    for reference in references(text) {
        if !defined(reference.name) {
            note(unresolved, reference.name);
        }
    }
}

fn note(unresolved: &mut Vec<String>, name: &str) {
    if !unresolved.iter().any(|seen| seen == name) {
        unresolved.push(name.to_string());
    }
}

struct Substitution<'v, L: ?Sized>(&'v L);

impl<L: Lookup + ?Sized> Substitution<'_, L> {
    fn text(&self, text: &str) -> String {
        self.0.substitute(text, &mut Vec::new()).into_owned()
    }

    /// Keeps `text` when it has nothing to substitute.
    fn owned_text(&self, text: String) -> String {
        let substituted = match self.0.substitute(&text, &mut Vec::new()) {
            Cow::Borrowed(_) => None,
            Cow::Owned(substituted) => Some(substituted),
        };
        substituted.unwrap_or(text)
    }

    /// Shares `text` when it has nothing to substitute.
    fn raw_text(&self, text: RawText) -> RawText {
        let substituted = match self.0.substitute(&text, &mut Vec::new()) {
            Cow::Borrowed(_) => None,
            Cow::Owned(substituted) => Some(substituted),
        };
        substituted.map_or(text, RawText::from)
    }

    fn rows(&self, rows: &[KeyValue]) -> Vec<KeyValue> {
        rows.iter()
            .map(|row| KeyValue {
                enabled: row.enabled,
                key: self.text(&row.key),
                value: self.text(&row.value),
            })
            .collect()
    }

    fn owned_rows(&self, rows: Vec<KeyValue>) -> Vec<KeyValue> {
        rows.into_iter()
            .map(|row| KeyValue {
                enabled: row.enabled,
                key: self.owned_text(row.key),
                value: self.owned_text(row.value),
            })
            .collect()
    }

    fn auth(&self, auth: &Auth) -> Auth {
        self.owned_auth(auth.clone())
    }

    fn owned_auth(&self, auth: Auth) -> Auth {
        match auth {
            Auth::Inherit => Auth::Inherit,
            Auth::None => Auth::None,
            Auth::ApiKey {
                key,
                value,
                location,
            } => Auth::ApiKey {
                key: self.owned_text(key),
                value: self.owned_text(value),
                location,
            },
            Auth::Basic { username, password } => Auth::Basic {
                username: self.owned_text(username),
                password: self.owned_text(password),
            },
            Auth::Bearer { token } => Auth::Bearer {
                token: self.owned_text(token),
            },
        }
    }
}

impl Request {
    /// Variables substituted (method untouched). Text without references is
    /// kept as it is, not copied.
    pub(super) fn with_variables_substituted(self, variables: &impl Lookup) -> Request {
        let s = Substitution(variables);
        Request {
            method: self.method,
            url: s.owned_text(self.url),
            params: s.owned_rows(self.params),
            headers: s.owned_rows(self.headers),
            body: match self.body {
                Body::None => Body::None,
                Body::FormData(fields) => Body::FormData(s.owned_rows(fields)),
                Body::UrlEncoded(fields) => Body::UrlEncoded(s.owned_rows(fields)),
                Body::Raw { kind, text } => Body::Raw {
                    kind,
                    text: s.raw_text(text),
                },
            },
            auth: s.owned_auth(self.auth),
        }
    }

    #[cfg(test)]
    pub fn unresolved_variables(&self, variables: &Variables<'_>) -> Vec<String> {
        self.unresolved_variables_with_auth(&self.auth, variables)
    }

    /// The names that don't resolve in the parts of the request that get
    /// sent with `auth` in place of its own, in first-seen order. Whether a
    /// part is sent is decided as [`Request::resolve`] does, after substitution.
    pub(super) fn unresolved_variables_with_auth(
        &self,
        auth: &Auth,
        variables: &impl Lookup,
    ) -> Vec<String> {
        // Only keys and auth decide what is sent; the URL and raw text always are.
        let s = Substitution(variables);
        let substituted = Request {
            method: self.method,
            url: String::new(),
            params: s.rows(&self.params),
            headers: s.rows(&self.headers),
            body: match &self.body {
                Body::FormData(fields) => Body::FormData(s.rows(fields)),
                Body::UrlEncoded(fields) => Body::UrlEncoded(s.rows(fields)),
                _ => Body::None,
            },
            auth: s.auth(auth),
        };

        let mut texts: Vec<&str> = vec![&self.url];
        let rows = [
            (&self.params[..], &substituted.params[..], false),
            (&self.headers[..], &substituted.headers[..], true),
            match (&self.body, &substituted.body) {
                (Body::FormData(fields), Body::FormData(sent))
                | (Body::UrlEncoded(fields), Body::UrlEncoded(sent)) => {
                    (&fields[..], &sent[..], false)
                }
                _ => (&[][..], &[][..], false),
            },
        ];
        for (rows, sent, is_header) in rows {
            for (row, sent) in rows.iter().zip(sent) {
                let sent = if is_header {
                    substituted.sends_header(sent)
                } else {
                    is_active(sent)
                };
                if sent {
                    texts.extend([row.key.as_str(), row.value.as_str()]);
                }
            }
        }
        let mut unresolved = Vec::new();
        for text in texts {
            variables.collect_unresolved(text, &mut unresolved);
        }
        if let Body::Raw { text, .. } = &self.body {
            for name in text.reference_names() {
                if variables.value(name).is_none() {
                    note(&mut unresolved, name);
                }
            }
        }
        if substituted.sends_auth() {
            for text in auth.texts() {
                variables.collect_unresolved(text, &mut unresolved);
            }
        }
        unresolved
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::{ApiKeyLocation, HttpMethod, RawKind};

    fn row(enabled: bool, key: &str, value: &str) -> KeyValue {
        KeyValue {
            enabled,
            key: key.into(),
            value: value.into(),
        }
    }

    fn vars(rows: &[(&str, &str)]) -> Variables<'static> {
        let rows: Vec<_> = rows.iter().map(|(k, v)| row(true, k, v)).collect();
        Variables::from_rows(&rows)
    }

    fn sub(variables: &Variables<'_>, text: &str) -> (String, Vec<String>) {
        let mut unresolved = Vec::new();
        let out = variables.substitute(text, &mut unresolved).into_owned();
        (out, unresolved)
    }

    fn names(text: &str) -> Vec<&str> {
        references(text).map(|r| r.name).collect()
    }

    #[test]
    fn finds_references_with_ranges() {
        let refs: Vec<_> = references("a {{x}} b {{ y }}").collect();
        assert_eq!(
            refs,
            [
                Reference {
                    range: 2..7,
                    name: "x"
                },
                Reference {
                    range: 10..17,
                    name: "y"
                },
            ]
        );
    }

    #[test]
    fn invalid_braces_are_plain_text() {
        assert!(names("{{}} {{ }} {{x").is_empty());
        assert!(names("{{a\nb}} {{a}b}}").is_empty());
        assert_eq!(names("{{ {{x}}"), ["x"]);
        assert_eq!(names("{{{x}}}"), ["x"]);
        assert_eq!(names("}}{{x}}"), ["x"]);
    }

    #[test]
    fn multibyte_before_a_delimiter_does_not_panic() {
        for text in [
            "{{é\nx}}",
            "{{é{x}}",
            "{{ü}x}}",
            "{{é\rx}}",
            "{{😀\nx}}",
            "{{e\u{301}{x}}",
        ] {
            assert!(names(text).is_empty(), "{text:?}");
        }
        assert_eq!(names("{{é{{x}}"), ["x"]);
        assert_eq!(names("é{{ü}} {{😀\n{{y}}"), ["ü", "y"]);
    }

    #[test]
    fn unicode_names_have_exact_ranges() {
        let refs: Vec<_> = references("ä{{naïve}}").collect();
        assert_eq!(
            refs,
            [Reference {
                range: 2..12,
                name: "naïve"
            }]
        );
        assert_eq!(names("{{ 😀 }}{{e\u{301}}}"), ["😀", "e\u{301}"]);
    }

    #[test]
    fn awkward_strings_never_panic() {
        const PIECES: [&str; 9] = ["{", "}", "\n", "\r", "é", "😀", "\u{301}", "x", " "];
        let v = vars(&[("x", "é"), ("é", "{{x}}")]);
        let mut text = String::new();
        let mut digits = Vec::new();
        for len in 0..=6 {
            digits.clear();
            digits.resize(len, 0);
            loop {
                text.clear();
                text.extend(digits.iter().map(|&d| PIECES[d]));
                let mut previous_end = 0;
                for reference in references(&text) {
                    let range = reference.range.clone();
                    assert!(range.start >= previous_end, "{text:?}");
                    assert!(text.is_char_boundary(range.start), "{text:?}");
                    assert!(text.is_char_boundary(range.end), "{text:?}");
                    let slice = &text[range.clone()];
                    assert!(slice.starts_with("{{") && slice.ends_with("}}"), "{text:?}");
                    assert_eq!(slice[2..slice.len() - 2].trim(), reference.name);
                    previous_end = range.end;
                }
                sub(&v, &text);

                let Some(i) = digits.iter().rposition(|&d| d + 1 < PIECES.len()) else {
                    break;
                };
                digits[i] += 1;
                digits[i + 1..].fill(0);
            }
        }
    }

    #[test]
    fn many_unclosed_openings_resolve_in_linear_time() {
        let text = "{{a".repeat(200_000) + "}}";
        let refs: Vec<_> = references(&text).collect();
        assert_eq!(
            refs,
            [Reference {
                range: text.len() - 5..text.len(),
                name: "a"
            }]
        );
    }

    #[test]
    fn substitutes_basic_reference() {
        let v = vars(&[("host", "example.com")]);
        assert_eq!(
            sub(&v, "https://{{host}}/a"),
            ("https://example.com/a".into(), vec![])
        );
    }

    #[test]
    fn whitespace_inside_braces_is_trimmed() {
        let v = vars(&[("host", "h")]);
        assert_eq!(sub(&v, "{{  host\t}}").0, "h");
    }

    #[test]
    fn unchanged_text_is_borrowed() {
        let v = vars(&[("a", "1")]);
        let mut unresolved = Vec::new();
        assert!(matches!(
            v.substitute("plain", &mut unresolved),
            Cow::Borrowed(_)
        ));
        assert!(matches!(
            v.substitute("{{b}}", &mut unresolved),
            Cow::Borrowed(_)
        ));
        assert_eq!(unresolved, ["b"]);
    }

    #[test]
    fn unknown_and_disabled_names_stay_and_are_reported() {
        let v = Variables::from_rows(&[row(false, "off", "x"), row(true, "on", "1")]);
        assert_eq!(
            sub(&v, "{{on}} {{off}} {{ missing }} {{off}}"),
            (
                "1 {{off}} {{ missing }} {{off}}".into(),
                vec!["off".into(), "missing".into()]
            )
        );
    }

    #[test]
    fn first_enabled_row_wins_and_keys_are_trimmed() {
        let v = Variables::from_rows(&[
            row(false, "a", "disabled"),
            row(true, " a ", "first"),
            row(true, "a", "second"),
            row(true, "  ", "blank key"),
        ]);
        assert_eq!(v.get("a"), Some("first"));
        assert_eq!(v.get(""), None);
    }

    #[test]
    fn earlier_source_wins_when_layered() {
        let environment = [row(true, "host", "env"), row(false, "token", "off")];
        let collection = [row(true, "host", "col"), row(true, "token", "t")];
        let v = Variables::layered([&environment[..], &collection[..]]);
        assert_eq!(v.get("host"), Some("env"));
        assert_eq!(v.get("token"), Some("t"));
    }

    #[test]
    fn values_are_not_substituted_again() {
        let v = vars(&[("a", "{{b}}"), ("b", "no")]);
        assert_eq!(sub(&v, "{{a}}"), ("{{b}}".into(), vec![]));
    }

    #[test]
    fn unterminated_braces_are_left_alone() {
        let v = vars(&[("a", "1")]);
        assert_eq!(sub(&v, "{{a}} {{a").0, "1 {{a");
        assert_eq!(sub(&v, "{{a {{a}}").0, "{{a 1");
    }

    #[test]
    fn adjacent_references() {
        let v = vars(&[("a", "1"), ("b", "2")]);
        assert_eq!(sub(&v, "{{a}}{{b}}{{a}}").0, "121");
    }

    #[test]
    fn non_ascii_text_and_names() {
        let v = vars(&[("näme", "väärtus"), ("ключ", "🔑")]);
        assert_eq!(
            sub(&v, "é{{näme}}ü{{ ключ }}ß{{日本}}"),
            ("éväärtusü🔑ß{{日本}}".into(), vec!["日本".into()])
        );
    }

    fn templated_request() -> Request {
        Request {
            method: HttpMethod::Post,
            url: "{{base}}/users".into(),
            params: vec![
                row(true, "{{pk}}", "{{pv}}"),
                row(false, "off", "{{p_off}}"),
            ],
            headers: vec![row(true, "{{hk}}", "{{hv}}"), row(true, "", "{{h_blank}}")],
            body: Body::FormData(vec![row(true, "{{fk}}", "{{fv}}")]),
            auth: Auth::ApiKey {
                key: "{{ak}}".into(),
                value: "{{av}}".into(),
                location: ApiKeyLocation::Query,
            },
        }
    }

    const ALL_NAMES: [&str; 9] = ["base", "pk", "pv", "hk", "hv", "fk", "fv", "ak", "av"];

    #[test]
    fn raw_body_without_references_is_shared_by_substitution() {
        let text = RawText::from(r#"{"id": 1}"#);
        let request = Request {
            body: Body::Raw {
                kind: RawKind::Json,
                text: text.clone(),
            },
            ..Request::default()
        };
        let Body::Raw { text: out, .. } = request
            .with_variables_substituted(&vars(&[("id", "7")]))
            .body
        else {
            panic!("not a raw body");
        };
        assert!(RawText::ptr_eq(&text, &out));
    }

    #[test]
    fn raw_body_names_keep_their_place_in_the_unresolved_order() {
        let request = Request {
            url: "{{base}}/{{b}}".into(),
            body: Body::Raw {
                kind: RawKind::Text,
                text: "{{a}} {{base}} {{b}} {{c}} {{a}} {{known}}".into(),
            },
            auth: Auth::Bearer {
                token: "{{c}} {{token}}".into(),
            },
            ..Request::default()
        };
        assert_eq!(
            request.unresolved_variables(&vars(&[("known", "k")])),
            ["base", "b", "a", "c", "token"]
        );
    }

    #[test]
    fn with_variables_covers_every_field() {
        let rows: Vec<_> = ALL_NAMES
            .iter()
            .chain(&["p_off", "h_blank"])
            .map(|name| row(true, name, &name.to_uppercase()))
            .collect();
        let v = Variables::from_rows(&rows);
        let out = templated_request().with_variables_substituted(&v);
        assert!(templated_request().unresolved_variables(&v).is_empty());
        assert_eq!(
            out,
            Request {
                method: HttpMethod::Post,
                url: "BASE/users".into(),
                params: vec![row(true, "PK", "PV"), row(false, "off", "P_OFF")],
                headers: vec![row(true, "HK", "HV"), row(true, "", "H_BLANK")],
                body: Body::FormData(vec![row(true, "FK", "FV")]),
                auth: Auth::ApiKey {
                    key: "AK".into(),
                    value: "AV".into(),
                    location: ApiKeyLocation::Query,
                },
            }
        );

        let raw = Request {
            body: Body::Raw {
                kind: RawKind::Json,
                text: r#"{"id": "{{id}}"}"#.into(),
            },
            auth: Auth::Basic {
                username: "{{user}}".into(),
                password: "{{pass}}".into(),
            },
            ..Request::default()
        };
        let v = vars(&[("id", "7"), ("user", "u"), ("pass", "p")]);
        let out = raw.clone().with_variables_substituted(&v);
        assert!(raw.unresolved_variables(&v).is_empty());
        assert_eq!(
            out.body,
            Body::Raw {
                kind: RawKind::Json,
                text: r#"{"id": "7"}"#.into()
            }
        );
        assert_eq!(
            out.auth,
            Auth::Basic {
                username: "u".into(),
                password: "p".into()
            }
        );

        let bearer = Request {
            auth: Auth::Bearer {
                token: "{{t}}".into(),
            },
            ..Request::default()
        };
        let out = bearer
            .clone()
            .with_variables_substituted(&vars(&[("t", "secret")]));
        assert_eq!(
            out.auth,
            Auth::Bearer {
                token: "secret".into()
            }
        );
    }

    #[test]
    fn unresolved_are_deduped_in_request_order_and_skip_unsent_rows() {
        let mut request = templated_request();
        request.params.push(row(true, "{{base}}", "{{pv}}"));
        let expected: Vec<String> = ALL_NAMES.iter().map(|n| n.to_string()).collect();

        assert_eq!(
            request
                .clone()
                .with_variables_substituted(&Variables::default()),
            request
        );
        assert_eq!(
            request.unresolved_variables(&Variables::default()),
            expected
        );
    }

    #[test]
    fn defined_names_are_not_reported() {
        let request = templated_request();
        let v = vars(&[("pk", "1"), ("hv", "2"), ("ak", "3")]);
        assert_eq!(
            request.unresolved_variables(&v),
            ["base", "pv", "hk", "fk", "fv", "av"]
        );
    }

    #[test]
    fn rows_whose_key_resolves_to_empty_are_not_sent() {
        let request = Request {
            params: vec![row(true, "{{empty}}", "{{p}}")],
            headers: vec![row(true, "{{empty}}", "{{h}}")],
            body: Body::FormData(vec![row(true, "{{empty}}", "{{f}}")]),
            ..Request::default()
        };
        let v = vars(&[("empty", "")]);
        assert!(request.unresolved_variables(&v).is_empty());
        assert_eq!(
            request.unresolved_variables(&Variables::default()),
            ["empty", "p", "h", "f"]
        );
    }

    #[test]
    fn url_encoded_fields_are_substituted_and_checked() {
        let request = Request {
            headers: vec![row(true, "Content-Type", "{{type}}")],
            body: Body::UrlEncoded(vec![
                row(true, "{{k}}", "{{v}}"),
                row(false, "off", "{{off}}"),
                row(true, "{{empty}}", "{{unsent}}"),
            ]),
            ..Request::default()
        };
        let v = vars(&[("k", "K"), ("v", "V"), ("empty", "")]);
        assert_eq!(
            request.clone().with_variables_substituted(&v).body,
            Body::UrlEncoded(vec![
                row(true, "K", "V"),
                row(false, "off", "{{off}}"),
                row(true, "", "{{unsent}}"),
            ])
        );
        assert_eq!(request.unresolved_variables(&v), ["type"]);
        assert_eq!(
            request.unresolved_variables(&Variables::default()),
            ["type", "k", "v", "empty", "unsent"]
        );
    }

    #[test]
    fn form_data_drops_content_type_header() {
        let request = Request {
            headers: vec![row(true, "{{ct}}", "{{type}}")],
            body: Body::FormData(Vec::new()),
            ..Request::default()
        };
        assert_eq!(
            request.unresolved_variables(&Variables::default()),
            ["ct", "type"]
        );
        let v = vars(&[("ct", "content-type")]);
        assert!(request.unresolved_variables(&v).is_empty());
    }

    #[test]
    fn auth_that_isnt_sent_is_not_reported() {
        let bearer = Request {
            headers: vec![row(true, "{{auth}}", "Custom x")],
            auth: Auth::Bearer {
                token: "{{token}}".into(),
            },
            ..Request::default()
        };
        assert_eq!(
            bearer.unresolved_variables(&vars(&[("auth", "X-Other")])),
            ["token"]
        );
        assert!(
            bearer
                .unresolved_variables(&vars(&[("auth", "Authorization")]))
                .is_empty()
        );

        let basic = Request {
            headers: vec![row(true, "Authorization", "Custom x")],
            auth: Auth::Basic {
                username: "{{user}}".into(),
                password: "{{pass}}".into(),
            },
            ..Request::default()
        };
        assert!(basic.unresolved_variables(&Variables::default()).is_empty());

        let api_key = Request {
            headers: vec![row(true, "X-Key", "mine")],
            auth: Auth::ApiKey {
                key: "{{key}}".into(),
                value: "{{value}}".into(),
                location: ApiKeyLocation::Header,
            },
            ..Request::default()
        };
        assert_eq!(
            api_key.unresolved_variables(&vars(&[("key", "X-Other")])),
            ["value"]
        );
        assert!(
            api_key
                .unresolved_variables(&vars(&[("key", "x-key")]))
                .is_empty()
        );
        assert!(
            api_key
                .unresolved_variables(&vars(&[("key", "")]))
                .is_empty()
        );

        let query_key = Request {
            params: vec![row(true, "api_key", "mine")],
            headers: vec![row(true, "X-Key", "mine")],
            auth: Auth::ApiKey {
                key: "{{key}}".into(),
                value: "{{value}}".into(),
                location: ApiKeyLocation::Query,
            },
            ..Request::default()
        };
        assert_eq!(
            query_key.unresolved_variables(&vars(&[("key", "X-Key")])),
            ["value"]
        );
        assert!(
            query_key
                .unresolved_variables(&vars(&[("key", "api_key")]))
                .is_empty()
        );
    }

    #[test]
    fn substituted_values_are_not_reported() {
        let request = Request {
            url: "{{a}}".into(),
            ..Request::default()
        };
        assert!(
            request
                .unresolved_variables(&vars(&[("a", "{{b}}")]))
                .is_empty()
        );
    }
}
