//! Podman object naming: the base image, per-project image tag, container name,
//! and volume name. Mirrors the scheme in `launch_dev_container.sh` with the
//! `remote-code-` prefix replaced by `introdus-`.
//!
//! Each project gets a stable 4-char `IMAGE_SUFFIX` (normally generated once and
//! persisted in `.env`). When absent, we derive a deterministic fallback from
//! `project@hostname` so the same project on two hosts still gets distinct
//! image/container names (keeping VS Code's per-name attach config separate).

/// The shared base image, built once and reused across projects.
pub const BASE_IMAGE: &str = "introdus-base:latest";

/// Lowercase a project name into a slug safe for an image tag: keep
/// `[a-z0-9._-]`, collapse everything else to `-`.
pub fn image_slug(project_name: &str) -> String {
    let mut out = String::with_capacity(project_name.len());
    for ch in project_name.chars() {
        let c = ch.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
            out.push(c);
        } else {
            out.push('-');
        }
    }
    out
}

/// Slug a project name into a valid single-label hostname for the container's
/// `--hostname` (so paseo's server name and the shell prompt read as the project,
/// not a fixed literal). A DNS label is `[a-z0-9-]`, no leading/trailing hyphen,
/// ≤63 chars; we lowercase, map every other char to `-`, collapse runs, trim
/// hyphens, and cap the length. Empty/degenerate names fall back to `introdus`.
pub fn hostname_slug(project_name: &str) -> String {
    let mut out = String::with_capacity(project_name.len());
    for ch in project_name.chars() {
        let c = ch.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let slug = out.trim_matches('-');
    let slug: String = slug.chars().take(63).collect();
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() {
        "introdus".to_owned()
    } else {
        slug.to_owned()
    }
}

/// Per-project image tag alias of [`BASE_IMAGE`] (a `podman tag`, no rebuild).
pub fn image_name(project_name: &str, suffix: &str) -> String {
    format!("introdus-{}-{suffix}:latest", image_slug(project_name))
}

/// The container name, carrying the per-project suffix.
pub fn container_name(project_name: &str, suffix: &str) -> String {
    format!("introdus-{project_name}-{suffix}")
}

/// The persistent per-project volume backing `/home/dev`.
pub fn volume_name(project_name: &str) -> String {
    format!("introdus-vol-{project_name}")
}

/// Deterministic 4-hex-char fallback suffix from `project@hostname`, used only
/// when `.env` has no explicit `IMAGE_SUFFIX`. FNV-1a keeps it dependency-free
/// and stable across launches on the same host.
pub fn fallback_suffix(project_name: &str, hostname: &str) -> String {
    let seed = format!("{project_name}@{hostname}");
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in seed.bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{:04x}", (hash & 0xffff) as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ta18_slug_sanitizes() {
        assert_eq!(image_slug("My Project!"), "my-project-");
        assert_eq!(image_slug("web.app_2"), "web.app_2");
    }

    #[test]
    fn ta159_hostname_slug_is_a_valid_dns_label() {
        // Lowercased; the project name passes through as the container hostname.
        assert_eq!(hostname_slug("algo-builder"), "algo-builder");
        assert_eq!(hostname_slug("Algo Builder"), "algo-builder");
        // Unlike image_slug, dots and underscores are NOT valid in a single
        // hostname label — they collapse to a hyphen, and runs don't stack.
        assert_eq!(hostname_slug("web.app_2"), "web-app-2");
        assert_eq!(hostname_slug("a  b__c"), "a-b-c");
        // No leading/trailing hyphen even when the name starts/ends with junk.
        assert_eq!(hostname_slug("!My Project!"), "my-project");
        // Length is capped at 63 chars, with no trailing hyphen after the cut.
        let long = hostname_slug(&"x".repeat(80));
        assert_eq!(long.len(), 63);
        // Degenerate/empty names fall back to the old literal.
        assert_eq!(hostname_slug(""), "introdus");
        assert_eq!(hostname_slug("___"), "introdus");
    }

    #[test]
    fn ta17_names_carry_suffix() {
        assert_eq!(container_name("web", "ab12"), "introdus-web-ab12");
        assert_eq!(volume_name("web"), "introdus-vol-web");
        assert_eq!(
            image_name("Web App", "ab12"),
            "introdus-web-app-ab12:latest"
        );
    }

    #[test]
    fn ta19_fallback_suffix_is_deterministic_and_4_hex() {
        let a = fallback_suffix("web", "host1");
        let b = fallback_suffix("web", "host1");
        assert_eq!(a, b);
        assert_eq!(a.len(), 4);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(
            fallback_suffix("web", "host1"),
            fallback_suffix("web", "host2")
        );
    }
}
