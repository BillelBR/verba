//! "Is there a newer Verba?": one request to the releases page of the project, only when the person asks for it (the button in General) or
//! has turned on the daily check. Off by default: Verba has no other network code, and this one sends nothing but the request itself.
//!
//! The request is made by `curl.exe`, which Windows 10 and 11 bring, so that Verba itself links no network library. The answer is read for
//! one thing, the name of the latest release (`tag_name`). Nothing is downloaded or installed: when a newer version exists the person
//! is taken to its page.

use std::process::Command;

/// The page that lists the releases, from the repository address in `Cargo.toml`.
pub fn releases_page() -> String {
    format!("{}/releases", env!("CARGO_PKG_REPOSITORY").trim_end_matches('/'))
}

/// The page of one release.
pub fn release_page(tag: &str) -> String {
    format!("{}/tag/{tag}", releases_page())
}

/// The address the recent releases are asked at (the API of the host of the repository). The list is asked, not `releases/latest`: that one
/// leaves out every release marked as a pre-release, and the releases of a 0.x version are (the release workflow marks them so).
fn releases_url() -> Option<String> {
    let repo = env!("CARGO_PKG_REPOSITORY").trim_end_matches('/');
    let path = repo.strip_prefix("https://github.com/")?;
    Some(format!("https://api.github.com/repos/{path}/releases?per_page=10"))
}

/// Every value of `"tag_name"` in the answer (one per release, e.g. `v0.10.9`).
fn tags_of(json: &str) -> Vec<String> {
    let key = "\"tag_name\"";
    let mut out = Vec::new();
    let mut rest = json;
    while let Some(at) = rest.find(key) {
        rest = &rest[at + key.len()..];
        let Some(value) = rest.trim_start().strip_prefix(':').map(str::trim_start).and_then(|v| v.strip_prefix('"')) else { continue };
        if let Some(end) = value.find('"') {
            let tag = &value[..end];
            if !tag.is_empty() && tag.len() < 40 {
                out.push(tag.to_string());
            }
        }
    }
    out
}

/// The highest version among the releases in the answer.
pub fn tag_of(json: &str) -> Option<String> {
    let mut best: Option<(Vec<u64>, String)> = None;
    for tag in tags_of(json) {
        let n = numbers(&tag);
        if best.as_ref().is_none_or(|(b, _)| is_newer_numbers(b, &n)) {
            best = Some((n, tag));
        }
    }
    best.map(|(_, tag)| tag)
}

/// The numbers of a version: `v0.10.9` is `[0, 10, 9]`; what follows a number (`-beta`) is ignored.
fn numbers(version: &str) -> Vec<u64> {
    version.trim_start_matches(['v', 'V']).split('.').map(|p| p.chars().take_while(char::is_ascii_digit).collect::<String>().parse().unwrap_or(0)).collect()
}

fn is_newer_numbers(a: &[u64], b: &[u64]) -> bool {
    let len = a.len().max(b.len());
    let at = |v: &[u64], i: usize| v.get(i).copied().unwrap_or(0);
    (0..len).map(|i| (at(a, i), at(b, i))).find(|(x, y)| x != y).is_some_and(|(x, y)| y > x)
}

/// Whether `latest` is a newer version than `current`.
pub fn is_newer(current: &str, latest: &str) -> bool {
    is_newer_numbers(&numbers(current), &numbers(latest))
}

/// Asks the project's releases page: `Ok(Some(tag))` when a newer version exists, `Ok(None)` when this one is the latest, `Err(sentence)`
/// when the question could not be answered (no connection, the page is not public).
pub fn check(current: &str) -> Result<Option<String>, String> {
    let url = releases_url().ok_or("This copy of Verba does not know where its releases are.")?;
    let mut cmd = Command::new("curl.exe");
    cmd.args(["-sSL", "--max-time", "10", "-H", "Accept: application/vnd.github+json", "-H", "User-Agent: Verba-update-check", &url]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // no console window flashes
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().map_err(|_| "Windows has no curl.exe here, so Verba cannot ask.".to_string())?;
    if !out.status.success() {
        return Err("Could not reach the releases page: check the connection.".to_string());
    }
    let body = String::from_utf8_lossy(&out.stdout);
    let Some(tag) = tag_of(&body) else {
        return Err("The releases page gave no version (no release yet, or GitHub did not answer).".to_string());
    };
    Ok(is_newer(current, &tag).then_some(tag))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_compared_by_their_numbers() {
        assert!(is_newer("0.10.8", "v0.10.9"));
        assert!(is_newer("0.10.9", "v0.11.0"), "10 is less than 11, not a text comparison of 9 and 10");
        assert!(is_newer("0.9.9", "0.10.0"), "0.9 is older than 0.10");
        assert!(is_newer("0.10.8", "1.0"));
        assert!(!is_newer("0.10.8", "v0.10.8"), "the same");
        assert!(!is_newer("0.10.8", "v0.10.7"), "older");
        assert!(!is_newer("0.10.8", "v0.10.8-beta1"), "what follows the number is ignored");
        assert!(!is_newer("0.10", "0.10.0"), "a missing number is a zero");
    }

    #[test]
    fn the_highest_tag_is_read_from_the_list_of_releases() {
        assert_eq!(tag_of(r#"[{"url":"x","tag_name": "v0.10.9","name":"Verba"}]"#), Some("v0.10.9".to_string()));
        assert_eq!(tag_of(r#"{"tag_name":"v1.0"}"#), Some("v1.0".to_string()));
        // the releases come newest first, but the highest version is taken whatever the order (0.11.2 against 0.9.0 and 0.11.10)
        assert_eq!(tag_of(r#"[{"tag_name":"v0.11.2"},{"tag_name":"v0.9.0"},{"tag_name":"v0.11.10"},{"tag_name":"v0.11.1"}]"#), Some("v0.11.10".to_string()));
        // a repository that is not public answers with a message
        assert_eq!(tag_of(r#"{"message":"Not Found","documentation_url":"https://docs.github.com"}"#), None);
        assert_eq!(tag_of("[]"), None);
        assert_eq!(tag_of(""), None);
        assert_eq!(tag_of(r#"{"tag_name":""}"#), None);
    }

    #[test]
    fn the_addresses_come_from_the_repository_of_the_project() {
        assert_eq!(releases_url().as_deref(), Some("https://api.github.com/repos/BillelBR/verba/releases?per_page=10"));
        assert_eq!(release_page("v0.10.9"), "https://github.com/BillelBR/verba/releases/tag/v0.10.9");
    }
}

#[cfg(test)]
mod network_tests {
    /// Asks GitHub for real: `cargo test -p verba-app update -- --ignored --nocapture`.
    #[test]
    #[ignore = "goes to the network"]
    fn the_real_releases_page_answers() {
        // an old version is told about a newer one, the highest release is not newer than itself
        let newer = super::check("0.1.0").expect("GitHub answers");
        println!("newest release seen from 0.1.0: {newer:?}");
        let tag = newer.expect("a release newer than 0.1.0");
        assert_eq!(super::check(tag.trim_start_matches('v')).expect("GitHub answers"), None, "the latest release is not newer than itself");
    }
}
