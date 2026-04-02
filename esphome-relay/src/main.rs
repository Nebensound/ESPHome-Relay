mod cache;
mod config;
mod github;
mod server;
mod status;
mod webhook;

use cache::FirmwareCache;
use config::Config;
use github::{GitHubClient, parse_device_assets};
use server::{AppState, build_router};
use status::{StatusTracker, SyncStatus};

use std::sync::Arc;
use tokio::sync::Notify;
use tracing::{error, info};

#[tokio::main]
async fn main() {
    let config = Config::load().unwrap_or_else(|e| {
        eprintln!("Konfigurationsfehler: {}", e);
        std::process::exit(1);
    });

    // Init tracing
    let filter = tracing_subscriber::EnvFilter::try_new(&config.log_level)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    info!("ESPHome Firmware Relay startet...");
    info!("Repo: {}, Poll-Intervall: {}min", config.github_repo, config.poll_interval_minutes);

    // Init components
    let cache = Arc::new(FirmwareCache::new(&config.cache_dir));
    cache.ensure_dir().expect("Cache-Verzeichnis konnte nicht erstellt werden");

    let status = StatusTracker::new();
    let refresh_notify = Arc::new(Notify::new());

    let (owner, repo) = config.repo_parts();
    let github_client = Arc::new(GitHubClient::new(&config.github_token, owner, repo));

    // Determine relay base URL (HA sets INGRESS_PATH, fallback to port 8099)
    let relay_base_url = "http://localhost:8099".to_string();

    let state = AppState {
        cache: cache.clone(),
        status: status.clone(),
        webhook_secret: config.webhook_secret.clone(),
        refresh_notify: refresh_notify.clone(),
        relay_base_url: relay_base_url.clone(),
    };

    // Spawn background sync task
    let sync_cache = cache.clone();
    let sync_status = status.clone();
    let sync_github = github_client.clone();
    let sync_notify = refresh_notify.clone();
    let poll_interval = std::time::Duration::from_secs(config.poll_interval_minutes as u64 * 60);

    tokio::spawn(async move {
        // Initial sync on startup
        do_sync(&sync_github, &sync_cache, &sync_status, &relay_base_url).await;

        loop {
            tokio::select! {
                _ = tokio::time::sleep(poll_interval) => {
                    info!("Scheduled poll-sync");
                }
                _ = sync_notify.notified() => {
                    info!("Triggered sync (webhook/manual refresh)");
                }
            }
            do_sync(&sync_github, &sync_cache, &sync_status, &relay_base_url).await;
        }
    });

    // Start HTTP server
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8099")
        .await
        .expect("Port 8099 konnte nicht gebunden werden");
    info!("HTTP-Server läuft auf 0.0.0.0:8099");
    axum::serve(listener, app).await.expect("Server-Fehler");
}

async fn do_sync(
    github: &GitHubClient,
    cache: &FirmwareCache,
    status: &StatusTracker,
    relay_base_url: &str,
) {
    info!("Starte Sync...");

    let release = match github.get_latest_release().await {
        Ok(r) => r,
        Err(e) => {
            error!("GitHub API Fehler: {}", e);
            return;
        }
    };

    let devices = parse_device_assets(&release);
    let device_names: Vec<String> = devices.iter().map(|d| d.device_name.clone()).collect();

    // Update remote versions
    for d in &devices {
        status.set_remote_version(&d.device_name, &d.version);
    }

    // Download/update each device
    for d in &devices {
        let cached_ver = cache.cached_version(&d.device_name);
        let needs_update = cached_ver
            .as_ref()
            .map(|v| !crate::status::versions_match(v, &d.version))
            .unwrap_or(true);

        if !needs_update {
            info!("Device {} ist aktuell ({})", d.device_name, d.version);
            status.set_local_version(&d.device_name, &d.version);
            continue;
        }

        info!("Lade Firmware für {}: {}", d.device_name, d.version);
        status.set_sync_status(&d.device_name, SyncStatus::LadeFirmware);

        // Download manifest
        match github.download_asset(&d.manifest_url).await {
            Ok(manifest_bytes) => {
                let manifest_str = String::from_utf8_lossy(&manifest_bytes);
                if let Err(e) = cache.store_manifest(&d.device_name, &manifest_str, relay_base_url)
                {
                    error!("Manifest speichern fehlgeschlagen für {}: {}", d.device_name, e);
                    status.set_sync_status(&d.device_name, SyncStatus::Fehler);
                    continue;
                }
            }
            Err(e) => {
                error!("Manifest-Download fehlgeschlagen für {}: {}", d.device_name, e);
                status.set_sync_status(&d.device_name, SyncStatus::Fehler);
                continue;
            }
        }

        // Download firmware
        match github.download_asset(&d.firmware_url).await {
            Ok(fw_bytes) => {
                if let Err(e) = cache.store_firmware(&d.device_name, &fw_bytes) {
                    error!("Firmware speichern fehlgeschlagen für {}: {}", d.device_name, e);
                    status.set_sync_status(&d.device_name, SyncStatus::Fehler);
                    continue;
                }
            }
            Err(e) => {
                error!("Firmware-Download fehlgeschlagen für {}: {}", d.device_name, e);
                status.set_sync_status(&d.device_name, SyncStatus::Fehler);
                continue;
            }
        }

        status.set_local_version(&d.device_name, &d.version);
        info!("Device {} aktualisiert auf {}", d.device_name, d.version);
    }

    // Sync-Delete: remove local devices not in remote
    let removed = cache.sync_delete(&device_names);
    for name in &removed {
        status.remove_device(name);
    }
    status.retain_devices(&device_names);

    info!("Sync abgeschlossen: {} Geräte, {} entfernt", devices.len(), removed.len());
}
