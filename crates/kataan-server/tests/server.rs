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
    #[cfg(feature = "embed-ui")]
    {
        let root = http_get(addr, "/").expect("root request");
        assert!(root.contains("200 OK"), "root status: {root}");
        assert!(root.contains("<html"), "root body: {root}");
    }

    #[cfg(not(feature = "embed-ui"))]
    {
        let root = http_get(addr, "/").expect("root request");
        assert!(root.contains("200 OK"), "root status: {root}");
        assert!(
            root.contains("API server is running"),
            "api-only root body: {root}"
        );
    }

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

/// Heavy requests must not stall unrelated ones.
///
/// Every handler used to do its filesystem and CPU work directly on the tokio
/// runtime, which has one worker per core. Enough concurrent
/// `/api/file/highlight` requests — each reading a large file and tokenising it
/// — occupied every worker, and the whole API stopped answering, `/api/health`
/// included. That is the opposite of what a health check is for.
#[test]
fn a_slow_request_does_not_stall_the_rest_of_the_api() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    kataan_core::init::init_vault(&vault, "Test Vault").unwrap();

    // Something genuinely expensive to highlight: large, and real syntax.
    let code_dir = vault.join("code");
    std::fs::create_dir_all(&code_dir).unwrap();
    let unit =
        "pub fn compute(value: u64) -> u64 { value.wrapping_mul(2718281828).rotate_left(7) }\n";
    std::fs::write(code_dir.join("big.rs"), unit.repeat(6_000)).unwrap();

    let addr = "127.0.0.1:38801";
    let server = ServerGuard(
        Command::new(env!("CARGO_BIN_EXE_kataan-server"))
            .args(["--vault"])
            .arg(&vault)
            .args(["--bind", addr])
            .spawn()
            .expect("spawn kataan-server"),
    );

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if http_get(addr, "/api/health").is_ok() {
            break;
        }
        assert!(Instant::now() < deadline, "server never became reachable");
        std::thread::sleep(Duration::from_millis(200));
    }

    // Confirm the file is actually reachable and expensive, so a later pass
    // cannot be "the request 404'd instantly".
    let probe = Instant::now();
    let highlighted = http_get(addr, "/api/file/highlight?path=code/big.rs").expect("highlight");
    assert!(
        highlighted.contains("200 OK"),
        "highlight status: {}",
        &highlighted[..highlighted.len().min(300)]
    );
    let single = probe.elapsed();

    // Saturate: more concurrent highlights than the runtime has workers.
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
    let load: Vec<_> = (0..workers * 2)
        .map(|_| {
            std::thread::spawn(move || {
                let _ = http_get(addr, "/api/file/highlight?path=code/big.rs");
            })
        })
        .collect();

    // While they run, health must still answer promptly.
    std::thread::sleep(Duration::from_millis(150));
    let started = Instant::now();
    let health = http_get(addr, "/api/health").expect("health during load");
    let latency = started.elapsed();

    for worker in load {
        let _ = worker.join();
    }
    drop(server);

    assert!(health.contains("200 OK"), "health under load: {health}");
    // The bound is scaled to the machine rather than absolute, and the two
    // outcomes are nowhere near it: measured on this hardware, health answers
    // in ~10ms with the work offloaded and ~3s without, against one highlight
    // taking ~1.5s. Anything approaching the cost of a highlight means health
    // queued behind it.
    let bound = (single / 4).max(Duration::from_millis(250));
    assert!(
        latency < bound,
        "health took {latency:?} under load (bound {bound:?}) while one highlight \
         takes {single:?} — it queued behind the blocking work"
    );
}
