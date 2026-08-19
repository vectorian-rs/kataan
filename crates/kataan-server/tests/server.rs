//! Integration test that runs the actual `kataan-server` binary against a real
//! vault and drives it over HTTP, exercising process startup + binding + serving
//! (the in-process `api::tests` cover the handlers via oneshot; this covers the
//! binary itself).

use std::{
    io::{Read, Write},
    net::TcpStream,
    process::{Child, Command},
    time::{Duration, Instant},
};

/// Kills the server on drop so a failing assertion never leaks the process.
struct ServerGuard(Child);

impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Minimal HTTP/1.1 GET over a raw socket (no client dependency). Returns the
/// full response text, or an error if the connection is refused (server not up).
fn http_get(addr: &str, path: &str) -> std::io::Result<String> {
    let mut stream = TcpStream::connect(addr)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    )?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    Ok(response)
}

#[test]
fn server_binary_boots_and_serves_the_api() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    kataan_core::init::init_vault(&vault, "Test Vault").unwrap();

    let addr = "127.0.0.1:38799";
    let server = ServerGuard(
        Command::new(env!("CARGO_BIN_EXE_kataan-server"))
            .args(["--vault"])
            .arg(&vault)
            .args(["--bind", addr])
            .spawn()
            .expect("spawn kataan-server"),
    );

    // Wait for the server to bind (poll health up to ~10s).
    let deadline = Instant::now() + Duration::from_secs(10);
    let health = loop {
        if let Ok(response) = http_get(addr, "/api/health") {
            break response;
        }
        assert!(Instant::now() < deadline, "server never became reachable");
        std::thread::sleep(Duration::from_millis(200));
    };
    assert!(health.contains("200 OK"), "health status: {health}");
    assert!(health.contains("\"ok\":true"), "health body: {health}");

    // A read endpoint served from the loaded vault.
    let folders = http_get(addr, "/api/folders").expect("folders request");
    assert!(folders.contains("200 OK"), "folders status: {folders}");
    assert!(
        folders.contains("\"folder\":\"projects\""),
        "folders body: {folders}"
    );

    // An unknown /api path is a 404, not the SPA fallback.
    let missing = http_get(addr, "/api/nope").expect("missing request");
    assert!(missing.contains("404"), "unknown path status: {missing}");

    drop(server);
}
