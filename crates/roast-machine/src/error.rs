//! Errors of this crate.

use std::fmt;
use std::path::PathBuf;

/// Loading or saving a machine profile failed.
#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    /// A file or directory could not be read.
    #[error("cannot read {}: {source}", path.display())]
    Io {
        /// The file or directory.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
    /// The TOML is malformed or does not match the schema.
    #[error("{origin}: {source}")]
    Toml {
        /// The file name, or `profile` for text.
        origin: String,
        /// The parser error, with line and column.
        #[source]
        source: toml::de::Error,
    },
    /// The profile parsed but breaks one or more rules.
    #[error("{origin} is invalid:\n{source}")]
    Invalid {
        /// The file name or profile id.
        origin: String,
        /// Every rule that is broken.
        #[source]
        source: ValidationErrors,
    },
    /// The file name does not match the profile id.
    #[error("{}: profile id `{id}` must match the file name (`{id}.toml`)", path.display())]
    IdMismatch {
        /// The file.
        path: PathBuf,
        /// The id inside the file.
        id: String,
    },
    /// The profile cannot be written as TOML.
    #[error("cannot serialize profile: {0}")]
    Serialize(#[from] toml::ser::Error),
}

/// One broken rule of a profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationIssue {
    /// Dotted path of the offending field, for example `safe_state.drum`.
    pub field: String,
    /// What is wrong, for a person to read.
    pub message: String,
}

impl fmt::Display for ValidationIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

/// Every broken rule of a profile, reported together.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub struct ValidationErrors {
    /// The broken rules, in the order they were checked.
    pub issues: Vec<ValidationIssue>,
}

impl ValidationErrors {
    /// Whether any issue concerns this field (or a field below it).
    #[must_use]
    pub fn mentions(&self, field: &str) -> bool {
        self.issues.iter().any(|i| i.field.starts_with(field))
    }
}

impl fmt::Display for ValidationErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, issue) in self.issues.iter().enumerate() {
            if i > 0 {
                f.write_str("\n")?;
            }
            write!(f, "  - {issue}")?;
        }
        Ok(())
    }
}

/// An Artisan settings file could not be imported.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ImportError {
    /// The file has no `[Device] id`, so it is probably not an `.aset` file.
    #[error("no [Device] id found; is this an Artisan settings (.aset) file?")]
    NoDevice,
    /// The file configures a device other than TC4.
    #[error("Artisan device id {0} is not supported; only TC4 (id 19) can be imported")]
    UnsupportedDevice(String),
}
