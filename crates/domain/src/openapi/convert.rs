//! Mapping a parsed spec to a collection.

use std::collections::HashMap;

use chrono::Utc;
use serde_json::{Map, Value};

use super::{
    DEFAULT_NAME, ImportResult, document::Version, example, refs::Spec, warnings::Counted,
};
use crate::{
    collection::{
        Collection, CollectionSource, Folder, Item, Origin, SavedRequest, SourceKind,
        SourceLocation, auth_hash, value_hash,
    },
    request::{ApiKeyLocation, Auth, Body, HttpMethod, KeyValue, RawKind, Request},
};

const METHODS: [(&str, HttpMethod); 7] = [
    ("get", HttpMethod::Get),
    ("put", HttpMethod::Put),
    ("post", HttpMethod::Post),
    ("delete", HttpMethod::Delete),
    ("options", HttpMethod::Options),
    ("head", HttpMethod::Head),
    ("patch", HttpMethod::Patch),
];
const UNSUPPORTED_METHODS: [&str; 2] = ["trace", "query"];
/// OpenAPI says header parameters with these names are ignored.
const IGNORED_HEADERS: [&str; 3] = ["accept", "content-type", "authorization"];

pub(super) fn convert(root: &Value, version: Version, location: SourceLocation) -> ImportResult {
    let source_url = match &location {
        SourceLocation::Url(url) => url::Url::parse(url).ok(),
        SourceLocation::File(_) => None,
    };
    let mut converter = Converter {
        spec: Spec::new(root),
        version,
        source_url,
        auth_variables: Vec::new(),
        path_variables: Vec::new(),
        cell_bytes: example::MAX_BODY_BYTES,
    };
    if let Version::OpenApi3 { minor } = version
        && minor > 1
    {
        converter.spec.warnings.note(format!(
            "OpenAPI 3.{minor} is newer than 3.1; parts of it may not import."
        ));
    }

    let info = root.get("info");
    let text = |key| {
        info.and_then(|info| info.get(key))
            .map(value_text)
            .map(|text| text.trim().to_string())
            .unwrap_or_default()
    };
    let title = text("title");
    let spec_version = text("version");

    let base_url = converter.base_url();
    let global_security = root.get("security");
    let auth = converter.collection_auth(global_security);
    let operations = converter.operations();

    let mut id_counts: HashMap<&str, usize> = HashMap::new();
    for operation in &operations {
        if let Some(id) = operation.id() {
            *id_counts.entry(id).or_default() += 1;
        }
    }

    let mut groups: Vec<(String, Vec<Item>)> = root
        .get("tags")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|tag| tag.get("name")?.as_str())
        .map(|name| (name.to_string(), Vec::new()))
        .collect();
    let mut root_items = Vec::new();
    for operation in &operations {
        let mut saved = SavedRequest::new(
            operation.name(),
            converter.request(operation, global_security),
        );
        let key = match operation.id() {
            Some(id) if id_counts.get(id) == Some(&1) => id.to_string(),
            _ => operation.label(),
        };
        saved.origin = Some(Origin {
            key,
            hash: saved.content_hash(),
            untagged: operation.tag().is_none(),
        });
        let item = Item::Request(saved);
        match operation.tag() {
            Some(tag) => match groups.iter_mut().find(|(name, _)| name == tag) {
                Some((_, items)) => items.push(item),
                None => groups.push((tag.to_string(), vec![item])),
            },
            None => root_items.push(item),
        }
    }

    let mut collection = Collection::new(if title.is_empty() {
        DEFAULT_NAME
    } else {
        &title
    });
    collection.items = groups
        .into_iter()
        .filter(|(_, items)| !items.is_empty())
        .map(|(tag, items)| {
            let mut folder = Folder::new(tag.clone());
            folder.items = items;
            folder.origin = Some(Origin {
                key: tag,
                hash: folder.content_hash(),
                untagged: false,
            });
            Item::Folder(folder)
        })
        .chain(root_items)
        .collect();
    collection.auth = auth;
    collection.variables = converter.variables(base_url);
    collection.source = Some(CollectionSource {
        kind: SourceKind::OpenApi,
        location,
        title,
        version: spec_version,
        imported_at: Utc::now(),
        variables: collection
            .variables
            .iter()
            .map(|variable| Origin {
                key: variable.key.clone(),
                hash: value_hash(&variable.value),
                untagged: false,
            })
            .collect(),
        auth_hash: auth_hash(&collection.auth),
    });
    ImportResult {
        collection,
        warnings: converter.spec.warnings.into_messages(),
    }
}

