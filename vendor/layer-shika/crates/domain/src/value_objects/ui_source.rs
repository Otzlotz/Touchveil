use std::path::PathBuf;

/// Source for UI definition at domain level
///
/// Lower-level alternative to `CompiledUiSource`. Typically used internally.
#[derive(Debug, Clone)]
pub enum UiSource {
    /// Load from a file path
    File(PathBuf),
    /// Parse from source code string
    Source(String),
}

impl UiSource {
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self::File(path.into())
    }

    pub fn source(code: impl Into<String>) -> Self {
        Self::Source(code.into())
    }
}

impl From<&str> for UiSource {
    fn from(s: &str) -> Self {
        Self::File(PathBuf::from(s))
    }
}

impl From<String> for UiSource {
    fn from(s: String) -> Self {
        Self::File(PathBuf::from(s))
    }
}

impl From<PathBuf> for UiSource {
    fn from(path: PathBuf) -> Self {
        Self::File(path)
    }
}
