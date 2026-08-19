//! Black-box integration tests: run the built `kataan-cli` binary and assert on
//! its stdout / stderr / exit code (the CLI's actual contract).

use std::{path::Path, process::Command};

fn kataan() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kataan-cli"))
}

fn init_vault(path: &Path) {
    let output = kataan()
        .args(["init"])
        .arg(path)
        .args(["--name", "Test Vault"])
        .output()
        .expect("run init");
    assert!(output.status.success(), "init failed: {output:?}");
}

#[test]
fn guide_prints_the_agent_guide_to_stdout() {
    let output = kataan().arg("guide").output().expect("run guide");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.starts_with("# Kataan CLI guide"),
        "unexpected guide output: {stdout:.40}"
    );
}

#[test]
fn init_creates_a_vault_and_keeps_stdout_clean() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");

    let output = kataan()
        .args(["init"])
        .arg(&vault)
        .args(["--name", "Test Vault"])
        .output()
        .expect("run init");

    assert!(output.status.success());
    assert!(vault.join("kataan.toml").is_file());
    // Confirmations are logs: they go to stderr, leaving stdout empty.
    assert!(
        output.stdout.is_empty(),
        "stdout should be empty, got: {:?}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("initialized vault"), "stderr: {stderr}");
}

#[test]
fn validate_reports_valid_on_stdout() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    init_vault(&vault);

    let output = kataan().arg("validate").arg(&vault).output().expect("run");

    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), "valid");
}

#[test]
fn validate_json_emits_a_structured_report() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    init_vault(&vault);

    // Valid vault -> {"ok":true,"diagnostics":[]} on stdout, exit 0.
    let output = kataan()
        .args(["validate"])
        .arg(&vault)
        .arg("--json")
        .output()
        .expect("run");
    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("valid JSON");
    assert_eq!(json["ok"], true);
    assert_eq!(json["diagnostics"].as_array().unwrap().len(), 0);

    // Tampered vault -> ok:false with a diagnostic, exit non-zero, still valid JSON.
    let doc = vault.join("type/note.md");
    let mut content = std::fs::read_to_string(&doc).unwrap();
    content.push_str("\ntampered\n");
    std::fs::write(&doc, content).unwrap();

    let output = kataan()
        .args(["validate"])
        .arg(&vault)
        .arg("--json")
        .output()
        .expect("run");
    assert!(!output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("valid JSON");
    assert_eq!(json["ok"], false);
    assert!(json["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["code"] == "checksum-mismatch" && d["severity"] == "error"));
}

#[test]
fn validate_reports_diagnostics_on_stdout_and_exits_nonzero() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    init_vault(&vault);

    // Tamper with a seeded document so its markdown no longer matches the stored
    // checksum -> a checksum-mismatch diagnostic.
    let doc = vault.join("type/note.md");
    let mut content = std::fs::read_to_string(&doc).unwrap();
    content.push_str("\ntampered\n");
    std::fs::write(&doc, content).unwrap();

    let output = kataan().arg("validate").arg(&vault).output().expect("run");

    assert!(!output.status.success(), "expected non-zero exit");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("checksum-mismatch"),
        "diagnostics should be on stdout, got: {stdout}"
    );
}
