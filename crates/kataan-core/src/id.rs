use std::{
    fmt,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CanonicalId(String);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CanonicalIdError {
    #[error("canonical ID must be a relative path with URL-safe path segments: {0}")]
    InvalidShape(String),
    #[error("canonical ID contains an invalid path segment: {0}")]
    InvalidSegment(String),
    #[error("document path must end in .md or .toml: {0}")]
    InvalidExtension(String),
    #[error("root index is not a document ID: {0}")]
    RootIndex(String),
}

impl CanonicalId {
    pub fn parse(value: impl AsRef<str>) -> Result<Self, CanonicalIdError> {
        let value = normalize_separators(value.as_ref());

        if value.is_empty()
            || value.starts_with('/')
            || value.ends_with('/')
            || value.contains("//")
            || value.split('/').any(str::is_empty)
            || value.split('/').any(|segment| segment.contains('.'))
        {
            return Err(CanonicalIdError::InvalidShape(value));
        }

        if !value.split('/').all(is_canonical_segment) {
            return Err(CanonicalIdError::InvalidSegment(value));
        }

        Ok(Self(value))
    }

    pub fn from_document_path(path: impl AsRef<Path>) -> Result<Self, CanonicalIdError> {
        let path = path.as_ref();
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .ok_or_else(|| CanonicalIdError::InvalidExtension(path.display().to_string()))?;

        if extension != "md" && extension != "toml" {
            return Err(CanonicalIdError::InvalidExtension(
                path.display().to_string(),
            ));
        }

        let file_stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or_else(|| CanonicalIdError::InvalidShape(path.display().to_string()))?;

        if file_stem == "index" {
            let Some(parent) = path.parent() else {
                return Err(CanonicalIdError::RootIndex(path.display().to_string()));
            };
            let value = normalize_separators(&parent.to_string_lossy());
            if value.is_empty() || value == "." {
                return Err(CanonicalIdError::RootIndex(path.display().to_string()));
            }
            return Self::parse(value);
        }

        let without_extension = path.with_extension("");
        let value = normalize_separators(&without_extension.to_string_lossy());
        Self::parse(value)
    }

    pub fn top_level_folder(&self) -> &str {
        self.0.split('/').next().expect("validated canonical id")
    }

    pub fn containing_folder(&self) -> &str {
        self.0
            .rsplit_once('/')
            .map(|(folder, _)| folder)
            .unwrap_or("")
    }

    pub fn folder(&self) -> &str {
        self.containing_folder()
    }

    pub fn slug(&self) -> &str {
        self.0
            .rsplit_once('/')
            .map(|(_, slug)| slug)
            .unwrap_or(self.0.as_str())
    }

    pub fn ancestors(&self) -> Vec<&str> {
        let segments = self.0.split('/').collect::<Vec<_>>();
        if segments.len() <= 2 {
            return Vec::new();
        }
        segments[1..segments.len() - 1].to_vec()
    }

    pub fn path_keywords(&self) -> Vec<&str> {
        self.0.split('/').skip(1).collect()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn markdown_path(&self) -> PathBuf {
        regular_path(self.containing_folder(), self.slug(), "md")
    }

    pub fn toml_path(&self) -> PathBuf {
        regular_path(self.containing_folder(), self.slug(), "toml")
    }

    pub fn folder_index_markdown_path(&self) -> PathBuf {
        PathBuf::from(self.as_str()).join("index.md")
    }

    pub fn folder_index_toml_path(&self) -> PathBuf {
        PathBuf::from(self.as_str()).join("index.toml")
    }
}

impl fmt::Display for CanonicalId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

fn regular_path(folder: &str, slug: &str, extension: &str) -> PathBuf {
    if folder.is_empty() {
        PathBuf::from(format!("{slug}.{extension}"))
    } else {
        PathBuf::from(folder).join(format!("{slug}.{extension}"))
    }
}

fn normalize_separators(value: &str) -> String {
    value.replace('\\', "/")
}

fn is_canonical_segment(value: &str) -> bool {
    if value.is_empty() || value.starts_with('-') || value.ends_with('-') || value.contains("--") {
        return false;
    }

    value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_top_level_folder_id() {
        let id = CanonicalId::parse("projects").unwrap();
        assert_eq!(id.top_level_folder(), "projects");
        assert_eq!(id.containing_folder(), "");
        assert_eq!(id.slug(), "projects");
        assert_eq!(id.as_str(), "projects");
    }

    #[test]
    fn parses_nested_canonical_id() {
        let id = CanonicalId::parse("projects/company-x/internal/q2-launch").unwrap();
        assert_eq!(id.top_level_folder(), "projects");
        assert_eq!(id.folder(), "projects/company-x/internal");
        assert_eq!(id.slug(), "q2-launch");
        assert_eq!(id.ancestors(), vec!["company-x", "internal"]);
        assert_eq!(
            id.path_keywords(),
            vec!["company-x", "internal", "q2-launch"]
        );
    }

    #[test]
    fn permits_mixed_case_document_ids() {
        let id =
            CanonicalId::parse("projects/snappy/sows/otp-travel/HU-otp-travel-POC-SOW1-260429")
                .unwrap();
        assert_eq!(
            id.as_str(),
            "projects/snappy/sows/otp-travel/HU-otp-travel-POC-SOW1-260429"
        );
    }

    #[test]
    fn rejects_invalid_canonical_ids() {
        for value in [
            "",
            "projects/kataan_redesign",
            "projects/.hidden",
            "projects/kataan.md",
            "/projects/kataan",
            "projects/kataan/",
            "projects//kataan",
        ] {
            assert!(
                CanonicalId::parse(value).is_err(),
                "{value} should be invalid"
            );
        }
    }

    #[test]
    fn converts_regular_document_ids_and_paths() {
        let id = CanonicalId::parse("projects/company-x/q2-launch").unwrap();
        assert_eq!(
            id.markdown_path().to_string_lossy(),
            "projects/company-x/q2-launch.md"
        );
        assert_eq!(
            id.toml_path().to_string_lossy(),
            "projects/company-x/q2-launch.toml"
        );
        assert_eq!(
            CanonicalId::from_document_path("projects/company-x/q2-launch.md").unwrap(),
            id
        );
        assert_eq!(
            CanonicalId::from_document_path("projects\\company-x\\q2-launch.toml").unwrap(),
            id
        );
    }

    #[test]
    fn converts_folder_index_paths_to_folder_ids() {
        let id = CanonicalId::parse("projects/company-x").unwrap();
        assert_eq!(
            id.folder_index_markdown_path().to_string_lossy(),
            "projects/company-x/index.md"
        );
        assert_eq!(
            id.folder_index_toml_path().to_string_lossy(),
            "projects/company-x/index.toml"
        );
        assert_eq!(
            CanonicalId::from_document_path("projects/company-x/index.md").unwrap(),
            id
        );
        assert_eq!(
            CanonicalId::from_document_path("projects/company-x/index.toml").unwrap(),
            id
        );
    }

    #[test]
    fn rejects_root_index_paths_as_document_ids() {
        assert!(CanonicalId::from_document_path("index.md").is_err());
        assert!(CanonicalId::from_document_path("index.toml").is_err());
    }
}
