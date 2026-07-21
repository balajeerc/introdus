//! Parsing and validation of `EXTRA_PORTS` entries — each either a single port
//! (published host:container on the same number) or `host:container` to remap.
//! Mirrors the validation in `launch_dev_container.sh`.

use anyhow::{bail, Result};
use std::net::TcpListener;

/// Find a free TCP port at or above `base`, skipping any in `avoid`, by trying to
/// bind `0.0.0.0:port` on the host — which also detects ports already published
/// by other introdus containers. The listener is dropped immediately, so the port
/// is free for the container to publish (a small TOCTOU window the caller closes
/// by persisting the pick and retrying). Errors if none is free in a 200-port
/// span. Used to assign each direct-mode paseo daemon a stable, non-colliding
/// port from [`crate::config::PASEO_PORT_BASE`].
pub fn pick_free_port(base: u16, avoid: &[u16]) -> Result<u16> {
    let end = base.saturating_add(200);
    for port in base..end {
        if avoid.contains(&port) {
            continue;
        }
        if TcpListener::bind(("0.0.0.0", port)).is_ok() {
            return Ok(port);
        }
    }
    bail!("no free port available in {base}..{end} for the paseo daemon");
}

/// Parse `EXTRA_PORTS` entries into `(host, container)` pairs, rejecting
/// malformed entries, out-of-range ports, and any host port colliding with the
/// webapp port.
pub fn parse_extra_ports(entries: &[String], webapp_port: u16) -> Result<Vec<(u16, u16)>> {
    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        let (host, container) = match entry.split_once(':') {
            Some((h, c)) => (parse_port(h, entry)?, parse_port(c, entry)?),
            None => {
                let p = parse_port(entry, entry)?;
                (p, p)
            }
        };
        if host == webapp_port {
            bail!("EXTRA_PORTS host port {host} collides with WEBAPP_PORT");
        }
        out.push((host, container));
    }
    Ok(out)
}

fn parse_port(s: &str, entry: &str) -> Result<u16> {
    match s.parse::<u32>() {
        Ok(p) if (1..=65535).contains(&p) => Ok(p as u16),
        _ => bail!("EXTRA_PORTS entry is not a valid port or host:container mapping: '{entry}'"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ta45_parses_single_and_mapped() {
        let e = vec!["8123".to_owned(), "16379:6379".to_owned()];
        assert_eq!(
            parse_extra_ports(&e, 3000).unwrap(),
            vec![(8123, 8123), (16379, 6379)]
        );
    }

    #[test]
    fn ta45_rejects_bad_and_colliding() {
        assert!(parse_extra_ports(&["0".to_owned()], 3000).is_err());
        assert!(parse_extra_ports(&["70000".to_owned()], 3000).is_err());
        assert!(parse_extra_ports(&["abc".to_owned()], 3000).is_err());
        assert!(parse_extra_ports(&["3000".to_owned()], 3000).is_err());
    }

    #[test]
    fn ta163_pick_free_port_returns_bindable_and_skips_avoid() {
        // Occupy a port, then ask starting there: the picker must skip the taken
        // one and return a different, actually-bindable port at/above the base.
        let occupied = TcpListener::bind(("0.0.0.0", 0)).unwrap();
        let taken = occupied.local_addr().unwrap().port();
        let got = pick_free_port(taken, &[]).unwrap();
        assert!(got >= taken);
        assert_ne!(got, taken, "must skip the port that is already bound");
        // The returned port is genuinely free (the picker dropped its test bind).
        assert!(TcpListener::bind(("0.0.0.0", got)).is_ok());
        // An explicit avoid entry is honored too.
        let got2 = pick_free_port(taken, &[got]).unwrap();
        assert_ne!(got2, got);
        assert_ne!(got2, taken);
    }
}
