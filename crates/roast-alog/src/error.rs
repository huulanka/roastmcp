use std::path::PathBuf;

use thiserror::Error;

use crate::ParseError;

/// Why an `.alog` file could not be read.
#[derive(Debug, Error)]
pub enum AlogError {
    /// The file is not a valid Python literal.
    #[error("not a valid Artisan profile: {0}")]
    Parse(#[from] ParseError),
    /// The file is a Python literal, but not a dictionary.
    #[error("not an Artisan profile: the file holds a {0}, not a dictionary")]
    NotADict(&'static str),
    /// A key without which the roast cannot be read is missing.
    #[error("not an Artisan profile: the key `{0}` is missing")]
    MissingKey(&'static str),
    /// A key holds a value of an unexpected type.
    #[error("the key `{key}` holds a {found}, expected {expected}")]
    WrongType {
        key: &'static str,
        expected: &'static str,
        found: &'static str,
    },
    /// The file could not be read.
    #[error("cannot read {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}
