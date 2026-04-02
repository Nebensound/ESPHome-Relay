use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub github_token: String,
    #[serde(default = "default_repo")]
    pub github_repo: String,
    #[serde(default = "default_poll_interval")]
    pub poll_interval_minutes: u32,
    #[serde(default)]
    pub webhook_secret: Option<String>,
    #[serde(default = "default_cache_dir")]
    pub cache_dir: String,
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

fn default_repo() -> String {
    "Nebensound/wagnerhof-esphome".to_string()
}

fn default_poll_interval() -> u32 {
    30
}

fn default_cache_dir() -> String {
    "/data/firmware-cache".to_string()
}

fn default_log_level() -> String {
    "info".to_string()
}

impl Config {
    pub fn load_from_file(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = serde_json::from_str(&content)?;
        config.validate()?;
        Ok(config)
    }

    pub fn load() -> Result<Self, Box<dyn std::error::Error>> {
        Self::load_from_file(Path::new("/data/options.json"))
    }

    fn validate(&self) -> Result<(), Box<dyn std::error::Error>> {
        if self.github_token.is_empty() {
            return Err("github_token darf nicht leer sein".into());
        }
        if self.github_repo.is_empty() {
            return Err("github_repo darf nicht leer sein".into());
        }
        if !self.github_repo.contains('/') {
            return Err("github_repo muss im Format 'owner/repo' sein".into());
        }
        if !(5..=1440).contains(&self.poll_interval_minutes) {
            return Err("poll_interval_minutes muss zwischen 5 und 1440 liegen".into());
        }
        let valid_levels = ["trace", "debug", "info", "warn", "error"];
        if !valid_levels.contains(&self.log_level.as_str()) {
            return Err(format!(
                "log_level muss einer von {:?} sein, war: {}",
                valid_levels, self.log_level
            )
            .into());
        }
        Ok(())
    }

    /// Returns (owner, repo) tuple parsed from github_repo
    pub fn repo_parts(&self) -> (&str, &str) {
        let (owner, repo) = self.github_repo.split_once('/').unwrap();
        (owner, repo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_config(json: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(json.as_bytes()).unwrap();
        f
    }

    #[test]
    fn test_full_config() {
        let f = write_config(
            r#"{
            "github_token": "ghp_test123",
            "github_repo": "Nebensound/wagnerhof-esphome",
            "poll_interval_minutes": 15,
            "webhook_secret": "mysecret",
            "cache_dir": "/tmp/cache",
            "log_level": "debug"
        }"#,
        );
        let cfg = Config::load_from_file(f.path()).unwrap();
        assert_eq!(cfg.github_token, "ghp_test123");
        assert_eq!(cfg.github_repo, "Nebensound/wagnerhof-esphome");
        assert_eq!(cfg.poll_interval_minutes, 15);
        assert_eq!(cfg.webhook_secret.as_deref(), Some("mysecret"));
        assert_eq!(cfg.cache_dir, "/tmp/cache");
        assert_eq!(cfg.log_level, "debug");
    }

    #[test]
    fn test_defaults() {
        let f = write_config(r#"{ "github_token": "ghp_abc" }"#);
        let cfg = Config::load_from_file(f.path()).unwrap();
        assert_eq!(cfg.github_repo, "Nebensound/wagnerhof-esphome");
        assert_eq!(cfg.poll_interval_minutes, 30);
        assert_eq!(cfg.cache_dir, "/data/firmware-cache");
        assert_eq!(cfg.log_level, "info");
        assert!(cfg.webhook_secret.is_none());
    }

    #[test]
    fn test_empty_token_fails() {
        let f = write_config(r#"{ "github_token": "" }"#);
        let err = Config::load_from_file(f.path()).unwrap_err();
        assert!(err.to_string().contains("github_token"));
    }

    #[test]
    fn test_invalid_repo_format() {
        let f = write_config(r#"{ "github_token": "ghp_x", "github_repo": "noslash" }"#);
        let err = Config::load_from_file(f.path()).unwrap_err();
        assert!(err.to_string().contains("owner/repo"));
    }

    #[test]
    fn test_poll_interval_too_low() {
        let f = write_config(r#"{ "github_token": "ghp_x", "poll_interval_minutes": 2 }"#);
        let err = Config::load_from_file(f.path()).unwrap_err();
        assert!(err.to_string().contains("poll_interval_minutes"));
    }

    #[test]
    fn test_poll_interval_too_high() {
        let f = write_config(r#"{ "github_token": "ghp_x", "poll_interval_minutes": 9999 }"#);
        let err = Config::load_from_file(f.path()).unwrap_err();
        assert!(err.to_string().contains("poll_interval_minutes"));
    }

    #[test]
    fn test_invalid_log_level() {
        let f = write_config(r#"{ "github_token": "ghp_x", "log_level": "verbose" }"#);
        let err = Config::load_from_file(f.path()).unwrap_err();
        assert!(err.to_string().contains("log_level"));
    }

    #[test]
    fn test_invalid_json() {
        let f = write_config("not json");
        assert!(Config::load_from_file(f.path()).is_err());
    }

    #[test]
    fn test_missing_file() {
        assert!(Config::load_from_file(Path::new("/nonexistent/file.json")).is_err());
    }

    #[test]
    fn test_repo_parts() {
        let f = write_config(r#"{ "github_token": "ghp_x", "github_repo": "Owner/MyRepo" }"#);
        let cfg = Config::load_from_file(f.path()).unwrap();
        let (owner, repo) = cfg.repo_parts();
        assert_eq!(owner, "Owner");
        assert_eq!(repo, "MyRepo");
    }
}
