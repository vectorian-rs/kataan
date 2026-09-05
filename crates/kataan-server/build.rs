//! Tell cargo that the embedded web build is an input.
//!
//! `#[derive(RustEmbed)]` reads `apps/web/dist` at *compile* time in release
//! (`include_bytes!`), but cargo only tracks `.rs` files unless told otherwise.
//! So rebuilding the web app and then rebuilding the server produced a binary
//! still carrying the previous UI — cargo saw no reason to recompile. A release
//! artifact could ship a stale UI, and the CI check that boots the binary to
//! prove the UI is embedded could pass against a binary built before the web
//! app existed.
//!
//! Directory mtime alone is not enough: it moves when entries are added or
//! removed, not when a file's contents change in place, and `index.html` keeps
//! its name across builds. So every file is registered individually.

use std::path::Path;

fn main() {
    // Only the embedding build depends on `dist`; the API-only build does not.
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_EMBED_UI");
    if std::env::var_os("CARGO_FEATURE_EMBED_UI").is_none() {
        return;
    }

    let dist = Path::new("../../apps/web/dist");
    // Registered even when absent, so creating it triggers a rebuild rather
    // than leaving a binary that embeds nothing.
    println!("cargo:rerun-if-changed={}", dist.display());
    register(dist);
}

fn register(path: &Path) {
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        println!("cargo:rerun-if-changed={}", path.display());
        if path.is_dir() {
            register(&path);
        }
    }
}