struct Operation<'a> {
    method: HttpMethod,
    path: &'a str,
    item: &'a Value,
    op: &'a Value,
}

impl<'a> Operation<'a> {
    fn label(&self) -> String {
        format!("{} {}", self.method.as_str(), self.path)
    }

    fn id(&self) -> Option<&'a str> {
        self.text("operationId")
    }

    fn text(&self, key: &str) -> Option<&'a str> {
        let text = self.op.get(key)?.as_str()?.trim();
        (!text.is_empty()).then_some(text)
    }

    fn name(&self) -> String {
        self.text("summary")
            .or(self.id())
            .map_or_else(|| self.label(), str::to_string)
    }

    fn tag(&self) -> Option<&'a str> {
        let tag = self.op.get("tags")?.as_array()?.first()?.as_str()?;
        (!tag.trim().is_empty()).then_some(tag)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MediaClass {
    Json,
    Form,
    Multipart,
    Xml,
    Text,
}

impl MediaClass {
    const PRIORITY: [MediaClass; 5] = [
        Self::Json,
        Self::Form,
        Self::Multipart,
        Self::Xml,
        Self::Text,
    ];

    fn of(media_type: &str) -> Option<Self> {
        let base = base_media_type(media_type);
        if base == "application/x-www-form-urlencoded" {
            Some(Self::Form)
        } else if base == "multipart/form-data" {
            Some(Self::Multipart)
        } else if base.ends_with("/json") || base.ends_with("+json") {
            Some(Self::Json)
        } else if base.ends_with("/xml") || base.ends_with("+xml") {
            Some(Self::Xml)
        } else if base.starts_with("text/") {
            Some(Self::Text)
        } else {
            None
        }
    }
}

fn base_media_type(media_type: &str) -> String {
    media_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}

/// The raw kind for a JSON, XML or text media type.
fn raw_kind(class: MediaClass, media_type: &str) -> RawKind {
    match class {
        MediaClass::Json => RawKind::Json,
        MediaClass::Xml => RawKind::Xml,
        _ if base_media_type(media_type) == "text/html" => RawKind::Html,
        _ => RawKind::Text,
    }
}

/// A Content-Type header for a raw body whose media type isn't the one the
/// kind sends anyway.
fn content_type_header(kind: RawKind, media_type: &str) -> Option<KeyValue> {
    let base = base_media_type(media_type);
    let default = base_media_type(kind.mime());
    (base != default && !media_type.contains('*')).then(|| row(true, "Content-Type", media_type))
}

enum SchemeAuth {
    Supported(Auth),
    Unsupported(String),
}

struct Converter<'a> {
    spec: Spec<'a>,
    version: Version,
    source_url: Option<url::Url>,
    auth_variables: Vec<&'static str>,
    path_variables: Vec<KeyValue>,
    /// What's left of the current operation's budget for parameter and form
    /// field values.
    cell_bytes: usize,
}

