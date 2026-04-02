use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{debug, info, warn};

/// Metadata for a cached device firmware
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedDevice {
    pub name: String,
    pub version: String,
}

/// Manages the local firmware cache directory.
///
/// Structure:
/// ```text
/// cache_dir/
///   device-a/
///     manifest.json
///     firmware.ota.bin
///   device-b/
///     manifest.json
///     firmware.ota.bin
/// ```
pub struct FirmwareCache {
    cache_dir: PathBuf,
}

impl FirmwareCache {
    pub fn new(cache_dir: &str) -> Self {
        Self {
            cache_dir: PathBuf::from(cache_dir),
        }
    }

    /// Ensure the cache root directory exists
    pub fn ensure_dir(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.cache_dir)
    }

    /// Device-specific directory
    fn device_dir(&self, device_name: &str) -> PathBuf {
        self.cache_dir.join(device_name)
    }

    /// Path to cached manifest
    pub fn manifest_path(&self, device_name: &str) -> PathBuf {
        self.device_dir(device_name).join("manifest.json")
    }

    /// Path to cached firmware binary
    pub fn firmware_path(&self, device_name: &str) -> PathBuf {
        self.device_dir(device_name).join("firmware.ota.bin")
    }

    /// Store a manifest for a device (with URL rewrite)
    pub fn store_manifest(
        &self,
        device_name: &str,
        manifest_json: &str,
        relay_base_url: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = self.device_dir(device_name);
        std::fs::create_dir_all(&dir)?;

        let rewritten = rewrite_manifest_urls(manifest_json, device_name, relay_base_url)?;
        std::fs::write(self.manifest_path(device_name), rewritten)?;
        debug!("Stored manifest for {}", device_name);
        Ok(())
    }

    /// Store firmware binary for a device
    pub fn store_firmware(
        &self,
        device_name: &str,
        data: &[u8],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = self.device_dir(device_name);
        std::fs::create_dir_all(&dir)?;
        std::fs::write(self.firmware_path(device_name), data)?;
        debug!("Stored firmware for {} ({} bytes)", device_name, data.len());
        Ok(())
    }

    /// Read cached manifest
    pub fn read_manifest(&self, device_name: &str) -> Option<String> {
        std::fs::read_to_string(self.manifest_path(device_name)).ok()
    }

    /// Read cached firmware binary
    pub fn read_firmware(&self, device_name: &str) -> Option<Vec<u8>> {
        std::fs::read(self.firmware_path(device_name)).ok()
    }

    /// List all cached device names
    pub fn list_devices(&self) -> Vec<String> {
        let mut devices = Vec::new();
        let entries = match std::fs::read_dir(&self.cache_dir) {
            Ok(e) => e,
            Err(_) => return devices,
        };
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                if let Some(name) = entry.file_name().to_str() {
                    devices.push(name.to_string());
                }
            }
        }
        devices.sort();
        devices
    }

    /// Remove devices from cache that are no longer in the remote set.
    /// Returns list of removed device names.
    pub fn sync_delete(&self, remote_devices: &[String]) -> Vec<String> {
        let local = self.list_devices();
        let mut removed = Vec::new();
        for name in &local {
            if !remote_devices.contains(name) {
                let dir = self.device_dir(name);
                if std::fs::remove_dir_all(&dir).is_ok() {
                    info!("Removed stale device from cache: {}", name);
                    removed.push(name.clone());
                } else {
                    warn!("Failed to remove stale device dir: {}", name);
                }
            }
        }
        removed
    }

    /// Extract version from a cached manifest (reads "version" field from builds[0])
    pub fn cached_version(&self, device_name: &str) -> Option<String> {
        let manifest = self.read_manifest(device_name)?;
        extract_version_from_manifest(&manifest)
    }
}

