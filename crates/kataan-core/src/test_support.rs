use std::{
    fs,
    path::{Path, PathBuf},
};

pub struct TestVault {
    root: PathBuf,
}

impl TestVault {
    pub fn new(name: &str) -> Self {
        let root = unique_temp_dir(name);
        fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    pub fn path(&self) -> &Path {
        &self.root
    }

    pub fn with_root_index(self) -> Self {
        fs::write(
            self.root.join("kataan.toml"),
            r#"schema_version = "0.1.0"
name = "Test Vault"

[limits]
max_folder_depth = 4

[type_folders]
intake = "intake"
project = "projects"
person = "people"
note = "notes"
topic = "topics"
code = "code"
type-definition = "type"
"#,
        )
        .unwrap();
        self
    }

    pub fn with_ontology(self) -> Self {
        fs::write(
            self.root.join("ontology.toml"),
            include_str!("../templates/default-ontology.toml"),
        )
        .unwrap();
        self
    }

    pub fn with_folder(self, folder: &str, ty: &str, title: &str) -> Self {
        let path = self.root.join(folder);
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("index.md"), format!("# {title}\n")).unwrap();
        fs::write(
            path.join("index.toml"),
            format!(
                r#"type = "{ty}"
name = "{title}"
markdown = "index.md"
"#
            ),
        )
        .unwrap();
        self
    }

    pub fn with_doc(self, id: &str, ty: &str, markdown: &str) -> Self {
        let (folder, slug) = id.rsplit_once('/').unwrap_or(("", id));
        let folder_path = self.root.join(folder);
        fs::create_dir_all(&folder_path).unwrap();
        fs::write(folder_path.join(format!("{slug}.md")), markdown).unwrap();
        fs::write(
            folder_path.join(format!("{slug}.toml")),
            format!(
                r#"type = "{ty}"
markdown = "{slug}.md"
"#
            ),
        )
        .unwrap();
        self
    }
}

impl Drop for TestVault {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

pub fn unique_temp_dir(name: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!("kataan-{name}-{}-{counter}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use crate::vault::Vault;

    use super::*;

    #[test]
    fn builds_test_vaults() {
        let vault = TestVault::new("builder")
            .with_root_index()
            .with_ontology()
            .with_folder("projects", "project", "Projects")
            .with_doc("projects/demo", "project", "# Demo\n");

        let opened = Vault::open(vault.path()).unwrap();
        assert_eq!(opened.index.name, "Test Vault");
        assert!(vault.path().join("projects/demo.toml").exists());
    }
}
