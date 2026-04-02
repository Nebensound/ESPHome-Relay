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
        let path =
            std::env::var("OPTIONS_PATH").unwrap_or_else(|_| "/data/options.json".to_string());
        Self::load_from_file(Path::new(&path))
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
    use std::collections::HashMap;
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

    /// Validates the HA addon config.yaml against Home Assistant schema rules.
    /// Catches invalid schema types (like `select()`) that would make HA ignore the addon.
    #[test]
    fn test_ha_addon_config_yaml_schema() {
        let yaml_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("config.yaml");
        let content =
            std::fs::read_to_string(&yaml_path).expect("config.yaml should exist in project root");

        let doc: serde_yaml::Value =
            serde_yaml::from_str(&content).expect("config.yaml should be valid YAML");
        let map = doc
            .as_mapping()
            .expect("config.yaml root should be a mapping");

        // Required top-level keys
        for key in ["name", "version", "slug", "arch"] {
            assert!(
                map.contains_key(&serde_yaml::Value::String(key.to_string())),
                "config.yaml missing required key: {key}"
            );
        }

        // Validate schema types if schema section exists
        if let Some(schema_val) = map.get(&serde_yaml::Value::String("schema".to_string())) {
            let schema = schema_val.as_mapping().expect("schema should be a mapping");

            // Valid HA addon schema type patterns
            let valid_patterns: &[&dyn Fn(&str) -> bool] = &[
                &|s: &str| {
                    matches!(
                        s,
                        "str"
                            | "str?"
                            | "bool"
                            | "bool?"
                            | "int"
                            | "int?"
                            | "float"
                            | "float?"
                            | "email"
                            | "email?"
                            | "url"
                            | "url?"
                            | "port"
                            | "port?"
                            | "password"
                            | "password?"
                    )
                },
                &|s: &str| s.starts_with("int(") && s.ends_with(')'), // int(min,max)
                &|s: &str| s.starts_with("float(") && s.ends_with(')'), // float(min,max)
                &|s: &str| s.starts_with("match(") && s.ends_with(')'), // match(regex)
                &|s: &str| s.starts_with("list(") && s.ends_with(')'), // list(type)
                &|s: &str| s.contains('|') && !s.contains('('),       // enum: val1|val2|val3
            ];

            for (key, value) in schema.iter() {
                let key_str = key.as_str().unwrap_or("??");
                if let Some(type_str) = value.as_str() {
                    let is_valid = valid_patterns.iter().any(|check| check(type_str));
                    assert!(
                        is_valid,
                        "Invalid HA addon schema type for '{key_str}': '{type_str}'. \
                         Valid types: str, int, float, bool, email, url, port, password \
                         (with optional ?), int(min,max), float(min,max), match(regex), \
                         list(type), or enum values separated by |"
                    );
                }
            }
        }

        // Validate options defaults match schema keys
        if let (Some(options_val), Some(schema_val)) = (
            map.get(&serde_yaml::Value::String("options".to_string())),
            map.get(&serde_yaml::Value::String("schema".to_string())),
        ) {
            let options: HashMap<String, serde_yaml::Value> =
                serde_yaml::from_value(options_val.clone()).expect("options should be a mapping");
            let schema: HashMap<String, serde_yaml::Value> =
                serde_yaml::from_value(schema_val.clone()).expect("schema should be a mapping");

            for key in options.keys() {
                assert!(
                    schema.contains_key(key),
                    "Option '{key}' has no matching schema entry"
                );
            }
            for key in schema.keys() {
                let is_optional = schema
                    .get(key)
                    .and_then(|v| v.as_str())
                    .is_some_and(|s| s.ends_with('?'));
                if !is_optional {
                    assert!(
                        options.contains_key(key),
                        "Required schema key '{key}' has no default in options"
                    );
                }
            }
        }
    }
}
