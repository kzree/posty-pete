use mime::Mime;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyFormat {
    Json,
    Html,
    Xml,
    Css,
    JavaScript,
    Yaml,
    Text,
}

impl BodyFormat {
    pub fn from_content_type(content_type: Option<&str>) -> Self {
        let Some(mime) = content_type.and_then(|ct| ct.parse::<Mime>().ok()) else {
            return Self::Text;
        };

        let (ty, subtype, suffix) = (mime.type_(), mime.subtype(), mime.suffix());

        if subtype == mime::JSON || suffix == Some(mime::JSON) {
            Self::Json
        } else if subtype == mime::XML || suffix == Some(mime::XML) {
            Self::Xml
        } else if ty == mime::TEXT && subtype == mime::HTML {
            Self::Html
        } else if ty == mime::TEXT && subtype == mime::CSS {
            Self::Css
        } else if subtype == mime::JAVASCRIPT {
            Self::JavaScript
        } else if subtype == "yaml" || subtype == "x-yaml" || suffix.is_some_and(|s| s == "yaml") {
            // mime has no YAML constant
            Self::Yaml
        } else {
            Self::Text
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_from_content_type() {
        for (content_type, expected) in [
            (None, BodyFormat::Text),
            (Some("not a mime"), BodyFormat::Text),
            (Some("application/json; charset=utf-8"), BodyFormat::Json),
            (Some("application/problem+json"), BodyFormat::Json),
            (Some("application/xml"), BodyFormat::Xml),
            (Some("text/xml"), BodyFormat::Xml),
            (Some("image/svg+xml"), BodyFormat::Xml),
            (Some("text/html"), BodyFormat::Html),
            (Some("text/css"), BodyFormat::Css),
            (Some("application/javascript"), BodyFormat::JavaScript),
            (Some("text/javascript"), BodyFormat::JavaScript),
            (Some("application/yaml"), BodyFormat::Yaml),
            (Some("application/x-yaml"), BodyFormat::Yaml),
            (Some("text/plain"), BodyFormat::Text),
            (Some("application/octet-stream"), BodyFormat::Text),
        ] {
            assert_eq!(
                BodyFormat::from_content_type(content_type),
                expected,
                "{content_type:?}"
            );
        }
    }
}