impl<'a> Converter<'a> {
    fn base_url(&mut self) -> String {
        let root = self.spec.root;
        let raw = match self.version {
            Version::OpenApi3 { .. } => {
                let server = root
                    .get("servers")
                    .and_then(Value::as_array)
                    .and_then(|servers| servers.first());
                match server.and_then(|server| server.get("url")?.as_str()) {
                    Some(url) => {
                        fill_server_variables(url, server.and_then(|s| s.get("variables")))
                    }
                    None => "/".to_string(),
                }
            }
            Version::Swagger2 => {
                let base_path = root.get("basePath").and_then(Value::as_str).unwrap_or("");
                let source_host = self
                    .source_url
                    .as_ref()
                    .filter(|url| url.has_host())
                    .map(|url| &url[url::Position::BeforeHost..url::Position::AfterPort]);
                let host = root.get("host").and_then(Value::as_str).or(source_host);
                let scheme = root
                    .get("schemes")
                    .and_then(Value::as_array)
                    .and_then(|schemes| schemes.first()?.as_str())
                    .or(self.source_url.as_ref().map(url::Url::scheme))
                    .unwrap_or("https");
                match host {
                    Some(host) => format!("{scheme}://{host}{base_path}"),
                    None => base_path.to_string(),
                }
            }
        };
        let resolved = if is_absolute(&raw) {
            raw
        } else if let Some(joined) = self
            .source_url
            .as_ref()
            .and_then(|base| base.join(&raw).ok())
        {
            joined.to_string()
        } else {
            if raw.trim_matches('/').is_empty() {
                self.spec
                    .warnings
                    .note("The spec doesn't say where the API is; set baseUrl.");
            } else {
                self.spec.warnings.note(format!(
                    "The server URL \"{raw}\" is relative; set baseUrl to the full URL."
                ));
            }
            raw
        };
        resolved.trim_end_matches('/').to_string()
    }

    fn collection_auth(&mut self, security: Option<&'a Value>) -> Auth {
        let Some(security) = security else {
            return Auth::None;
        };
        match self.requirement(security, None) {
            Ok(auth) => auth,
            Err(label) => {
                self.spec.warnings.note(format!(
                    "The API uses {label}, which isn't supported; the collection's auth was left empty."
                ));
                Auth::None
            }
        }
    }

