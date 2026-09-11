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

#[test]
fn help_lists_every_value_the_direction_flag_accepts() {
    // argh has no equivalent of clap's generated `[possible values: ...]`, so
    // the accepted values are written into the doc comment by hand. That is a
    // list in two places, which is the shape that drifts — this holds them
    // together.
    let help = String::from_utf8(
        kataan()
            .args(["graph", "neighbors", "--help"])
            .output()
            .expect("run help")
            .stdout,
    )
    .unwrap();

    for value in ["out", "in", "both"] {
        assert!(
            help.contains(value),
            "`--direction` accepts `{value}`, but help never mentions it:\n{help}"
        );
    }
}

#[test]
fn a_rejected_flag_value_names_the_ones_that_work() {
    let output = kataan()
        .args([
            "graph",
            "neighbors",
            ".",
            "topics/rust",
            "--direction",
            "nonsense",
        ])
        .output()
        .expect("run neighbors");
    assert!(!output.status.success());

    let stderr = String::from_utf8(output.stderr).unwrap();
    for value in ["out", "in", "both"] {
        assert!(
            stderr.contains(value),
            "rejection did not name `{value}`: {stderr}"
        );
    }
}

#[test]
fn a_list_flag_takes_commas_or_repetition() {
    // argh has no delimiter option, so `--type a,b` is split in the CLI — the
    // same way `wire::Csv` splits it out of a URL query string. Both spellings
    // must reach core as the same filter.
    let vault = std::env::temp_dir().join(format!("kataan-cli-lists-{}", std::process::id()));
    init_vault(&vault);

    let run = |args: &[&str]| {
        let output = kataan()
            .args(["graph", "export"])
            .arg(&vault)
            .args(args)
            .output()
            .expect("run graph export");
        assert!(output.status.success(), "{output:?}");
        String::from_utf8(output.stdout).unwrap()
    };

    assert_eq!(
        run(&["--type", "note,person"]),
        run(&["--type", "note", "--type", "person"])
    );

    std::fs::remove_dir_all(&vault).unwrap();
}

#[test]
fn exit_codes_distinguish_a_bad_invocation_from_an_invalid_vault() {
    // 1 means "ran fine, found problems"; 2 means "you gave me the wrong
    // arguments". A CI script reads the code, so collapsing them is a real
    // break — and it happened: `argh::from_env` exits with 1 of its own accord,
    // which the migration to argh silently adopted.
    let vault = std::env::temp_dir().join(format!("kataan-cli-exit-{}", std::process::id()));
    init_vault(&vault);

    let code = |args: &[&str]| {
        kataan()
            .args(args)
            .output()
            .expect("run kataan-cli")
            .status
            .code()
            .expect("exited normally")
    };

    assert_eq!(code(&["validate"]), 2, "missing argument");
    assert_eq!(code(&["definitely-not-a-command"]), 2, "unknown subcommand");
    assert_eq!(code(&["--help"]), 0, "help is a successful request");
    assert_eq!(
        code(&["validate", vault.to_str().unwrap()]),
        0,
        "a valid vault"
    );

    // Break the vault the way the checksum test does, and the code becomes 1 —
    // not 2.
    let document = vault.join("type/note.md");
    let mut content = std::fs::read_to_string(&document).unwrap();
    content.push_str("\ntampered\n");
    std::fs::write(&document, content).unwrap();
    assert_eq!(
        code(&["validate", vault.to_str().unwrap()]),
        1,
        "an invalid vault"
    );

    std::fs::remove_dir_all(&vault).unwrap();
}
