//! Update check: at start-up, ask GitHub for the latest MattCut release. When it is newer than
//! this build, the UI shows "MattCut X is available" and keeps an Update button in the top bar.
//!
//! A release whose notes contain a line `[required]` is a required update: the dialog has no
//! "Later" button. Drafts and pre-releases are ignored. No network: nothing happens.

use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

/// The latest published release of this repo.
pub const RELEASES_API: &str = "https://api.github.com/repos/aatirraza/mattcut/releases/latest";
/// Where "Update now" sends people (it always offers the latest version).
pub const DOWNLOAD_PAGE: &str = "https://mattcut-site.vercel.app/";
/// A line in the release notes that makes the update required.
const REQUIRED_MARK: &str = "[required]";
/// This build's version.
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone)]
pub struct Available {
    pub version: String,
    pub notes: String,
    pub required: bool,
}

/// Start the check on a background thread; the receiver gets a value only if an update exists.
pub fn check_in_background(ctx: egui::Context) -> Receiver<Available> {
    let (tx, rx) = channel();
    let spawned = std::thread::Builder::new().name("update-check".into()).spawn(move || match latest() {
        Ok(Some(a)) => {
            let _ = tx.send(a);
            ctx.request_repaint();
        }
        Ok(None) => {}
        Err(e) => log::info!("update check skipped: {e}"),
    });
    if let Err(e) = spawned {
        log::warn!("update check thread: {e}");
    }
    rx
}

fn latest() -> Result<Option<Available>, String> {
    let agent = mattcut_office::http_agent(Duration::from_secs(15));
    let mut resp = agent
        .get(RELEASES_API)
        .header("User-Agent", concat!("MattCut/", env!("CARGO_PKG_VERSION")))
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    if status != 200 {
        return Err(format!("HTTP {status}"));
    }
    let text = resp.body_mut().with_config().limit(1 << 20).read_to_string().map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    if v["draft"].as_bool() == Some(true) || v["prerelease"].as_bool() == Some(true) {
        return Ok(None);
    }
    let version = v["tag_name"].as_str().unwrap_or_default().trim_start_matches('v').to_string();
    if !is_newer(&version, CURRENT) {
        return Ok(None);
    }
    let (notes, required) = clean_notes(v["body"].as_str().unwrap_or_default());
    Ok(Some(Available { version, notes, required }))
}

/// Release notes as plain text (no `[required]` line, no Markdown emphasis), and whether the
/// update is required.
fn clean_notes(body: &str) -> (String, bool) {
    let mut required = false;
    let mut lines = Vec::new();
    for line in body.lines() {
        if line.trim().eq_ignore_ascii_case(REQUIRED_MARK) {
            required = true;
            continue;
        }
        lines.push(line.replace("**", "").replace('`', "").trim_start_matches("## ").trim_start_matches("# ").to_string());
    }
    let mut notes = lines.join("\n").trim().to_string();
    if notes.chars().count() > 1500 {
        notes = notes.chars().take(1500).collect::<String>() + "…";
    }
    (notes, required)
}

/// `candidate` is a newer X.Y.Z than `current` (anything after `-` or `+` is ignored).
pub fn is_newer(candidate: &str, current: &str) -> bool {
    matches!((parse(candidate), parse(current)), (Some(a), Some(b)) if a > b)
}

fn parse(v: &str) -> Option<[u64; 3]> {
    let core = v.split(['-', '+']).next()?;
    let mut it = core.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next().unwrap_or("0").parse().ok()?;
    let patch = it.next().unwrap_or("0").parse().ok()?;
    Some([major, minor, patch])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(is_newer("0.2.0", "0.1.0"));
        assert!(is_newer("0.1.1", "0.1.0"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(is_newer("0.10.0", "0.9.0"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.0.9", "0.1.0"));
        assert!(!is_newer("", "0.1.0"));
        assert!(!is_newer("latest", "0.1.0"));
    }

    #[test]
    fn notes() {
        let (n, r) = clean_notes("**New:** captions\n[Required]\nUse `Q` to trim");
        assert!(r);
        assert_eq!(n, "New: captions\nUse Q to trim");
        assert!(!clean_notes("nothing required here").1);
    }
}
