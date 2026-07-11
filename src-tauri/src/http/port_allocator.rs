//! HTTP port allocation with retry-then-fallback.
//!
//! `bind_with_retry` tries the preferred port up to `max_attempts`
//! times with exponential-ish backoff. If every attempt fails it
//! falls back to an OS-assigned port (`127.0.0.1:0`) and signals
//! `used_fallback = true` so the caller can persist the actual port
//! and warn the user that their locked port could not be honoured.

use std::net::TcpListener;
use std::time::Duration;

/// Bind to `preferred` on loopback, retrying with linear backoff
/// before giving up and falling back to an OS-assigned port.
///
/// Returns `(listener, actual_port, used_fallback)` where
/// `used_fallback` is `true` only when the OS had to pick a random
/// port because every preferred-port attempt failed.
pub fn bind_with_retry(preferred: u16, max_attempts: u32) -> std::io::Result<(TcpListener, u16, bool)> {
    if preferred != 0 {
        for attempt in 0..max_attempts {
            match TcpListener::bind(("127.0.0.1", preferred)) {
                Ok(l) => return Ok((l, preferred, false)),
                Err(e) => {
                    let last = attempt + 1 >= max_attempts;
                    if last {
                        eprintln!(
                            "port {} bind failed after {} attempts ({}); falling back to random",
                            preferred, max_attempts, e
                        );
                        let l = TcpListener::bind(("127.0.0.1", 0))?;
                        let port = l.local_addr()?.port();
                        return Ok((l, port, true));
                    }
                    eprintln!(
                        "port {} bind failed ({}), retry {}/{}",
                        preferred,
                        e,
                        attempt + 1,
                        max_attempts
                    );
                    std::thread::sleep(Duration::from_millis(100 * (attempt as u64 + 1)));
                }
            }
        }
    }
    // preferred == 0 → straight to OS-assigned.
    let l = TcpListener::bind(("127.0.0.1", 0))?;
    let port = l.local_addr()?.port();
    Ok((l, port, true))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_with_retry_returns_preferred_when_available() {
        // Bind to a random port, drop immediately so the port is free.
        let taken = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = taken.local_addr().unwrap().port();
        drop(taken);
        let (l, actual, used_fallback) = bind_with_retry(port, 3).unwrap();
        assert_eq!(actual, port);
        assert!(!used_fallback);
        drop(l);
    }

    #[test]
    fn bind_with_retry_falls_back_when_preferred_taken() {
        // Hold the port open for the duration of the call.
        let taken = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = taken.local_addr().unwrap().port();
        let (_l, actual, used_fallback) = bind_with_retry(port, 2).unwrap();
        assert_ne!(actual, port);
        assert!(used_fallback);
    }

    #[test]
    fn bind_with_retry_skips_retry_when_preferred_is_zero() {
        // 0 is "no preference" — should not retry, just pick OS port.
        let (l, _actual, used_fallback) = bind_with_retry(0, 5).unwrap();
        assert!(used_fallback);
        drop(l);
    }
}