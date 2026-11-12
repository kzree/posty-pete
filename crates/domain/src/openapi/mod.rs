//! Converting an OpenAPI 3.x or Swagger 2.0 spec, as JSON or YAML text, into
//! a collection. The spec is read into a `serde_json::Value` and walked, so
//! both versions share most code.

mod convert;
mod document;
mod example;
pub mod load;
mod refs;
mod warnings;

#[cfg(test)]
mod tests;

use std::fmt;

use crate::collection::{Collection, SourceLocation};

/// The collection name when the spec has no title.
pub const DEFAULT_NAME: &str = "Imported API";

/// The collection variable that holds the server URL.
pub const BASE_URL_VARIABLE: &str = "baseUrl";

#[derive(Clone, Debug)]
pub struct ImportResult {
    /// With `source` set, and an `origin` on each request and folder.
    pub collection: Collection,
    /// Short, deduplicated notes on what didn't import fully.
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportError {
    /// Neither JSON nor YAML. `line` is 1-based.
    Parse {
        message: String,
        line: Option<usize>,
    },
    NotOpenApi,
    /// Names the version, like "Swagger 1.2".
    UnsupportedVersion(String),
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Parse {
                message,
                line: Some(line),
            } => write!(f, "Couldn't read the spec: {message} (line {line})."),
            Self::Parse {
                message,
                line: None,
            } => write!(f, "Couldn't read the spec: {message}."),
            Self::NotOpenApi => f.write_str("This isn't an OpenAPI or Swagger document."),
            Self::UnsupportedVersion(version) => write!(
                f,
                "{version} isn't supported. Use OpenAPI 3.0 or 3.1, or Swagger 2.0."
            ),
        }
    }
}

impl std::error::Error for ImportError {}

/// Converts spec `text` read from `location`, which the collection's source
/// records and relative server URLs resolve against.
pub fn import(text: &str, location: SourceLocation) -> Result<ImportResult, ImportError> {
    let root = document::parse(text)?;
    let version = document::version(&root)?;
    Ok(convert::convert(&root, version, location))
}
