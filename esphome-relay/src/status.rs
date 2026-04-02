use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Normalize a version string for comparison:
/// strips leading 'v'/'V', then tries semver parse.
pub fn versions_match(a: &str, b: &str) -> bool {
    let a = a
        .strip_prefix('v')
        .or_else(|| a.strip_prefix('V'))
        .unwrap_or(a);
    let b = b
        .strip_prefix('v')
        .or_else(|| b.strip_prefix('V'))
        .unwrap_or(b);

    // Try semver comparison first
    if let (Ok(va), Ok(vb)) = (semver::Version::parse(a), semver::Version::parse(b)) {
        return va == vb;
    }
    // Fallback: plain string comparison after stripping prefix
    a == b
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncStatus {
    Aktuell,
    Veraltet,
    LadeFirmware,
    Fehler,
    Unbekannt,
}

impl std::fmt::Display for SyncStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncStatus::Aktuell => write!(f, "aktuell"),
            SyncStatus::Veraltet => write!(f, "veraltet"),
            SyncStatus::LadeFirmware => write!(f, "lade Firmware…"),
            SyncStatus::Fehler => write!(f, "Fehler"),
            SyncStatus::Unbekannt => write!(f, "unbekannt"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceStatus {
    pub name: String,
    pub local_version: Option<String>,
    pub remote_version: Option<String>,
    pub sync_status: SyncStatus,
}

impl DeviceStatus {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            local_version: None,
            remote_version: None,
            sync_status: SyncStatus::Unbekannt,
        }
    }

    /// Recalculate sync status based on local vs remote version
    pub fn update_sync_status(&mut self) {
        self.sync_status = match (&self.local_version, &self.remote_version) {
            (Some(local), Some(remote)) if versions_match(local, remote) => SyncStatus::Aktuell,
            (Some(_), Some(_)) => SyncStatus::Veraltet,
            (None, Some(_)) => SyncStatus::Veraltet,
            (Some(_), None) => SyncStatus::Aktuell, // local only, no remote = fine
            (None, None) => SyncStatus::Unbekannt,
        };
    }
}

/// Thread-safe status tracker for all devices
#[derive(Clone)]
pub struct StatusTracker {
    devices: Arc<RwLock<HashMap<String, DeviceStatus>>>,
}

