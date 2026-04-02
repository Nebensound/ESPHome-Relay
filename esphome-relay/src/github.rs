use serde::Deserialize;
use tracing::{debug, info, warn};

/// A GitHub release
#[derive(Debug, Clone, Deserialize)]
pub struct Release {
    pub tag_name: String,
    pub assets: Vec<Asset>,
}

/// A release asset (firmware binary or manifest)
#[derive(Debug, Clone, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
}

/// Parsed device info extracted from release assets
#[derive(Debug, Clone)]
pub struct DeviceAssets {
    pub device_name: String,
    pub version: String,
    pub manifest_url: String,
    pub firmware_url: String,
}

/// GitHub API client for fetching releases and downloading assets.
pub struct GitHubClient {
    token: String,
    owner: String,
    repo: String,
    client: reqwest::Client,
    api_base: String,
}

impl GitHubClient {
    pub fn new(token: &str, owner: &str, repo: &str) -> Self {
        Self::with_base_url(token, owner, repo, "https://api.github.com")
    }

    /// Create client with custom API base URL (for testing)
    pub fn with_base_url(token: &str, owner: &str, repo: &str, api_base: &str) -> Self {
        let client = reqwest::Client::builder()
            .user_agent("esphome-relay")
            .build()
            .expect("failed to build HTTP client");
        Self {
            token: token.to_string(),
            owner: owner.to_string(),
            repo: repo.to_string(),
            client,
            api_base: api_base.trim_end_matches('/').to_string(),
        }
    }

    /// Fetch the latest release from the GitHub API
    pub async fn get_latest_release(&self) -> Result<Release, Box<dyn std::error::Error>> {
        let url = format!(
            "{}/repos/{}/{}/releases/latest",
            self.api_base, self.owner, self.repo
        );
        debug!("Fetching latest release from {}", url);

        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("GitHub API error {}: {}", status, body).into());
        }

        let release: Release = resp.json().await?;
        info!(
            "Latest release: {} ({} assets)",
            release.tag_name,
            release.assets.len()
        );
        Ok(release)
    }

    /// Download an asset by URL, returning the bytes
    pub async fn download_asset(&self, url: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        debug!("Downloading asset: {}", url);
        let resp = self
            .client
            .get(url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/octet-stream")
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(format!("Download failed {}: {}", status, url).into());
        }

        let bytes = resp.bytes().await?;
        debug!("Downloaded {} bytes from {}", bytes.len(), url);
        Ok(bytes.to_vec())
    }
}

