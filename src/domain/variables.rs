//! `{{name}}` references and their substitution.

use std::{borrow::Cow, collections::HashMap, ops::Range};

use super::request::{Auth, Body, KeyValue, Request};

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
        if text[last_bad - 1..=last_bad] == *"{{" && !name.is_empty() {
            return Some(Reference {
                range: last_bad - 1..end + 2,
                name,
            });
        }
        from = end;
    }
}

/// Variable values by name, merged from sources in priority order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Variables {
    values: HashMap<String, String>,
}

impl Variables {
    pub fn from_rows(rows: &[KeyValue]) -> Self {
        Self::layered([rows])
    }

    /// The first source wins for a name defined in several; within a source,
    /// the first enabled row wins. Keys are trimmed.
    pub fn layered<'a>(sources: impl IntoIterator<Item = &'a [KeyValue]>) -> Self {
        let mut values = HashMap::new();
        for row in sources.into_iter().flatten() {
            let key = row.key.trim();
            if row.enabled && !key.is_empty() && !values.contains_key(key) {
                values.insert(key.to_string(), row.value.clone());
            }
        }
        Self { values }
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Replaces resolvable references in one pass (values aren't scanned
    /// again). Others stay as written and their names are added to
    /// `unresolved` unless already there.
    pub fn substitute<'t>(&self, text: &'t str, unresolved: &mut Vec<String>) -> Cow<'t, str> {
        let mut out: Option<String> = None;
        let mut copied = 0;
        for reference in references(text) {
            match self.get(reference.name) {
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
        for reference in references(text) {
            if self.get(reference.name).is_none() {
                note(unresolved, reference.name);
            }
        }
    }
}

fn note(unresolved: &mut Vec<String>, name: &str) {
    if !unresolved.iter().any(|seen| seen == name) {
        unresolved.push(name.to_string());
    }
}

/// Rows that are sent; only these report unresolved names.
fn is_sent(row: &KeyValue) -> bool {
    row.enabled && !row.key.is_empty()
}

struct Substitution<'v> {
    variables: &'v Variables,
    unresolved: Vec<String>,
}

impl Substitution<'_> {
    fn text(&mut self, text: &str, report: bool) -> String {
        if report {
            self.variables.substitute(text, &mut self.unresolved)
        } else {
            self.variables.substitute(text, &mut Vec::new())
        }
        .into_owned()
    }

    fn rows(&mut self, rows: &[KeyValue]) -> Vec<KeyValue> {
        rows.iter()
            .map(|row| {
                let sent = is_sent(row);
                KeyValue {
                    enabled: row.enabled,
                    key: self.text(&row.key, sent),
                    value: self.text(&row.value, sent),
                }
            })
            .collect()
    }
}

impl Request {
    /// A copy with variables substituted (method untouched), and the names
    /// that didn't resolve in the parts that get sent, in first-seen order.
    pub fn with_variables(&self, variables: &Variables) -> (Request, Vec<String>) {
        let mut s = Substitution {
            variables,
            unresolved: Vec::new(),
        };
        let url = s.text(&self.url, true);
        let params = s.rows(&self.params);
        let headers = s.rows(&self.headers);
        let body = match &self.body {
            Body::None => Body::None,
            Body::FormData(fields) => Body::FormData(s.rows(fields)),
            Body::Raw { kind, text } => Body::Raw {
                kind: *kind,
                text: s.text(text, true),
            },
        };
        let auth = match &self.auth {
            Auth::None => Auth::None,
            Auth::ApiKey { key, value } => Auth::ApiKey {
                key: s.text(key, true),
                value: s.text(value, true),
            },
            Auth::Basic { username, password } => Auth::Basic {
                username: s.text(username, true),
                password: s.text(password, true),
            },
            Auth::Bearer { token } => Auth::Bearer {
                token: s.text(token, true),
            },
        };
        let request = Request {
            method: self.method,
            url,
            params,
            headers,
            body,
            auth,
        };
        (request, s.unresolved)
    }

    /// The names [`Request::with_variables`] would report, without building the copy.
    pub fn unresolved_variables(&self, variables: &Variables) -> Vec<String> {
        let mut unresolved = Vec::new();
        for text in sent_texts(self) {
            variables.collect_unresolved(text, &mut unresolved);
        }
        unresolved
    }
}

fn sent_row_texts(rows: &[KeyValue]) -> impl Iterator<Item = &str> {
    rows.iter()
        .filter(|row| is_sent(row))
        .flat_map(|row| [row.key.as_str(), row.value.as_str()])
}

/// The texts that are sent, in the order [`Request::with_variables`] visits them.
fn sent_texts(request: &Request) -> impl Iterator<Item = &str> {
    let form: &[KeyValue] = match &request.body {
        Body::FormData(fields) => fields,
        _ => &[],
    };
    let raw = match &request.body {
        Body::Raw { text, .. } => Some(text.as_str()),
        _ => None,
    };
    let auth: Vec<&str> = match &request.auth {
        Auth::None => Vec::new(),
        Auth::ApiKey { key, value } => vec![key, value],
        Auth::Basic { username, password } => vec![username, password],
        Auth::Bearer { token } => vec![token],
    };
    std::iter::once(request.url.as_str())
        .chain(sent_row_texts(&request.params))
        .chain(sent_row_texts(&request.headers))
        .chain(sent_row_texts(form))
        .chain(raw)
        .chain(auth)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::request::{HttpMethod, RawKind};

    fn row(enabled: bool, key: &str, value: &str) -> KeyValue {
        KeyValue {
            enabled,
            key: key.into(),
            value: value.into(),
        }
    }

    fn vars(rows: &[(&str, &str)]) -> Variables {
        let rows: Vec<_> = rows.iter().map(|(k, v)| row(true, k, v)).collect();
        Variables::from_rows(&rows)
    }

    fn sub(variables: &Variables, text: &str) -> (String, Vec<String>) {
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
            },
        }
    }

    const ALL_NAMES: [&str; 9] = ["base", "pk", "pv", "hk", "hv", "fk", "fv", "ak", "av"];

    #[test]
    fn with_variables_covers_every_field() {
        let rows: Vec<_> = ALL_NAMES
            .iter()
            .chain(&["p_off", "h_blank"])
            .map(|name| row(true, name, &name.to_uppercase()))
            .collect();
        let v = Variables::from_rows(&rows);
        let (out, unresolved) = templated_request().with_variables(&v);
        assert!(unresolved.is_empty());
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
        let (out, unresolved) = raw.with_variables(&v);
        assert!(unresolved.is_empty());
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
        let (out, _) = bearer.with_variables(&vars(&[("t", "secret")]));
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

        let (out, unresolved) = request.with_variables(&Variables::default());
        assert_eq!(out, request);
        assert_eq!(unresolved, expected);
        assert_eq!(
            request.unresolved_variables(&Variables::default()),
            expected
        );
    }

    #[test]
    fn unresolved_variables_matches_with_variables() {
        let request = templated_request();
        let v = vars(&[("pk", "1"), ("hv", "2"), ("ak", "3")]);
        assert_eq!(
            request.unresolved_variables(&v),
            request.with_variables(&v).1
        );
    }
}
