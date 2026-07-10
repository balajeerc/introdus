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
    fn slug_sanitizes() {
        assert_eq!(image_slug("My Project!"), "my-project-");
        assert_eq!(image_slug("web.app_2"), "web.app_2");
    }

    #[test]
    fn names_carry_suffix() {
        assert_eq!(container_name("web", "ab12"), "introdus-web-ab12");
        assert_eq!(volume_name("web"), "introdus-vol-web");
        assert_eq!(
            image_name("Web App", "ab12"),
            "introdus-web-app-ab12:latest"
        );
    }

    #[test]
    fn fallback_suffix_is_deterministic_and_4_hex() {
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
