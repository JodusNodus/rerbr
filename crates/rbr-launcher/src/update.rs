//! A newer release on GitHub: checked once at the start, on its own thread, and shown as a link to its page in the
//! status bar. Nothing is downloaded; a failed check shows nothing.

use std::sync::OnceLock;

/// The published releases, newest first (drafts are not listed).
const RELEASES: &str = "https://api.github.com/repos/JodusNodus/rerbr/releases?per_page=20";
/// A release's page, before its tag.
const PAGE: &str = "https://github.com/JodusNodus/rerbr/releases/tag/";

/// The newer release's tag, once the check found one.
static NEWER: OnceLock<String> = OnceLock::new();

/// Starts the check.
pub fn start() {
    std::thread::spawn(|| {
        let body = crate::gvrc::agent().get(RELEASES).call().and_then(|mut r| r.body_mut().read_to_string());
        match body {
            Ok(body) => _ = newer(env!("CARGO_PKG_VERSION"), &body).map(|tag| NEWER.set(tag.to_owned())),
            Err(e) => eprintln!("launcher: update check: {e}"),
        }
    });
}

/// The link to the newer release's page, when there is one.
pub fn link(ui: &mut egui::Ui) {
    if let Some(tag) = NEWER.get() {
        if ui.link(format!("ReRBR {tag} is out: download")).on_hover_text(format!("{PAGE}{tag}")).clicked() {
            crate::crash::open(format!("{PAGE}{tag}").as_ref());
        }
    }
}

/// The highest `v` tag of the release pages linked in `body` (the API's JSON), when it is above `current`.
fn newer<'a>(current: &str, body: &'a str) -> Option<&'a str> {
    let tags = body.split(PAGE).skip(1).filter_map(|s| s.split('"').next()).filter(|t| t.starts_with('v'));
    let current = key(current)?;
    tags.filter_map(|t| Some((key(&t[1..])?, t))).filter(|(k, _)| *k > current).max().map(|(_, t)| t)
}

type Key<'a> = (Vec<u64>, bool, Vec<(bool, u64, &'a str)>);

/// A version's order: the numbers, then a release above its pre-releases, then the pre-release's parts (a number
/// below a word, as in semver).
fn key(version: &str) -> Option<Key<'_>> {
    let (core, pre) = version.split_once('-').map_or((version, None), |(c, p)| (c, Some(p)));
    let numbers = core.split('.').map(|n| n.parse().ok()).collect::<Option<Vec<u64>>>()?;
    let parts =
        pre.map_or(Vec::new(), |p| p.split('.').map(|s| s.parse().map_or((true, 0, s), |n| (false, n, ""))).collect());
    Some((numbers, pre.is_none(), parts))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(tags: &[&str]) -> String {
        tags.iter()
            .map(|t| format!("{{\"html_url\": \"{PAGE}{t}\", \"body\": \"notes\"}}"))
            .collect::<Vec<_>>()
            .join(",")
    }

    #[test]
    fn the_highest_newer_v_tag_is_offered() {
        let releases = body(&["test-data-v9", "v0.1.0-alpha.10", "v0.1.0-alpha.9", "v0.1.0-alpha.3"]);
        assert_eq!(newer("0.1.0-alpha.3", &releases), Some("v0.1.0-alpha.10"));
        assert_eq!(newer("0.1.0-alpha.10", &releases), None);
        assert_eq!(newer("0.1.0-alpha.11", &releases), None);
    }

    #[test]
    fn versions_order_as_in_semver() {
        let order = ["0.1.0-alpha.2", "0.1.0-alpha.10", "0.1.0-beta", "0.1.0-rc.1", "0.1.0", "0.1.1", "0.10.0"];
        for pair in order.windows(2) {
            assert!(key(pair[0]) < key(pair[1]), "{} < {}", pair[0], pair[1]);
        }
        assert_eq!(key("0.1.x"), None);
    }

    #[test]
    fn nothing_without_releases() {
        assert_eq!(newer("0.1.0", "[]"), None);
        assert_eq!(newer("0.1.0", &body(&["test-data-v3", "vnext"])), None);
    }
}
