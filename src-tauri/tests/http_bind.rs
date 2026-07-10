//! Regression for the Phase 1 bug where `axum::serve` was spawned on
//! Tauri's main tokio runtime and immediately starved. We bind a real
//! `TcpListener` on a dedicated thread + `new_current_thread` runtime
//! (the same shape `build_router` uses) and assert a real HTTP
//! request comes back within 2 s.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

mod common;

#[test]
fn bound_listener_serves_health_within_2s() {
    let (port, shutdown) = common::bind_real();
    let addr = format!("127.0.0.1:{}", port);
    let start = Instant::now();

    // Crude blocking HTTP/1.0 request via stdlib — no extra crate dep.
    let mut stream = TcpStream::connect(&addr).expect("tcp connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("set_read_timeout");
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .expect("set_write_timeout");
    write!(
        stream,
        "GET /api/health HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )
    .expect("write request");
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).expect("read response");
    let elapsed = start.elapsed();
    shutdown.store(true, std::sync::atomic::Ordering::Relaxed);

    assert!(
        elapsed < Duration::from_secs(2),
        "request took {:?}",
        elapsed
    );
    let head = &buf[..buf.len().min(16)];
    assert!(
        head.starts_with(b"HTTP/"),
        "got bytes {:?}",
        String::from_utf8_lossy(head)
    );
    assert!(
        std::str::from_utf8(&buf).unwrap_or("").contains("\"status\":\"ok\""),
        "response did not contain status:ok: {:?}",
        String::from_utf8_lossy(&buf)
    );
}
