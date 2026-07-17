//! Whimsical tmux session names (`introdus-fast-roving-car`).
//!
//! Each container runs inside one tmux session. On first launch we mint a
//! readable `introdus-<adjective>-<adjective>-<noun>` name and persist it in
//! `.env` as `SESSION_NAME`. The name is derived deterministically from the
//! project name (via FNV-1a), so it's stable and reproducible even before it's
//! persisted, while still differing between projects.

const ADJECTIVES: &[&str] = &[
    "fast", "roving", "quiet", "amber", "brave", "clever", "dapper", "eager", "fabled", "gentle",
    "hidden", "iron", "jolly", "keen", "lucid", "mellow", "nimble", "opal", "plucky", "quirky",
    "rustic", "silver", "tidal", "umber", "vivid", "witty", "zesty",
];

const NOUNS: &[&str] = &[
    "car", "fox", "lynx", "otter", "raven", "heron", "comet", "delta", "ember", "fjord", "grove",
    "harbor", "island", "jetty", "kiln", "lagoon", "meadow", "nebula", "oasis", "prairie",
    "quartz", "reef", "summit", "tundra",
];

/// A stable session name for `project_name`:
/// `introdus-<adjective>-<adjective>-<noun>`.
pub fn generate(project_name: &str) -> String {
    let seed = fnv1a(project_name);
    let a1 = ADJECTIVES[(seed % ADJECTIVES.len() as u64) as usize];
    let a2 = ADJECTIVES[((seed / 7) % ADJECTIVES.len() as u64) as usize];
    let n = NOUNS[((seed / 31) % NOUNS.len() as u64) as usize];
    // Avoid the odd "fast-fast-x" repeat by nudging the second adjective.
    let a2 = if a2 == a1 {
        ADJECTIVES[((seed / 7 + 1) % ADJECTIVES.len() as u64) as usize]
    } else {
        a2
    };
    format!("introdus-{a1}-{a2}-{n}")
}

fn fnv1a(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ta70_deterministic_and_shaped() {
        assert_eq!(generate("web"), generate("web"));
        let name = generate("web");
        assert!(name.starts_with("introdus-"));
        assert_eq!(name.split('-').count(), 4);
    }

    #[test]
    fn ta71_adjectives_differ() {
        for p in ["web", "api", "worker", "site", "db", "x"] {
            let name = generate(p);
            let parts: Vec<&str> = name.split('-').collect();
            assert_ne!(
                parts[1], parts[2],
                "adjectives should differ for {p}: {name}"
            );
        }
    }

    #[test]
    fn ta71_differs_between_projects() {
        assert_ne!(generate("web"), generate("api"));
    }
}
