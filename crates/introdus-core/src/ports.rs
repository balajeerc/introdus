//! Parsing and validation of `EXTRA_PORTS` entries — each either a single port
//! (published host:container on the same number) or `host:container` to remap.
//! Mirrors the validation in `launch_dev_container.sh`.

use anyhow::{bail, Result};

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
    fn parses_single_and_mapped() {
        let e = vec!["8123".to_owned(), "16379:6379".to_owned()];
        assert_eq!(
            parse_extra_ports(&e, 3000).unwrap(),
            vec![(8123, 8123), (16379, 6379)]
        );
    }

    #[test]
    fn rejects_bad_and_colliding() {
        assert!(parse_extra_ports(&["0".to_owned()], 3000).is_err());
        assert!(parse_extra_ports(&["70000".to_owned()], 3000).is_err());
        assert!(parse_extra_ports(&["abc".to_owned()], 3000).is_err());
        assert!(parse_extra_ports(&["3000".to_owned()], 3000).is_err());
    }
}
