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

    /// Fetch the latest release from the GitHub API.
    ///
    /// Tries `/releases/latest` first (only published, non-prerelease).
    /// On 404, falls back to `/releases?per_page=1` which includes pre-releases.
    pub async fn get_latest_release(&self) -> Result<Release, Box<dyn std::error::Error>> {
        // Try /releases/latest first
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

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            debug!("/releases/latest returned 404, falling back to /releases");
            return self.get_latest_release_fallback().await;
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("GitHub API error {} for {}: {}", status, url, body).into());
        }

        let release: Release = resp.json().await?;
        info!(
            "Latest release: {} ({} assets)",
            release.tag_name,
            release.assets.len()
        );
        Ok(release)
    }

    /// Fallback: fetch releases list (includes pre-releases) and return the first one.
    async fn get_latest_release_fallback(&self) -> Result<Release, Box<dyn std::error::Error>> {
        let url = format!(
            "{}/repos/{}/{}/releases?per_page=1",
            self.api_base, self.owner, self.repo
        );
        debug!("Fetching releases list from {}", url);

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
            return Err(format!("GitHub API error {} for {}: {}", status, url, body).into());
        }

        let releases: Vec<Release> = resp.json().await?;
        let release = releases
            .into_iter()
            .next()
            .ok_or("Keine Releases im Repository gefunden")?;

        info!(
            "Latest release (via fallback): {} ({} assets)",
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
            return Err(format!("Download failed {} for {}", status, url).into());
        }

        let bytes = resp.bytes().await?;
        debug!("Downloaded {} bytes from {}", bytes.len(), url);
        Ok(bytes.to_vec())
    }
}

/// Parse release assets into per-device info.
///
/// Expects naming convention from ESPHome CI:
///   - `<device-name>.manifest.json`  (manifest)
///   - `<device-name>.ota.bin`        (firmware binary)
pub fn parse_device_assets(release: &Release) -> Vec<DeviceAssets> {
    let mut manifests: std::collections::HashMap<String, &Asset> = std::collections::HashMap::new();
    let mut firmwares: std::collections::HashMap<String, &Asset> = std::collections::HashMap::new();

    for asset in &release.assets {
        if asset.name.ends_with(".manifest.json") {
            let device = asset.name.trim_end_matches(".manifest.json");
            manifests.insert(device.to_string(), asset);
        } else if asset.name.ends_with(".ota.bin") {
            let device = asset.name.trim_end_matches(".ota.bin");
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
                make_asset("aufzug-lager.manifest.json", "https://gh/manifest"),
                make_asset("aufzug-lager.ota.bin", "https://gh/firmware"),
            ],
        );
        let devices = parse_device_assets(&release);
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].device_name, "aufzug-lager");
        assert_eq!(devices[0].version, "v2025.1.0");
        assert_eq!(devices[0].manifest_url, "https://gh/manifest");
        assert_eq!(devices[0].firmware_url, "https://gh/firmware");
    }

    #[test]
    fn test_parse_multiple_devices() {
        let release = make_release(
            "v1.0",
            vec![
                make_asset("aufzug-lager.manifest.json", "https://gh/a-manifest"),
                make_asset("aufzug-lager.ota.bin", "https://gh/a-firmware"),
                make_asset("brutschrank.manifest.json", "https://gh/b-manifest"),
                make_asset("brutschrank.ota.bin", "https://gh/b-firmware"),
            ],
        );
        let devices = parse_device_assets(&release);
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].device_name, "aufzug-lager");
        assert_eq!(devices[1].device_name, "brutschrank");
    }

    #[test]
    fn test_parse_manifest_without_firmware_skipped() {
        let release = make_release(
            "v1.0",
            vec![make_asset("orphan.manifest.json", "https://gh/manifest")],
        );
        let devices = parse_device_assets(&release);
        assert!(devices.is_empty());
    }

    #[test]
    fn test_parse_firmware_without_manifest_skipped() {
        let release = make_release(
            "v1.0",
            vec![make_asset("orphan.ota.bin", "https://gh/firmware")],
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
                make_asset("my-esp.manifest.json", "https://gh/manifest"),
                make_asset("my-esp.ota.bin", "https://gh/firmware"),
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
                    "name": "test.ota.bin",
                    "browser_download_url": "https://example.com/fw",
                    "size": 512000
                }
            ]
        }"#;
        let release: Release = serde_json::from_str(json).unwrap();
        assert_eq!(release.tag_name, "v2025.3.0");
        assert_eq!(release.assets.len(), 1);
        assert_eq!(release.assets[0].name, "test.ota.bin");
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

    // --- HTTP integration tests ---

    #[tokio::test]
    async fn test_get_latest_release_calls_correct_url() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/Nebensound/ESPHome-WagnerHof/releases/latest"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "tag_name": "v1.0.0",
                "assets": []
            })))
            .expect(1)
            .mount(&mock_server)
            .await;

        let client = GitHubClient::with_base_url(
            "fake-token",
            "Nebensound",
            "ESPHome-WagnerHof",
            &mock_server.uri(),
        );
        let release = client.get_latest_release().await.unwrap();
        assert_eq!(release.tag_name, "v1.0.0");
    }

    #[tokio::test]
    async fn test_get_latest_release_fallback_on_404() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        // /releases/latest returns 404
        Mock::given(method("GET"))
            .and(path("/repos/MyOwner/MyRepo/releases/latest"))
            .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                "message": "Not Found"
            })))
            .expect(1)
            .mount(&mock_server)
            .await;

        // /releases?per_page=1 returns a pre-release
        Mock::given(method("GET"))
            .and(path("/repos/MyOwner/MyRepo/releases"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "tag_name": "v0.1.0-beta",
                    "assets": []
                }
            ])))
            .expect(1)
            .mount(&mock_server)
            .await;

        let client =
            GitHubClient::with_base_url("fake-token", "MyOwner", "MyRepo", &mock_server.uri());
        let release = client.get_latest_release().await.unwrap();
        assert_eq!(release.tag_name, "v0.1.0-beta");
    }
}
