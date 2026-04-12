use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::Duration;

fn find_free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn wait_for_server(port: u16) -> bool {
    for _ in 0..30 {
        if std::net::TcpStream::connect(format!("127.0.0.1:{port}")).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

#[test]
fn version_endpoint_returns_cargo_version() {
    let port = find_free_port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_wtop"))
        .args(["--localhost-only", "--port", &port.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to start wtop");

    assert!(wait_for_server(port), "Server did not start in time");

    let resp = ureq::get(&format!("http://127.0.0.1:{port}/version")).call();
    let body = resp.unwrap().into_body().read_to_string().unwrap();
    assert_eq!(body, env!("CARGO_PKG_VERSION"));

    child.kill().ok();
    child.wait().ok();
}

#[test]
fn static_handler_returns_html() {
    let port = find_free_port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_wtop"))
        .args(["--localhost-only", "--port", &port.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to start wtop");

    assert!(wait_for_server(port), "Server did not start in time");

    let resp = ureq::get(&format!("http://127.0.0.1:{port}/"))
        .call()
        .unwrap();
    assert_eq!(resp.headers().get("content-type").unwrap(), "text/html");
    assert_eq!(
        resp.headers().get("x-content-type-options").unwrap(),
        "nosniff"
    );
    assert_eq!(resp.headers().get("x-frame-options").unwrap(), "DENY");

    child.kill().ok();
    child.wait().ok();
}

#[test]
fn sse_endpoint_streams_json() {
    let port = find_free_port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_wtop"))
        .args(["--localhost-only", "--port", &port.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to start wtop");

    assert!(wait_for_server(port), "Server did not start in time");

    // Connect to SSE and read at least one data event
    let resp = ureq::get(&format!("http://127.0.0.1:{port}/events"))
        .header("Accept", "text/event-stream")
        .call()
        .unwrap();

    let reader = BufReader::new(resp.into_body().into_reader());
    let mut found_data = false;

    // Read lines until we find a "data:" line (timeout via metrics interval ~1.5s)
    for line in reader.lines() {
        let line = line.unwrap();
        if line.starts_with("data:") {
            let json_str = line.trim_start_matches("data:").trim();
            let parsed: serde_json::Value =
                serde_json::from_str(json_str).expect("SSE data should be valid JSON");

            // Verify required top-level fields
            assert!(parsed.get("hostname").is_some(), "Missing hostname");
            assert!(parsed.get("cpu").is_some(), "Missing cpu");
            assert!(parsed.get("mem").is_some(), "Missing mem");
            assert!(parsed.get("net").is_some(), "Missing net");
            assert!(parsed.get("disk_io").is_some(), "Missing disk_io");
            assert!(parsed.get("processes").is_some(), "Missing processes");
            assert!(parsed.get("uptime").is_some(), "Missing uptime");

            found_data = true;
            break;
        }
    }

    assert!(found_data, "No SSE data event received");

    child.kill().ok();
    child.wait().ok();
}

#[test]
fn not_found_returns_404() {
    let port = find_free_port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_wtop"))
        .args(["--localhost-only", "--port", &port.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to start wtop");

    assert!(wait_for_server(port), "Server did not start in time");

    let resp = ureq::get(&format!("http://127.0.0.1:{port}/nonexistent")).call();
    match resp {
        Err(ureq::Error::StatusCode(code)) => assert_eq!(code, 404),
        other => panic!("Expected 404 status error, got: {other:?}"),
    }

    child.kill().ok();
    child.wait().ok();
}
