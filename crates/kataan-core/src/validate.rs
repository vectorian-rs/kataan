use std::path::Path;

use crate::{
    diagnostic::{Diagnostic, DiagnosticReport},
    vault::Vault,
    Result,
};

pub fn validate(root: impl AsRef<Path>) -> Result<DiagnosticReport> {
    let vault = Vault::open(root)?;
    let mut issues = Vec::new();

    for required_type in [
        "raw",
        "project",
        "person",
        "note",
        "topic",
        "type-definition",
    ] {
        if !vault.index.type_folders.contains_key(required_type) {
            issues.push(
                Diagnostic::error(
                    "missing-type-folder",
                    format!("missing type_folders entry for `{required_type}`"),
                )
                .with_path("index.toml"),
            );
        }
    }

    Ok(DiagnosticReport::new(issues))
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::*;

    #[test]
    fn reports_missing_required_type_folder_entries() {
        let root = unique_temp_dir();
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("index.toml"),
            r#"
schema_version = "0.1.0"
name = "Test Vault"

[type_folders]
raw = "raw"
"#,
        )
        .unwrap();

        let report = validate(&root).unwrap();

        assert!(!report.is_ok());
        assert!(report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "missing-type-folder"
                && diagnostic.message.contains("project")));

        fs::remove_dir_all(root).unwrap();
    }

    fn unique_temp_dir() -> PathBuf {
        std::env::temp_dir().join(format!(
            "kataan-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }
}
