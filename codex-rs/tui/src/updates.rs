#![cfg(not(debug_assertions))]

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::history_cell::UpdateAvailableHistoryCell;
use crate::legacy_core::config::Config;
use crate::npm_registry;
use crate::npm_registry::NpmPackageInfo;
use crate::update_action;
use crate::update_action::UpdateAction;
use crate::update_versions::extract_version_from_latest_tag;
use crate::update_versions::is_newer;
use crate::update_versions::is_source_build_version;
use crate::updates_cache::VersionInfo;
use crate::updates_cache::read_version_info;
use crate::updates_cache::version_filepath;
use chrono::Duration;
use chrono::Utc;
use codex_http_client::ClientRouteClass;
use codex_http_client::HttpClientFactory;
use codex_http_client::RouteAwareClientPool;
use codex_login::default_client::default_headers;
use serde::Deserialize;
use std::path::Path;

use crate::version::CODEX_CLI_VERSION;

pub(crate) use crate::updates_cache::dismiss_version;

pub fn get_upgrade_version(config: &Config) -> Option<String> {
    if !config.check_for_update_on_startup || is_source_build_version(CODEX_CLI_VERSION) {
        return None;
    }

    let action = update_action::get_update_action();
    let version_file = version_filepath(config);
    let info = read_version_info(&version_file).ok();

    if !crate::version::IS_HARDENED_BUILD
        && match &info {
            None => true,
            Some(info) => info.last_checked_at < Utc::now() - Duration::hours(20),
        }
    {
        let http_client_factory = config.http_client_factory();
        // Refresh the cached latest version in the background so TUI startup
        // isn’t blocked by a network call. The UI reads the previously cached
        // value (if any) for this run; the next run shows the banner if needed.
        tokio::spawn(async move {
            check_for_update(&version_file, action, http_client_factory)
                .await
                .inspect_err(|e| tracing::error!("Failed to update version: {e}"))
        });
    }

    info.and_then(|info| {
        if is_newer(&info.latest_version, CODEX_CLI_VERSION).unwrap_or(false) {
            Some(info.latest_version)
        } else {
            None
        }
    })
}

// We use the latest version from the cask if installation is via homebrew - homebrew does not immediately pick up the latest release and can lag behind.
const HOMEBREW_CASK_API_URL: &str = "https://formulae.brew.sh/api/cask/codex.json";
const LATEST_RELEASE_URL: &str = "https://api.github.com/repos/openai/codex/releases/latest";

#[derive(Deserialize, Debug, Clone)]
struct ReleaseInfo {
    tag_name: String,
}

#[derive(Deserialize, Debug, Clone)]
struct HomebrewCaskInfo {
    version: String,
}

async fn check_for_update(
    version_file: &Path,
    action: Option<UpdateAction>,
    http_client_factory: HttpClientFactory,
) -> anyhow::Result<String> {
    let client_pool = RouteAwareClientPool::with_chatgpt_cloudflare_cookies(
        http_client_factory,
        ClientRouteClass::Other,
    )
    .with_legacy_custom_ca_fallback();
    let latest_version = match action {
        Some(UpdateAction::Daemon(_)) => return Ok(CODEX_CLI_VERSION.to_string()),
        Some(UpdateAction::BrewUpgrade) => {
            let HomebrewCaskInfo { version } = client_pool
                .get(HOMEBREW_CASK_API_URL)
                .headers(default_headers())
                .send()
                .await?
                .error_for_status()?
                .json::<HomebrewCaskInfo>()
                .await?;
            version
        }
        Some(UpdateAction::NpmGlobalLatest)
        | Some(UpdateAction::BunGlobalLatest)
        | Some(UpdateAction::VitePlusGlobalLatest)
        | Some(UpdateAction::PnpmGlobalLatest) => {
            let latest_version = fetch_latest_github_release_version(&client_pool).await?;
            let package_info = client_pool
                .get(npm_registry::PACKAGE_URL)
                .headers(default_headers())
                .send()
                .await?
                .error_for_status()?
                .json::<NpmPackageInfo>()
                .await?;
            npm_registry::ensure_version_ready(&package_info, &latest_version)?;
            latest_version
        }
        Some(UpdateAction::StandaloneUnix) | Some(UpdateAction::StandaloneWindows) | None => {
            fetch_latest_github_release_version(&client_pool).await?
        }
    };

    // Preserve any previously dismissed version if present.
    let prev_info = read_version_info(version_file).ok();
    let info = VersionInfo {
        latest_version: latest_version.clone(),
        last_checked_at: Utc::now(),
        dismissed_version: prev_info.and_then(|p| p.dismissed_version),
    };

    let json_line = format!("{}\n", serde_json::to_string(&info)?);
    if let Some(parent) = version_file.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(version_file, json_line).await?;
    Ok(latest_version)
}