impl StatusTracker {
    pub fn new() -> Self {
        Self {
            devices: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Set local version for a device
    pub fn set_local_version(&self, device: &str, version: &str) {
        let mut map = self.devices.write().unwrap();
        let status = map
            .entry(device.to_string())
            .or_insert_with(|| DeviceStatus::new(device));
        status.local_version = Some(version.to_string());
        status.update_sync_status();
    }

    /// Set remote version for a device
    pub fn set_remote_version(&self, device: &str, version: &str) {
        let mut map = self.devices.write().unwrap();
        let status = map
            .entry(device.to_string())
            .or_insert_with(|| DeviceStatus::new(device));
        status.remote_version = Some(version.to_string());
        status.update_sync_status();
    }

    /// Set sync status directly (e.g. "loading" or "error")
    pub fn set_sync_status(&self, device: &str, sync_status: SyncStatus) {
        let mut map = self.devices.write().unwrap();
        let status = map
            .entry(device.to_string())
            .or_insert_with(|| DeviceStatus::new(device));
        status.sync_status = sync_status;
    }

    /// Get status for a single device
    #[allow(dead_code)]
    pub fn get_device(&self, device: &str) -> Option<DeviceStatus> {
        let map = self.devices.read().unwrap();
        map.get(device).cloned()
    }

    /// Get all device statuses, sorted by name
    pub fn all_devices(&self) -> Vec<DeviceStatus> {
        let map = self.devices.read().unwrap();
        let mut devices: Vec<DeviceStatus> = map.values().cloned().collect();
        devices.sort_by(|a, b| a.name.cmp(&b.name));
        devices
    }

    /// Remove device from tracker
    pub fn remove_device(&self, device: &str) {
        let mut map = self.devices.write().unwrap();
        map.remove(device);
    }

    /// Remove all devices not in the given set
    pub fn retain_devices(&self, device_names: &[String]) {
        let mut map = self.devices.write().unwrap();
        map.retain(|name, _| device_names.contains(name));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- SyncStatus tests ---

    #[test]
    fn test_sync_status_display() {
        assert_eq!(SyncStatus::Aktuell.to_string(), "aktuell");
        assert_eq!(SyncStatus::Veraltet.to_string(), "veraltet");
        assert_eq!(SyncStatus::LadeFirmware.to_string(), "lade Firmware…");
        assert_eq!(SyncStatus::Fehler.to_string(), "Fehler");
        assert_eq!(SyncStatus::Unbekannt.to_string(), "unbekannt");
    }

    // --- DeviceStatus tests ---

    #[test]
    fn test_device_status_new() {
        let ds = DeviceStatus::new("my-esp");
        assert_eq!(ds.name, "my-esp");
        assert!(ds.local_version.is_none());
        assert!(ds.remote_version.is_none());
        assert_eq!(ds.sync_status, SyncStatus::Unbekannt);
    }

    #[test]
    fn test_sync_status_both_equal() {
        let mut ds = DeviceStatus::new("dev");
        ds.local_version = Some("v1.0".into());
        ds.remote_version = Some("v1.0".into());
        ds.update_sync_status();
        assert_eq!(ds.sync_status, SyncStatus::Aktuell);
    }

    #[test]
    fn test_sync_status_versions_differ() {
        let mut ds = DeviceStatus::new("dev");
        ds.local_version = Some("v1.0".into());
        ds.remote_version = Some("v2.0".into());
        ds.update_sync_status();
        assert_eq!(ds.sync_status, SyncStatus::Veraltet);
    }

    #[test]
    fn test_sync_status_no_local() {
        let mut ds = DeviceStatus::new("dev");
        ds.remote_version = Some("v1.0".into());
        ds.update_sync_status();
        assert_eq!(ds.sync_status, SyncStatus::Veraltet);
    }

    #[test]
    fn test_sync_status_local_only() {
        let mut ds = DeviceStatus::new("dev");
        ds.local_version = Some("v1.0".into());
        ds.update_sync_status();
        assert_eq!(ds.sync_status, SyncStatus::Aktuell);
    }

    #[test]
    fn test_sync_status_both_none() {
        let mut ds = DeviceStatus::new("dev");
        ds.update_sync_status();
        assert_eq!(ds.sync_status, SyncStatus::Unbekannt);
    }

    // --- StatusTracker tests ---

    #[test]
    fn test_tracker_set_local_version() {
        let tracker = StatusTracker::new();
        tracker.set_local_version("esp-1", "v1.0");
        let dev = tracker.get_device("esp-1").unwrap();
        assert_eq!(dev.local_version.as_deref(), Some("v1.0"));
    }

    #[test]
    fn test_tracker_set_remote_version() {
        let tracker = StatusTracker::new();
        tracker.set_remote_version("esp-1", "v2.0");
        let dev = tracker.get_device("esp-1").unwrap();
        assert_eq!(dev.remote_version.as_deref(), Some("v2.0"));
        assert_eq!(dev.sync_status, SyncStatus::Veraltet);
    }

    #[test]
    fn test_tracker_both_versions_match() {
        let tracker = StatusTracker::new();
        tracker.set_remote_version("esp-1", "v1.0");
        tracker.set_local_version("esp-1", "v1.0");
        let dev = tracker.get_device("esp-1").unwrap();
        assert_eq!(dev.sync_status, SyncStatus::Aktuell);
    }

    #[test]
    fn test_tracker_set_sync_status_directly() {
        let tracker = StatusTracker::new();
        tracker.set_sync_status("esp-1", SyncStatus::LadeFirmware);
        let dev = tracker.get_device("esp-1").unwrap();
        assert_eq!(dev.sync_status, SyncStatus::LadeFirmware);
    }

    #[test]
    fn test_tracker_get_nonexistent() {
        let tracker = StatusTracker::new();
        assert!(tracker.get_device("nope").is_none());
    }

    #[test]
    fn test_tracker_all_devices_sorted() {
        let tracker = StatusTracker::new();
        tracker.set_local_version("z-device", "v1");
        tracker.set_local_version("a-device", "v1");
        tracker.set_local_version("m-device", "v1");

        let all = tracker.all_devices();
        let names: Vec<&str> = all.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["a-device", "m-device", "z-device"]);
    }

    #[test]
    fn test_tracker_remove_device() {
        let tracker = StatusTracker::new();
        tracker.set_local_version("esp-1", "v1");
        tracker.remove_device("esp-1");
        assert!(tracker.get_device("esp-1").is_none());
    }

    #[test]
    fn test_tracker_retain_devices() {
        let tracker = StatusTracker::new();
        tracker.set_local_version("keep", "v1");
        tracker.set_local_version("drop", "v1");
        tracker.retain_devices(&["keep".to_string()]);

        assert!(tracker.get_device("keep").is_some());
        assert!(tracker.get_device("drop").is_none());
    }

    // --- Serialization test ---

    #[test]
    fn test_device_status_json() {
        let mut ds = DeviceStatus::new("test");
        ds.local_version = Some("v1.0".into());
        ds.remote_version = Some("v1.0".into());
        ds.sync_status = SyncStatus::Aktuell;

        let json = serde_json::to_string(&ds).unwrap();
        assert!(json.contains("\"sync_status\":\"aktuell\""));
        assert!(json.contains("\"local_version\":\"v1.0\""));
    }

    // --- Thread safety test ---

    #[test]
    fn test_tracker_clone_shares_state() {
        let tracker1 = StatusTracker::new();
        let tracker2 = tracker1.clone();
        tracker1.set_local_version("dev", "v1");
        let dev = tracker2.get_device("dev").unwrap();
        assert_eq!(dev.local_version.as_deref(), Some("v1"));
    }

    // --- versions_match tests ---

    #[test]
    fn test_versions_match_identical() {
        assert!(versions_match("v1.0.0", "v1.0.0"));
    }

    #[test]
    fn test_versions_match_with_and_without_v_prefix() {
        assert!(versions_match("v1.0.0", "1.0.0"));
        assert!(versions_match("1.0.0", "v1.0.0"));
    }

    #[test]
    fn test_versions_match_uppercase_v() {
        assert!(versions_match("V1.0.0", "1.0.0"));
    }

    #[test]
    fn test_versions_differ() {
        assert!(!versions_match("v1.0.0", "v2.0.0"));
    }

    #[test]
    fn test_versions_match_non_semver_fallback() {
        // Non-semver strings: plain comparison after stripping v
        assert!(versions_match("v2025.1", "2025.1"));
        assert!(!versions_match("v2025.1", "2025.2"));
    }

    #[test]
    fn test_versions_match_calver_style() {
        assert!(versions_match("v2025.3.1", "2025.3.1"));
    }

    #[test]
    fn test_sync_status_v_prefix_mismatch_still_equal() {
        let mut ds = DeviceStatus::new("dev");
        ds.local_version = Some("1.0.0".into());
        ds.remote_version = Some("v1.0.0".into());
        ds.update_sync_status();
        assert_eq!(ds.sync_status, SyncStatus::Aktuell);
    }
}
