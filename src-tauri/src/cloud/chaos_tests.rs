// Scratch chaos probes for the deploy transport (not for merge).
use super::{config, http};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::Duration;

fn cfg(url: &str) -> config::CloudConfig {
    config::CloudConfig {
        url: url.into(),
        anon_key: "anon".into(),
        site_url: String::new(),
        configured: true,
        public_key_fingerprint: None,
    }
}

fn temp_file(bytes: &[u8]) -> std::path::PathBuf {
    let file = std::env::temp_dir().join(format!("ixt-chaos-{}.ixt", uuid::Uuid::new_v4()));
    std::fs::write(&file, bytes).unwrap();
    file
}

/// Reads one request fully (head + content-length body).
fn read_request(s: &mut std::net::TcpStream) -> usize {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let n = s.read(&mut chunk).unwrap_or(0);
        if n == 0 {
            return 0;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..i]).to_lowercase();
            let len: usize = head
                .lines()
                .find_map(|l| l.strip_prefix("content-length:"))
                .map(|v| v.trim().parse().unwrap())
                .unwrap_or(0);
            while buf.len() < i + 4 + len {
                let n = s.read(&mut chunk).unwrap_or(0);
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            return len;
        }
    }
}

#[test]
fn chaos_upload_hangs_when_the_server_stalls_after_the_body() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        read_request(&mut s);
        // Black hole: never answer, never close.
        std::thread::sleep(Duration::from_secs(3600));
    });
    let file = temp_file(b"archive-bytes");
    let (tx, rx) = mpsc::channel();
    let c = cfg(&url);
    let signed = format!("{url}/storage/v1/object/upload/sign/x");
    std::thread::spawn(move || {
        let r = http::upload_file_with(&c, &signed, &file, "", 4, Duration::from_millis(1));
        let _ = tx.send(r.map_err(|e| e.code));
    });
    // Longer than the 60 s JSON API timeout and the 20 s connect timeout.
    let outcome = rx.recv_timeout(Duration::from_secs(90));
    eprintln!("CHAOS stalled upload after 90s: {outcome:?}");
    assert!(outcome.is_err(), "upload returned: {outcome:?}");
}

#[test]
fn chaos_download_hangs_when_the_body_stalls_midway() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        read_request(&mut s);
        let _ = write!(s, "HTTP/1.1 200 OK\r\ncontent-length: 1000000\r\n\r\n");
        let _ = s.write_all(&[0u8; 1000]);
        std::thread::sleep(Duration::from_secs(3600));
    });
    let (tx, rx) = mpsc::channel();
    let c = cfg(&url);
    let dir = std::env::temp_dir().join(format!("ixt-chaos-dl-{}", uuid::Uuid::new_v4()));
    std::thread::spawn(move || {
        let r = http::download(&c, &format!("{url}/storage/v1/object/sign/x"), &dir, 1 << 30, "");
        let _ = tx.send(r.map(|_| ()).map_err(|e| e.code));
    });
    let outcome = rx.recv_timeout(Duration::from_secs(90));
    eprintln!("CHAOS stalled download after 90s: {outcome:?}");
    assert!(outcome.is_err(), "download returned: {outcome:?}");
}

/// Attempt 1: the body lands, then the connection drops before any reply (lost 200).
/// Attempt 2: Storage's real duplicate reply (captured from the local stack).
#[test]
fn chaos_lost_upload_reply_turns_a_complete_upload_into_a_hard_failure() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        read_request(&mut s);
        drop(s);
        let (mut s, _) = listener.accept().unwrap();
        read_request(&mut s);
        let body = r#"{"statusCode":"409","error":"Duplicate","message":"The resource already exists","code":"KeyAlreadyExists"}"#;
        let _ = write!(
            s,
            "HTTP/1.1 400 Bad Request\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
    });
    let file = temp_file(b"archive-bytes");
    let r = http::upload_file_with(
        &cfg(&url),
        &format!("{url}/storage/v1/object/upload/sign/x"),
        &file,
        "",
        4,
        Duration::from_millis(1),
    );
    let e = r.expect_err("the retry is refused");
    let ui = crate::ui::cloud::errors::from_body(
        400,
        &serde_json::from_str(r#"{"statusCode":"409","error":"Duplicate","message":"The resource already exists","code":"KeyAlreadyExists"}"#).unwrap(),
    );
    eprintln!(
        "CHAOS lost reply: desktop code={} message={:?} transient={} | ui headline={:?}",
        e.code,
        e.message,
        http::is_transient(&e),
        crate::ui::cloud::errors::cloud_error_headline(&ui)
    );
    assert_eq!(e.code, "CLOUD_ERROR");
    assert!(!http::is_transient(&e));
}

#[test]
fn chaos_gateway_expired_jwt_code_is_not_unauthenticated() {
    let e = http::error_from(401, r#"{"code":"UNAUTHORIZED_JWT","message":"Invalid JWT","msg":"Invalid JWT"}"#);
    let ui = crate::ui::cloud::errors::from_body(401, &serde_json::json!({"code":"UNAUTHORIZED_JWT","message":"Invalid JWT"}));
    eprintln!(
        "CHAOS expired JWT: desktop code={} ui code={} headline={:?}",
        e.code,
        ui.code,
        crate::ui::cloud::errors::cloud_error_headline(&ui)
    );
}
