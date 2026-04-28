use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CanonicalId(String);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CanonicalIdError {
    #[error("canonical ID must be folder/slug: {0}")]
    InvalidShape(String),
    #[error("canonical ID contains an invalid folder or slug: {0}")]
    InvalidSlug(String),
    #[error("document path must end in .md or .toml: {0}")]
    InvalidExtension(String),
}

impl CanonicalId {
    pub fn parse(value: impl AsRef<str>) -> Result<Self, CanonicalIdError> {
        let value = value.as_ref();
        let Some((folder, slug)) = value.split_once('/') else {
            return Err(CanonicalIdError::InvalidShape(value.to_owned()));
        };

        if folder.is_empty() || slug.is_empty() || slug.contains('/') || value.starts_with('/') {
            return Err(CanonicalIdError::InvalidShape(value.to_owned()));
        }

        if !is_kebab_segment(folder) || !is_kebab_segment(slug) {
            return Err(CanonicalIdError::InvalidSlug(value.to_owned()));
        }

        Ok(Self(value.to_owned()))
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

        let without_extension = path.with_extension("");
        let value = without_extension.to_string_lossy().replace('\\', "/");
        Self::parse(value)
    }

    pub fn folder(&self) -> &str {
        self.0.split_once('/').expect("validated canonical id").0
    }

    pub fn slug(&self) -> &str {
        self.0.split_once('/').expect("validated canonical id").1
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn markdown_path(&self) -> PathBuf {
        PathBuf::from(self.folder()).join(format!("{}.md", self.slug()))
    }

    pub fn toml_path(&self) -> PathBuf {
        PathBuf::from(self.folder()).join(format!("{}.toml", self.slug()))
    }
}

fn is_kebab_segment(value: &str) -> bool {
    if value.starts_with('-') || value.ends_with('-') || value.contains("--") {
        return false;
    }

    value
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_canonical_id() {
        let id = CanonicalId::parse("projects/kataan-redesign").unwrap();
        assert_eq!(id.folder(), "projects");
        assert_eq!(id.slug(), "kataan-redesign");
        assert_eq!(id.as_str(), "projects/kataan-redesign");
    }

    #[test]
    fn rejects_invalid_canonical_ids() {
        for value in [
            "projects",
            "Projects/kataan-redesign",
            "projects/Kataan",
            "projects/kataan_redesign",
            "projects/kataan/redesign",
            "projects/.hidden",
            "projects/kataan.md",
            "/projects/kataan",
        ] {
            assert!(
                CanonicalId::parse(value).is_err(),
                "{value} should be invalid"
            );
        }
    }

    #[test]
    fn converts_between_id_and_paths() {
        let id = CanonicalId::parse("projects/kataan-redesign").unwrap();
        assert_eq!(
            id.markdown_path().to_string_lossy(),
            "projects/kataan-redesign.md"
        );
        assert_eq!(
            id.toml_path().to_string_lossy(),
            "projects/kataan-redesign.toml"
        );
        assert_eq!(
            CanonicalId::from_document_path("projects/kataan-redesign.md").unwrap(),
            id
        );
        assert_eq!(
            CanonicalId::from_document_path("projects/kataan-redesign.toml").unwrap(),
            id
        );
    }
}
