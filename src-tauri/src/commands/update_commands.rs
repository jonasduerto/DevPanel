use serde::{Deserialize, Serialize};

use crate::state::AppState;

const RELEASES_URL: &str = "https://api.github.com/repos/jonasduerto/DevPanel/releases/latest";

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    name: Option<String>,
}

#[derive(Serialize)]
pub struct UpdateCheck {
    pub enabled: bool,
    pub current_version: String,
    pub latest_version: Option<String>,
    pub release_name: Option<String>,
    pub download_url: Option<String>,
    pub available: bool,
}

fn version_parts(version: &str) -> Vec<u64> {
    version
        .trim_start_matches('v')
        .split(|character: char| !character.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse().ok())
        .collect()
}

fn is_newer(latest: &str, current: &str) -> bool {
    let latest = version_parts(latest);
    let current = version_parts(current);
    let length = latest.len().max(current.len());
    (0..length)
        .find_map(|index| {
            let left = *latest.get(index).unwrap_or(&0);
            let right = *current.get(index).unwrap_or(&0);
            (left != right).then_some(left > right)
        })
        .unwrap_or(false)
}

#[tauri::command]
pub async fn check_for_update(state: tauri::State<'_, AppState>) -> Result<UpdateCheck, String> {
    let enabled = {
        let config = state.config.lock().await;
        config.get().update_checks_enabled
    };
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    if !enabled {
        return Ok(UpdateCheck {
            enabled,
            current_version,
            latest_version: None,
            release_name: None,
            download_url: None,
            available: false,
        });
    }

    let release = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(4))
        .user_agent("DevPanel update checker")
        .build()
        .map_err(|error| error.to_string())?
        .get(RELEASES_URL)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|error| format!("Could not check GitHub releases: {error}"))?
        .error_for_status()
        .map_err(|error| format!("GitHub release check failed: {error}"))?
        .json::<GitHubRelease>()
        .await
        .map_err(|error| format!("Could not read GitHub release data: {error}"))?;

    let available = is_newer(&release.tag_name, &current_version);
    Ok(UpdateCheck {
        enabled,
        current_version,
        latest_version: available.then_some(release.tag_name),
        release_name: available.then_some(release.name.unwrap_or_else(|| "DevPanel update".into())),
        download_url: available.then_some(release.html_url),
        available,
    })
}