    fn operations(&mut self) -> Vec<Operation<'a>> {
        let mut operations = Vec::new();
        let Some(paths) = self.spec.root.get("paths").and_then(Value::as_object) else {
            return operations;
        };
        for (path, item) in paths {
            if !path.starts_with('/') {
                continue;
            }
            let Some(item) = self.spec.resolve(item) else {
                continue;
            };
            for (key, op) in item.as_object().into_iter().flatten() {
                if let Some((_, method)) = METHODS.iter().find(|(name, _)| name == key) {
                    if op.is_object() {
                        operations.push(Operation {
                            method: *method,
                            path,
                            item,
                            op,
                        });
                    }
                } else if UNSUPPORTED_METHODS.contains(&key.as_str()) {
                    let method = key.to_ascii_uppercase();
                    let label = format!("{method} {path}");
                    self.spec
                        .warnings
                        .count(Counted::SkippedMethod(method), label);
                }
            }
        }
        operations
    }

    fn request(&mut self, operation: &Operation<'a>, global_security: Option<&Value>) -> Request {
        let label = operation.label();
        self.cell_bytes = example::MAX_BODY_BYTES;
        let mut request = Request {
            method: operation.method,
            url: format!("{{{{baseUrl}}}}{}", path_with_variables(operation.path)),
            ..Request::default()
        };
        let mut path_values = HashMap::new();
        let mut body_param = None;
        let mut form_params = Vec::new();
        for param in self.parameters(operation) {
            let Some(name) = param.get("name").and_then(Value::as_str) else {
                continue;
            };
            let required = param.get("required").and_then(Value::as_bool) == Some(true);
            match param.get("in").and_then(Value::as_str) {
                Some("path") => {
                    path_values.insert(name, self.param_value(param, &label));
                }
                Some("query") => {
                    let value = self.param_value(param, &label);
                    request.params.push(row(required, name, value));
                }
                Some("header")
                    if !IGNORED_HEADERS.contains(&name.to_ascii_lowercase().as_str()) =>
                {
                    let value = self.param_value(param, &label);
                    request.headers.push(row(required, name, value));
                }
                Some("cookie") => self
                    .spec
                    .warnings
                    .count(Counted::CookieParam, format!("{label} {name}")),
                Some("body") => body_param = Some(param),
                Some("formData") => form_params.push(param),
                _ => {}
            }
        }
        for name in path_template_names(operation.path) {
            let value = path_values.get(name).cloned().unwrap_or_default();
            self.add_path_variable(name, value);
        }

        let (body, content_type) = match self.version {
            Version::OpenApi3 { .. } => self.body_v3(operation.op, &label),
            Version::Swagger2 => self.body_v2(operation.op, body_param, &form_params, &label),
        };
        request.body = body;
        let has_content_type = request
            .headers
            .iter()
            .any(|header| header.key.eq_ignore_ascii_case("content-type"));
        if let Some(header) = content_type.filter(|_| !has_content_type) {
            request.headers.push(header);
        }

        request.auth = match operation.op.get("security") {
            None => Auth::Inherit,
            Some(security) if Some(security) == global_security => Auth::Inherit,
            Some(security) => match self.requirement(security, Some(&label)) {
                Ok(auth) => auth,
                Err(scheme) => {
                    self.spec
                        .warnings
                        .count(Counted::UnsupportedOperationAuth(scheme), label);
                    Auth::Inherit
                }
            },
        };
        request
    }

    /// Path-level parameters overridden by operation-level ones with the same
    /// name and location.
    fn parameters(&mut self, operation: &Operation<'a>) -> Vec<&'a Value> {
        let mut merged: Vec<&'a Value> = Vec::new();
        for source in [operation.item, operation.op] {
            let params = source.get("parameters").and_then(Value::as_array);
            for param in params.into_iter().flatten() {
                let Some(param) = self.spec.resolve(param) else {
                    continue;
                };
                let key = |p: &Value| (p.get("name").cloned(), p.get("in").cloned());
                match merged.iter_mut().find(|seen| key(seen) == key(param)) {
                    Some(seen) => *seen = param,
                    None => merged.push(param),
                }
            }
        }
        merged
    }

    /// The parameter's example, default or first enum value, else empty.
    fn param_value(&mut self, param: &'a Value, label: &str) -> String {
        let value = match param.get("example").or_else(|| param.get("x-example")) {
            Some(example) => Some(example),
            None => self.first_example(param).or_else(|| {
                param
                    .get("schema")
                    .and_then(|schema| self.spec.resolve(schema))
                    .and_then(example::given)
                    .or_else(|| example::given(param))
            }),
        };
        value.map_or_else(String::new, |value| self.cell_text(value, label))
    }

    /// [`value_text`] within the operation's budget; empty once it runs out.
    fn cell_text(&mut self, value: &Value, label: &str) -> String {
        match value_text_within(value, self.cell_bytes) {
            Some(text) => {
                self.cell_bytes -= text.len();
                text
            }
            None => {
                self.cell_bytes = 0;
                self.spec.warnings.count(Counted::OversizedExample, label);
                String::new()
            }
        }
    }

    /// The `value` of the first entry in an `examples` map.
    fn first_example(&mut self, object: &'a Value) -> Option<&'a Value> {
        let first = object.get("examples")?.as_object()?.values().next()?;
        self.spec.resolve(first)?.get("value")
    }

    fn add_path_variable(&mut self, name: &str, value: String) {
        match self.path_variables.iter_mut().find(|row| row.key == name) {
            Some(row) if row.value.is_empty() => row.value = value,
            Some(_) => {}
            None => self.path_variables.push(row(true, name, value)),
        }
    }

    fn body_v3(&mut self, op: &'a Value, label: &str) -> (Body, Option<KeyValue>) {
        let content = self
            .spec
            .get(op, "requestBody")
            .and_then(|body| body.get("content"))
            .and_then(Value::as_object);
        let Some(content) = content else {
            return (Body::None, None);
        };
        let picked = MediaClass::PRIORITY.into_iter().find_map(|class| {
            content
                .iter()
                .find(|(media_type, _)| MediaClass::of(media_type) == Some(class))
                .map(|(media_type, media)| (class, media_type, media))
        });
        let Some((class, media_type, media)) = picked else {
            if let Some(media_type) = content.keys().next() {
                let kind = Counted::UnsupportedBody(base_media_type(media_type));
                self.spec.warnings.count(kind, label);
            }
            return (Body::None, None);
        };
        match class {
            MediaClass::Form | MediaClass::Multipart => {
                let fields = self.schema_fields(media, label);
                let body = if class == MediaClass::Form {
                    Body::UrlEncoded(fields)
                } else {
                    Body::FormData(fields)
                };
                (body, None)
            }
            MediaClass::Json => {
                let text = self.json_example(media, label);
                (
                    Body::Raw {
                        kind: RawKind::Json,
                        text,
                    },
                    content_type_header(RawKind::Json, media_type),
                )
            }
            MediaClass::Xml | MediaClass::Text => {
                let kind = raw_kind(class, media_type);
                let text = media
                    .get("example")
                    .or_else(|| self.first_example(media))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let end = text.floor_char_boundary(example::MAX_BODY_BYTES);
                if end < text.len() {
                    self.spec.warnings.count(Counted::OversizedExample, label);
                }
                let text = text[..end].to_string();
                (
                    Body::Raw { kind, text },
                    content_type_header(kind, media_type),
                )
            }
        }
    }

    fn json_example(&mut self, media: &'a Value, label: &str) -> String {
        if let Some(example) = media.get("example").or_else(|| self.first_example(media)) {
            let example = match parsed(example) {
                Some(value) => example::bounded(&value),
                None => example::bounded(example),
            };
            return self.json_text(example, label);
        }
        let Some(schema) = media.get("schema") else {
            return String::new();
        };
        self.generated_json(schema, label)
    }

    fn generated_json(&mut self, schema: &'a Value, label: &str) -> String {
        let mut example = example::generate(&mut self.spec, schema);
        if let Some(value) = parsed(&example.value) {
            let reparsed = example::bounded(&value);
            example.value = reparsed.value;
            example.oversized |= reparsed.oversized;
        }
        self.json_text(example, label)
    }

    fn json_text(&mut self, example: example::Example, label: &str) -> String {
        if example.truncated {
            self.spec.warnings.count(Counted::TruncatedExample, label);
        }
        let (text, cut) = pretty(&example.value);
        if example.oversized || cut {
            self.spec.warnings.count(Counted::OversizedExample, label);
        }
        text
    }

    /// Form fields from an object schema's properties.
    fn schema_fields(&mut self, media: &'a Value, label: &str) -> Vec<KeyValue> {
        let example = media.get("example").and_then(Value::as_object);
        let Some(schema) = media.get("schema") else {
            return Vec::new();
        };
        let mut properties = Vec::new();
        let mut required = Vec::new();
        self.collect_properties(schema, &mut properties, &mut required, 0);
        properties
            .into_iter()
            .map(|(name, property)| {
                let file = property.is_some_and(is_file);
                let value = if file {
                    self.spec
                        .warnings
                        .count(Counted::FileField, format!("{label} {name}"));
                    String::new()
                } else {
                    example
                        .and_then(|example| example.get(name))
                        .or_else(|| property.and_then(example::given))
                        .map_or_else(String::new, |value| self.cell_text(value, label))
                };
                row(required.contains(&name), name, value)
            })
            .collect()
    }

    fn collect_properties(
        &mut self,
        schema: &'a Value,
        properties: &mut Vec<(&'a str, Option<&'a Value>)>,
        required: &mut Vec<&'a str>,
        depth: usize,
    ) {
        if depth > example::MAX_DEPTH {
            return;
        }
        let Some(schema) = self.spec.resolve(schema) else {
            return;
        };
        for part in schema
            .get("allOf")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            self.collect_properties(part, properties, required, depth + 1);
        }
        let names = schema.get("required").and_then(Value::as_array);
        required.extend(names.into_iter().flatten().filter_map(Value::as_str));
        for (name, property) in schema
            .get("properties")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            if !properties.iter().any(|(seen, _)| seen == name) {
                properties.push((name, self.spec.resolve(property)));
            }
        }
    }

    fn body_v2(
        &mut self,
        op: &'a Value,
        body_param: Option<&'a Value>,
        form_params: &[&'a Value],
        label: &str,
    ) -> (Body, Option<KeyValue>) {
        let consumes: Vec<&str> = op
            .get("consumes")
            .or_else(|| self.spec.root.get("consumes"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        let find = |class| {
            consumes
                .iter()
                .copied()
                .find(|mt| MediaClass::of(mt) == Some(class))
        };

        if let Some(param) = body_param {
            let json = find(MediaClass::Json);
            let (class, media_type) = match (json, find(MediaClass::Xml), find(MediaClass::Text)) {
                (Some(json), _, _) => (MediaClass::Json, json),
                _ if consumes.is_empty() => (MediaClass::Json, "application/json"),
                (None, Some(xml), _) => (MediaClass::Xml, xml),
                (None, None, Some(text)) => (MediaClass::Text, text),
                (None, None, None) => {
                    let kind = Counted::UnsupportedBody(base_media_type(consumes[0]));
                    self.spec.warnings.count(kind, label);
                    return (Body::None, None);
                }
            };
            let kind = raw_kind(class, media_type);
            let text = match (class, param.get("schema")) {
                (MediaClass::Json, Some(schema)) => self.generated_json(schema, label),
                _ => String::new(),
            };
            return (
                Body::Raw { kind, text },
                content_type_header(kind, media_type),
            );
        }

        if form_params.is_empty() {
            return (Body::None, None);
        }
        let mut multipart = find(MediaClass::Multipart).is_some();
        let fields = form_params
            .iter()
            .filter_map(|&param| {
                let name = param.get("name")?.as_str()?;
                let required = param.get("required").and_then(Value::as_bool) == Some(true);
                let value = if param.get("type").and_then(Value::as_str) == Some("file") {
                    multipart = true;
                    self.spec
                        .warnings
                        .count(Counted::FileField, format!("{label} {name}"));
                    String::new()
                } else {
                    self.param_value(param, label)
                };
                Some(row(required, name, value))
            })
            .collect();
        let body = if multipart {
            Body::FormData(fields)
        } else {
            Body::UrlEncoded(fields)
        };
        (body, None)
    }

    /// The auth for a security requirement list: the first alternative whose
    /// scheme is supported, or No auth for `[]`. `Err` names the unsupported
    /// scheme when there's nothing else. `label` names the operation, `None`
    /// for the global requirement.
    fn requirement(&mut self, security: &'a Value, label: Option<&str>) -> Result<Auth, String> {
        let Some(alternatives) = security.as_array() else {
            return Ok(Auth::None);
        };
        let mut unsupported = None;
        let mut anonymous = false;
        for alternative in alternatives.iter().filter_map(Value::as_object) {
            let Some(name) = alternative.keys().next() else {
                anonymous = true;
                continue;
            };
            match self.scheme(name) {
                Some(SchemeAuth::Supported(auth)) => {
                    match label {
                        _ if alternative.len() == 1 => {}
                        Some(label) => self.spec.warnings.count(Counted::CombinedSchemes, label),
                        None => self.spec.warnings.note(
                            "The API requires several security schemes together; only the first was used.",
                        ),
                    }
                    self.add_auth_variables(&auth);
                    return Ok(auth);
                }
                Some(SchemeAuth::Unsupported(scheme)) => {
                    unsupported.get_or_insert(scheme);
                }
                None => {}
            }
        }
        match unsupported {
            Some(scheme) if !anonymous => Err(scheme),
            _ => Ok(Auth::None),
        }
    }

    fn scheme(&mut self, name: &str) -> Option<SchemeAuth> {
        let root = self.spec.root;
        let definitions = match self.version {
            Version::OpenApi3 { .. } => root
                .get("components")
                .and_then(|c| c.get("securitySchemes")),
            Version::Swagger2 => root.get("securityDefinitions"),
        };
        let Some(definition) = definitions.and_then(|definitions| definitions.get(name)) else {
            self.spec
                .warnings
                .note(format!("The security scheme \"{name}\" isn't defined."));
            return None;
        };
        let definition = self.spec.resolve(definition)?;
        let text = |key| {
            definition
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
        };
        let auth = match text("type") {
            "apiKey" => {
                let location = match text("in") {
                    "header" => ApiKeyLocation::Header,
                    "query" => ApiKeyLocation::Query,
                    _ => return Some(SchemeAuth::Unsupported("an API key in a cookie".into())),
                };
                Auth::ApiKey {
                    key: text("name").to_string(),
                    value: "{{apiKey}}".into(),
                    location,
                }
            }
            "http" => match text("scheme").to_ascii_lowercase().as_str() {
                "bearer" => bearer(),
                "basic" => basic(),
                other => return Some(SchemeAuth::Unsupported(format!("HTTP {other} auth"))),
            },
            "basic" => basic(),
            "oauth2" => return Some(SchemeAuth::Unsupported("OAuth 2.0".into())),
            "openIdConnect" => return Some(SchemeAuth::Unsupported("OpenID Connect".into())),
            "mutualTLS" => return Some(SchemeAuth::Unsupported("mutual TLS".into())),
            other => {
                return Some(SchemeAuth::Unsupported(format!(
                    "the \"{other}\" security scheme"
                )));
            }
        };
        Some(SchemeAuth::Supported(auth))
    }

    fn add_auth_variables(&mut self, auth: &Auth) {
        let names: &[&'static str] = match auth {
            Auth::ApiKey { .. } => &["apiKey"],
            Auth::Bearer { .. } => &["token"],
            Auth::Basic { .. } => &["username", "password"],
            Auth::Inherit | Auth::None => &[],
        };
        for name in names {
            if !self.auth_variables.contains(name) {
                self.auth_variables.push(name);
            }
        }
    }

    /// `baseUrl`, then the auth variables (empty), then the path variables.
    fn variables(&mut self, base_url: String) -> Vec<KeyValue> {
        let mut variables = vec![row(true, super::BASE_URL_VARIABLE, base_url)];
        let auth = self
            .auth_variables
            .iter()
            .map(|name| row(true, name, String::new()));
        for variable in auth.chain(self.path_variables.drain(..)) {
            if !variables.iter().any(|seen| seen.key == variable.key) {
                variables.push(variable);
            }
        }
        variables
    }
}

fn bearer() -> Auth {
    Auth::Bearer {
        token: "{{token}}".into(),
    }
}

fn basic() -> Auth {
    Auth::Basic {
        username: "{{username}}".into(),
        password: "{{password}}".into(),
    }
}

fn row(enabled: bool, key: &str, value: impl Into<String>) -> KeyValue {
    KeyValue {
        enabled,
        key: key.to_string(),
        value: value.into(),
    }
}

fn is_absolute(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|url| !url.cannot_be_a_base())
}

fn is_file(schema: &Value) -> bool {
    let format = schema.get("format").and_then(Value::as_str);
    match example::schema_type(schema) {
        Some("file") => true,
        Some("string") => {
            matches!(format, Some("binary" | "base64"))
                || schema.get("contentMediaType").is_some()
                || schema.get("contentEncoding").is_some()
        }
        Some("array") => schema.get("items").is_some_and(is_file),
        _ => false,
    }
}

/// Text for a table cell: strings as-is, lists comma-separated, other values
/// as JSON.
fn value_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Array(items) => items.iter().map(value_text).collect::<Vec<_>>().join(","),
        other => other.to_string(),
    }
}

