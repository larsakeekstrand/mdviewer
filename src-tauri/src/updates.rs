use serde::{Deserialize, Serialize};

const REPO: &str = "larsakeekstrand/mdviewer";
const LATEST_URL: &str = "https://api.github.com/repos/larsakeekstrand/mdviewer/releases/latest";
const USER_AGENT: &str = "mdviewer-update-check";

#[derive(Deserialize)]
struct GhReleaseAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    html_url: String,
    name: Option<String>,
    body: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<GhReleaseAsset>,
}

#[derive(Serialize)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub has_update: bool,
    pub release_url: String,
    pub release_name: Option<String>,
    pub notes: Option<String>,
    pub published_at: Option<String>,
    /// First asset whose name ends with `.dmg`, if any. Useful for a direct
    /// download button.
    pub dmg_url: Option<String>,
    pub repo: String,
}

pub fn check() -> Result<UpdateInfo, String> {
    let current = env!("CARGO_PKG_VERSION").to_string();

    let mut response = ureq::get(LATEST_URL)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("HTTP error: {e}"))?;

    let body: GhRelease = response
        .body_mut()
        .read_json()
        .map_err(|e| format!("parse error: {e}"))?;

    // /releases/latest already filters draft+prerelease server-side, but be
    // defensive in case of redirects to other endpoints in the future.
    if body.draft || body.prerelease {
        return Ok(UpdateInfo {
            current_version: current.clone(),
            latest_version: current,
            has_update: false,
            release_url: body.html_url,
            release_name: body.name,
            notes: body.body,
            published_at: body.published_at,
            dmg_url: None,
            repo: REPO.to_string(),
        });
    }

    let latest = body.tag_name.trim_start_matches('v').to_string();
    let has_update = is_newer(&latest, &current);
    let dmg_url = body
        .assets
        .iter()
        .find(|a| a.name.to_lowercase().ends_with(".dmg"))
        .map(|a| a.browser_download_url.clone());

    Ok(UpdateInfo {
        current_version: current,
        latest_version: latest,
        has_update,
        release_url: body.html_url,
        release_name: body.name,
        notes: body.body,
        published_at: body.published_at,
        dmg_url,
        repo: REPO.to_string(),
    })
}

fn is_newer(latest: &str, current: &str) -> bool {
    use semver::Version;
    match (Version::parse(latest), Version::parse(current)) {
        (Ok(l), Ok(c)) => l > c,
        _ => false,
    }
}