/// Parse release assets into per-device info.
///
/// Expects naming convention from ESPHome CI:
///   - `<device-name>/manifest.json`
///   - `<device-name>/firmware.ota.bin`
///
/// OR flat naming:
///   - `<device-name>-manifest.json`
///   - `<device-name>-firmware.ota.bin`
pub fn parse_device_assets(release: &Release) -> Vec<DeviceAssets> {
    let mut manifests: std::collections::HashMap<String, &Asset> = std::collections::HashMap::new();
    let mut firmwares: std::collections::HashMap<String, &Asset> = std::collections::HashMap::new();

    for asset in &release.assets {
        // Try flat naming: device-name-manifest.json / device-name-firmware.ota.bin
        if asset.name.ends_with("-manifest.json") {
            let device = asset.name.trim_end_matches("-manifest.json");
            manifests.insert(device.to_string(), asset);
        } else if asset.name.ends_with("-firmware.ota.bin") {
            let device = asset.name.trim_end_matches("-firmware.ota.bin");
            firmwares.insert(device.to_string(), asset);
        }
    }

    let mut result = Vec::new();
    for (device_name, manifest_asset) in &manifests {
        if let Some(firmware_asset) = firmwares.get(device_name) {
            result.push(DeviceAssets {
                device_name: device_name.clone(),
                version: release.tag_name.clone(),
                manifest_url: manifest_asset.browser_download_url.clone(),
                firmware_url: firmware_asset.browser_download_url.clone(),
            });
        } else {
            warn!(
                "Device {} has manifest but no firmware asset, skipping",
                device_name
            );
        }
    }

    result.sort_by(|a, b| a.device_name.cmp(&b.device_name));
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_asset(name: &str, url: &str) -> Asset {
        Asset {
            name: name.to_string(),
            browser_download_url: url.to_string(),
            size: 100,
        }
    }

    fn make_release(tag: &str, assets: Vec<Asset>) -> Release {
        Release {
            tag_name: tag.to_string(),
            assets,
        }
    }

    // --- parse_device_assets tests ---

    #[test]
    fn test_parse_single_device() {
        let release = make_release(
            "v2025.1.0",
            vec![
                make_asset("living-room-manifest.json", "https://gh/manifest"),
                make_asset("living-room-firmware.ota.bin", "https://gh/firmware"),
            ],
        );
        let devices = parse_device_assets(&release);
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].device_name, "living-room");
        assert_eq!(devices[0].version, "v2025.1.0");
        assert_eq!(devices[0].manifest_url, "https://gh/manifest");
        assert_eq!(devices[0].firmware_url, "https://gh/firmware");
    }

    #[test]
    fn test_parse_multiple_devices() {
        let release = make_release(
            "v1.0",
            vec![
                make_asset("device-a-manifest.json", "https://gh/a-manifest"),
                make_asset("device-a-firmware.ota.bin", "https://gh/a-firmware"),
                make_asset("device-b-manifest.json", "https://gh/b-manifest"),
                make_asset("device-b-firmware.ota.bin", "https://gh/b-firmware"),
            ],
        );
        let devices = parse_device_assets(&release);
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].device_name, "device-a");
        assert_eq!(devices[1].device_name, "device-b");
    }

    #[test]
    fn test_parse_manifest_without_firmware_skipped() {
        let release = make_release(
            "v1.0",
            vec![make_asset("orphan-manifest.json", "https://gh/manifest")],
        );
        let devices = parse_device_assets(&release);
        assert!(devices.is_empty());
    }

    #[test]
    fn test_parse_firmware_without_manifest_skipped() {
        let release = make_release(
            "v1.0",
            vec![make_asset("orphan-firmware.ota.bin", "https://gh/firmware")],
        );
        let devices = parse_device_assets(&release);
        assert!(devices.is_empty());
    }

    #[test]
    fn test_parse_empty_release() {
        let release = make_release("v1.0", vec![]);
        let devices = parse_device_assets(&release);
        assert!(devices.is_empty());
    }

    #[test]
    fn test_parse_unrelated_assets_ignored() {
        let release = make_release(
            "v1.0",
            vec![
                make_asset("README.md", "https://gh/readme"),
                make_asset("checksums.txt", "https://gh/checksums"),
                make_asset("my-esp-manifest.json", "https://gh/manifest"),
                make_asset("my-esp-firmware.ota.bin", "https://gh/firmware"),
            ],
        );
        let devices = parse_device_assets(&release);
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].device_name, "my-esp");
    }

    // --- Deserialization tests ---

    #[test]
    fn test_release_deserialize() {
        let json = r#"{
            "tag_name": "v2025.3.0",
            "assets": [
                {
                    "name": "test-firmware.ota.bin",
                    "browser_download_url": "https://example.com/fw",
                    "size": 512000
                }
            ]
        }"#;
        let release: Release = serde_json::from_str(json).unwrap();
        assert_eq!(release.tag_name, "v2025.3.0");
        assert_eq!(release.assets.len(), 1);
        assert_eq!(release.assets[0].name, "test-firmware.ota.bin");
        assert_eq!(release.assets[0].size, 512000);
    }

    #[test]
    fn test_release_deserialize_extra_fields_ignored() {
        let json = r#"{
            "tag_name": "v1.0",
            "id": 12345,
            "draft": false,
            "assets": []
        }"#;
        let release: Release = serde_json::from_str(json).unwrap();
        assert_eq!(release.tag_name, "v1.0");
    }

    #[test]
    fn test_asset_size_defaults_to_zero() {
        let json = r#"{
            "name": "test.bin",
            "browser_download_url": "https://example.com/test"
        }"#;
        let asset: Asset = serde_json::from_str(json).unwrap();
        assert_eq!(asset.size, 0);
    }
}