/// Extract version string from an ESPHome manifest JSON
pub fn extract_version_from_manifest(manifest_json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(manifest_json).ok()?;
    v.get("version")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Rewrite `path` fields in manifest builds to point to the local relay
fn rewrite_manifest_urls(
    manifest_json: &str,
    device_name: &str,
    relay_base_url: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut manifest: serde_json::Value = serde_json::from_str(manifest_json)?;

    if let Some(builds) = manifest.get_mut("builds").and_then(|b| b.as_array_mut()) {
        for build in builds {
            if let Some(path) = build.get_mut("path") {
                let new_url = format!(
                    "{}/devices/{}/firmware.ota.bin",
                    relay_base_url.trim_end_matches('/'),
                    device_name
                );
                *path = serde_json::Value::String(new_url);
            }
        }
    }

    Ok(serde_json::to_string_pretty(&manifest)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> (TempDir, FirmwareCache) {
        let tmp = TempDir::new().unwrap();
        let cache = FirmwareCache::new(tmp.path().to_str().unwrap());
        (tmp, cache)
    }

    // --- Test: store and read manifest ---
    #[test]
    fn test_store_and_read_manifest() {
        let (_tmp, cache) = setup();
        let manifest = r#"{"version":"2025.1.0","builds":[{"path":"firmware.ota.bin"}]}"#;
        cache
            .store_manifest("test-device", manifest, "http://localhost:8099")
            .unwrap();

        let read = cache.read_manifest("test-device").unwrap();
        assert!(read.contains("2025.1.0"));
        // URL should be rewritten
        assert!(read.contains("http://localhost:8099/devices/test-device/firmware.ota.bin"));
        assert!(!read.contains("\"path\": \"firmware.ota.bin\""));
    }

    // --- Test: store and read firmware ---
    #[test]
    fn test_store_and_read_firmware() {
        let (_tmp, cache) = setup();
        let data = vec![0xDE, 0xAD, 0xBE, 0xEF];
        cache.store_firmware("test-device", &data).unwrap();

        let read = cache.read_firmware("test-device").unwrap();
        assert_eq!(read, data);
    }

    // --- Test: read nonexistent returns None ---
    #[test]
    fn test_read_nonexistent_manifest() {
        let (_tmp, cache) = setup();
        assert!(cache.read_manifest("nonexistent").is_none());
    }

    #[test]
    fn test_read_nonexistent_firmware() {
        let (_tmp, cache) = setup();
        assert!(cache.read_firmware("nonexistent").is_none());
    }

    // --- Test: list devices ---
    #[test]
    fn test_list_devices() {
        let (_tmp, cache) = setup();
        cache
            .store_firmware("device-b", &[1])
            .unwrap();
        cache
            .store_firmware("device-a", &[2])
            .unwrap();

        let devices = cache.list_devices();
        assert_eq!(devices, vec!["device-a", "device-b"]);
    }

    #[test]
    fn test_list_devices_empty() {
        let (_tmp, cache) = setup();
        assert!(cache.list_devices().is_empty());
    }

    // --- Test: sync-delete ---
    #[test]
    fn test_sync_delete_removes_stale() {
        let (_tmp, cache) = setup();
        cache.store_firmware("keep-me", &[1]).unwrap();
        cache.store_firmware("remove-me", &[2]).unwrap();

        let removed = cache.sync_delete(&["keep-me".to_string()]);
        assert_eq!(removed, vec!["remove-me"]);
        assert_eq!(cache.list_devices(), vec!["keep-me"]);
    }

    #[test]
    fn test_sync_delete_nothing_to_remove() {
        let (_tmp, cache) = setup();
        cache.store_firmware("device-a", &[1]).unwrap();

        let removed = cache.sync_delete(&["device-a".to_string()]);
        assert!(removed.is_empty());
    }

    // --- Test: version extraction ---
    #[test]
    fn test_cached_version() {
        let (_tmp, cache) = setup();
        let manifest = r#"{"version":"2025.3.1","builds":[{"path":"firmware.ota.bin"}]}"#;
        cache
            .store_manifest("versioned", manifest, "http://localhost:8099")
            .unwrap();

        assert_eq!(cache.cached_version("versioned").unwrap(), "2025.3.1");
    }

    #[test]
    fn test_extract_version_missing() {
        assert!(extract_version_from_manifest(r#"{"builds":[]}"#).is_none());
    }

    #[test]
    fn test_extract_version_invalid_json() {
        assert!(extract_version_from_manifest("not json").is_none());
    }

    // --- Test: manifest URL rewrite ---
    #[test]
    fn test_rewrite_manifest_urls() {
        let input = r#"{"version":"1.0","builds":[{"path":"https://github.com/some/url/firmware.ota.bin","chipFamily":"ESP32"}]}"#;
        let result = rewrite_manifest_urls(input, "my-esp", "http://10.0.0.5:8099").unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        let path = parsed["builds"][0]["path"].as_str().unwrap();
        assert_eq!(path, "http://10.0.0.5:8099/devices/my-esp/firmware.ota.bin");
        // Other fields preserved
        assert_eq!(parsed["builds"][0]["chipFamily"].as_str().unwrap(), "ESP32");
        assert_eq!(parsed["version"].as_str().unwrap(), "1.0");
    }

    #[test]
    fn test_rewrite_manifest_trailing_slash() {
        let input = r#"{"builds":[{"path":"old"}]}"#;
        let result = rewrite_manifest_urls(input, "dev", "http://host:8099/").unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(
            parsed["builds"][0]["path"].as_str().unwrap(),
            "http://host:8099/devices/dev/firmware.ota.bin"
        );
    }

    #[test]
    fn test_rewrite_manifest_no_builds() {
        let input = r#"{"version":"1.0"}"#;
        let result = rewrite_manifest_urls(input, "dev", "http://host:8099").unwrap();
        // Should not crash, just pass through
        assert!(result.contains("1.0"));
    }

    // --- Test: ensure_dir ---
    #[test]
    fn test_ensure_dir_creates_directory() {
        let tmp = TempDir::new().unwrap();
        let cache_path = tmp.path().join("new-cache");
        let cache = FirmwareCache::new(cache_path.to_str().unwrap());
        cache.ensure_dir().unwrap();
        assert!(cache_path.exists());
    }
}