/// [`value_text`], or `None` if it's longer than `limit` bytes.
fn value_text_within(value: &Value, limit: usize) -> Option<String> {
    match value {
        Value::Null => Some(String::new()),
        Value::String(s) => (s.len() <= limit).then(|| s.clone()),
        Value::Array(items) => {
            let mut text = String::new();
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    text.push(',');
                }
                let left = limit.checked_sub(text.len())?;
                text.push_str(&value_text_within(item, left)?);
            }
            (text.len() <= limit).then_some(text)
        }
        other => {
            let mut out = Capped::new(limit);
            serde_json::to_writer(&mut out, other).ok()?;
            String::from_utf8(out.bytes).ok()
        }
    }
}

/// A string example that holds JSON, parsed.
fn parsed(value: &Value) -> Option<Value> {
    serde_json::from_str(value.as_str()?).ok()
}

/// Pretty JSON, cut at [`example::MAX_BODY_BYTES`]; `true` if it was cut.
fn pretty(value: &Value) -> (String, bool) {
    let mut out = Capped::new(example::MAX_BODY_BYTES);
    let cut = serde_json::to_writer_pretty(&mut out, value).is_err();
    let bytes = out.bytes;
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(err) => {
            let valid = err.utf8_error().valid_up_to();
            let mut bytes = err.into_bytes();
            bytes.truncate(valid);
            String::from_utf8(bytes).unwrap_or_default()
        }
    };
    (text, cut)
}