pub(crate) fn next_update_refresh_delay(config: &Config) -> Option<std::time::Duration> {
    if !config.check_for_update_on_startup || is_source_build_version(CODEX_CLI_VERSION) {
        return None;
    }
    let version_file = version_filepath(config);
    let Ok(info) = read_version_info(&version_file) else {
        return Some(std::time::Duration::ZERO);
    };
    let next_check = info.last_checked_at + Duration::hours(20);
    Some(
        (next_check - Utc::now())
            .to_std()
            .unwrap_or(std::time::Duration::ZERO),
    )
}

pub(crate) fn spawn_hardened_update_watcher(
    config: &Config,
    update_tx: AppEventSender,
    mut last_notified_version: Option<String>,
) {
    if !crate::version::IS_HARDENED_BUILD {
        return;
    }
    let Some(initial_delay) = next_update_refresh_delay(config) else {
        return;
    };
    let update_config = config.clone();
    tokio::spawn(async move {
        let mut delay = initial_delay;
        loop {
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
            match refresh_upgrade_version(&update_config).await {
                Ok(Some(latest_version)) => {
                    if last_notified_version.as_deref() != Some(latest_version.as_str()) {
                        update_tx.send(AppEvent::InsertHistoryCell(Box::new(
                            UpdateAvailableHistoryCell::new(
                                latest_version.clone(),
                                /*update_action*/ None,
                            ),
                        )));
                        last_notified_version = Some(latest_version);
                    }
                    delay = std::time::Duration::from_secs(20 * 60 * 60);
                }
                Ok(None) => {
                    last_notified_version = None;
                    delay = std::time::Duration::from_secs(20 * 60 * 60);
                }
                Err(err) => {
                    tracing::warn!(%err, "hardened Codex update check failed");
                    delay = std::time::Duration::from_secs(60 * 60);
                }
            }
        }
    });
}

pub(crate) async fn refresh_upgrade_version(config: &Config) -> anyhow::Result<Option<String>> {
    let version_file = version_filepath(config);
    let latest = check_for_update(
        &version_file,
        update_action::get_update_action(),
        config.http_client_factory(),
    )
    .await?;
    Ok(is_newer(&latest, CODEX_CLI_VERSION)
        .unwrap_or(false)
        .then_some(latest))
}

async fn fetch_latest_github_release_version(
    client_pool: &RouteAwareClientPool,
) -> anyhow::Result<String> {
    let ReleaseInfo {
        tag_name: latest_tag_name,
    } = client_pool
        .get(LATEST_RELEASE_URL)
        .headers(default_headers())
        .send()
        .await?
        .error_for_status()?
        .json::<ReleaseInfo>()
        .await?;
    extract_version_from_latest_tag(&latest_tag_name)
}

/// Returns the latest version to show in a popup, if it should be shown.
/// This respects the user's dismissal choice for the current latest version.
pub fn get_upgrade_version_for_popup(config: &Config) -> Option<String> {
    if !config.check_for_update_on_startup || is_source_build_version(CODEX_CLI_VERSION) {
        return None;
    }

    let version_file = version_filepath(config);
    let latest = get_upgrade_version(config)?;
    // If the user dismissed this exact version previously, do not show the popup.
    if let Ok(info) = read_version_info(&version_file)
        && info.dismissed_version.as_deref() == Some(latest.as_str())
    {
        return None;
    }
    Some(latest)
}