/// A buffer that refuses writes past `limit` bytes.
struct Capped {
    bytes: Vec<u8>,
    limit: usize,
}

impl Capped {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
        }
    }
}

impl std::io::Write for Capped {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let room = self.limit - self.bytes.len();
        if room == 0 && !buf.is_empty() {
            return Err(std::io::ErrorKind::FileTooLarge.into());
        }
        let n = buf.len().min(room);
        self.bytes.extend_from_slice(&buf[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The `{name}`s in a path or server URL template, in order.
fn template_names(template: &str) -> impl Iterator<Item = &str> {
    template.split('{').skip(1).filter_map(|rest| {
        let name = &rest[..rest.find('}')?];
        (!name.is_empty()).then_some(name)
    })
}

fn path_template_names(path: &str) -> Vec<&str> {
    template_names(path).collect()
}

fn path_with_variables(path: &str) -> String {
    let mut out = String::with_capacity(path.len() + 8);
    let mut rest = path;
    while let Some(start) = rest.find('{') {
        let Some(len) = rest[start..].find('}') else {
            break;
        };
        out.push_str(&rest[..start]);
        out.push('{');
        out.push_str(&rest[start..=start + len]);
        out.push('}');
        rest = &rest[start + len + 1..];
    }
    out.push_str(rest);
    out
}

fn fill_server_variables(url: &str, variables: Option<&Value>) -> String {
    let mut out = url.to_string();
    let variables = variables.and_then(Value::as_object);
    for name in template_names(url) {
        let value = variables
            .and_then(|variables: &Map<String, Value>| variables.get(name))
            .and_then(|variable| {
                variable
                    .get("default")
                    .or_else(|| variable.get("enum")?.as_array()?.first())
            })
            .map(value_text);
        if let Some(value) = value {
            out = out.replacen(&format!("{{{name}}}"), &value, 1);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn pretty_bodies_are_capped() {
        let value = json!(["é".repeat(example::MAX_BODY_BYTES)]);
        let (text, cut) = pretty(&value);
        assert!(cut);
        assert!(text.len() <= example::MAX_BODY_BYTES);
        assert!(text.len() > example::MAX_BODY_BYTES - 4);

        assert_eq!(pretty(&json!({"a": 1})), ("{\n  \"a\": 1\n}".into(), false));
    }
}
