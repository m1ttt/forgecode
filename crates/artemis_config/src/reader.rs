use std::path::PathBuf;
use std::sync::LazyLock;

use config::ConfigBuilder;
use config::builder::DefaultState;

use crate::ForgeConfig;
use crate::legacy::LegacyConfig;

/// Loads all `.env` files found while walking up from the current working
/// directory to the root, with priority given to closer (lower) directories.
/// Executed at most once per process.
static LOAD_DOT_ENV: LazyLock<()> = LazyLock::new(|| {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut paths = vec![];
    let mut current = PathBuf::new();

    for component in cwd.components() {
        current.push(component);
        paths.push(current.clone());
    }

    paths.reverse();

    for path in paths {
        let env_file = path.join(".env");
        if env_file.is_file() {
            dotenvy::from_path(&env_file).ok();
        }
    }
});

/// Merges [`ForgeConfig`] from layered sources using a builder pattern.
#[derive(Default)]
pub struct ConfigReader {
    builder: ConfigBuilder<DefaultState>,
}

impl ConfigReader {
    /// Returns the path to the legacy JSON config file
    /// (`~/.artemis/.config.json`).
    pub fn config_legacy_path() -> PathBuf {
        Self::base_path().join(".config.json")
    }

    /// Returns the path to the primary TOML config file
    /// (`~/.artemis/.artemis.toml`). Falls back to checking `.forge.toml`
    /// for backward compatibility.
    pub fn config_path() -> PathBuf {
        let base = Self::base_path();
        let new_path = base.join(".artemis.toml");
        if new_path.exists() {
            return new_path;
        }
        let legacy_path = base.join(".forge.toml");
        if legacy_path.exists() {
            return legacy_path;
        }
        new_path
    }

    /// Returns the base directory for all Artemis config files.
    ///
    /// Resolution order:
    /// 1. `ARTEMIS_CONFIG` environment variable, if set.
    /// 2. `FORGE_CONFIG` environment variable, if set (legacy fallback).
    /// 3. `~/artemis` (legacy path), if that directory exists, so users who have
    ///    not yet run `artemis config migrate` continue to read from their
    ///    existing directory without disruption.
    /// 4. `~/.artemis` as the default path.
    pub fn base_path() -> PathBuf {
        if let Ok(path) = std::env::var("ARTEMIS_CONFIG") {
            return PathBuf::from(path);
        }

        if let Ok(path) = std::env::var("FORGE_CONFIG") {
            return PathBuf::from(path);
        }

        let base = dirs::home_dir().unwrap_or(PathBuf::from("."));
        let path = base.join("artemis");

        // Prefer ~/artemis (legacy) when it exists so existing users are not
        // disrupted; fall back to ~/.artemis as the default.
        if path.exists() {
            tracing::info!("Using legacy path");
            return path;
        }

        tracing::info!("Using new path");
        base.join(".artemis")
    }

    /// Adds the provided TOML string as a config source without touching the
    /// filesystem.
    pub fn read_toml(mut self, contents: &str) -> Self {
        self.builder = self
            .builder
            .add_source(config::File::from_str(contents, config::FileFormat::Toml));

        self
    }

    /// Adds the embedded default config (`../.artemis.toml`) as a source.
    pub fn read_defaults(self) -> Self {
        let defaults = include_str!("../.artemis.toml");

        self.read_toml(defaults)
    }

    /// Adds `ARTEMIS_`-prefixed environment variables as a config source.
    /// Also supports legacy `FORGE_`-prefixed variables as fallback.
    pub fn read_env(mut self) -> Self {
        self.builder = self.builder.add_source(
            config::Environment::with_prefix("ARTEMIS")
                .prefix_separator("_")
                .separator("__")
                .try_parsing(true)
                .list_separator(",")
                .with_list_parse_key("retry.status_codes")
                .with_list_parse_key("http.root_cert_paths"),
        );

        self.builder = self.builder.add_source(
            config::Environment::with_prefix("FORGE")
                .prefix_separator("_")
                .separator("__")
                .try_parsing(true)
                .list_separator(",")
                .with_list_parse_key("retry.status_codes")
                .with_list_parse_key("http.root_cert_paths"),
        );

        self
    }

    /// Builds and deserializes all accumulated sources into a [`ForgeConfig`].
    ///
    /// Triggers `.env` file loading (at most once per process) by walking up
    /// the directory tree from the current working directory, with closer
    /// directories taking priority.
    ///
    /// # Errors
    ///
    /// Returns an error if the configuration cannot be built or deserialized.
    pub fn build(self) -> crate::Result<ForgeConfig> {
        *LOAD_DOT_ENV;
        let config = self.builder.build()?;
        Ok(config.try_deserialize::<ForgeConfig>()?)
    }

    /// Adds `~/.artemis/.artemis.toml` as a config source, silently skipping
    /// if absent.
    pub fn read_global(mut self) -> Self {
        let path = Self::config_path();
        self.builder = self
            .builder
            .add_source(config::File::from(path).required(false));
        self
    }

    /// Reads `~/.artemis/.config.json` (legacy format) and adds it as a
    /// source, silently skipping errors.
    pub fn read_legacy(self) -> Self {
        let content = LegacyConfig::read(&Self::config_legacy_path());
        if let Ok(content) = content {
            self.read_toml(&content)
        } else {
            self
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, MutexGuard};

    use pretty_assertions::assert_eq;

    use super::*;
    use crate::ModelConfig;

    /// Serializes tests that mutate environment variables to prevent races.
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    /// Holds env vars set for a test's duration and removes them on drop, while
    /// holding [`ENV_MUTEX`].
    struct EnvGuard {
        keys: Vec<&'static str>,
        _lock: MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        /// Acquires [`ENV_MUTEX`], sets each `(key, value)` pair in the
        /// environment, and removes each key in `remove` if present. All
        /// set keys are cleaned up on drop.
        #[must_use]
        fn set(pairs: &[(&'static str, &str)]) -> Self {
            Self::set_and_remove(pairs, &[])
        }

        /// Like [`set`] but also removes the listed keys before the test runs.
        #[must_use]
        fn set_and_remove(pairs: &[(&'static str, &str)], remove: &[&'static str]) -> Self {
            let lock = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
            let keys = pairs.iter().map(|(k, _)| *k).collect();
            for key in remove {
                unsafe { std::env::remove_var(key) };
            }
            for (key, value) in pairs {
                unsafe { std::env::set_var(key, value) };
            }
            Self { keys, _lock: lock }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for key in &self.keys {
                unsafe { std::env::remove_var(key) };
            }
        }
    }

    #[test]
    fn test_base_path_uses_artemis_config_env_var() {
        let _guard = EnvGuard::set_and_remove(
            &[("ARTEMIS_CONFIG", "/custom/artemis/dir")],
            &["FORGE_CONFIG"],
        );
        let actual = ConfigReader::base_path();
        let expected = PathBuf::from("/custom/artemis/dir");
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_base_path_forge_config_fallback() {
        let _guard = EnvGuard::set_and_remove(
            &[("FORGE_CONFIG", "/custom/forge/dir")],
            &["ARTEMIS_CONFIG"],
        );
        let actual = ConfigReader::base_path();
        let expected = PathBuf::from("/custom/forge/dir");
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_base_path_artemis_takes_priority_over_forge() {
        let _guard = EnvGuard::set(&[
            ("ARTEMIS_CONFIG", "/artemis/path"),
            ("FORGE_CONFIG", "/forge/path"),
        ]);
        let actual = ConfigReader::base_path();
        let expected = PathBuf::from("/artemis/path");
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_base_path_falls_back_to_home_dir_when_env_var_absent() {
        // Hold the env mutex and ensure both vars are absent so this test
        // cannot race with the env var tests.
        let _guard = EnvGuard::set_and_remove(&[], &["ARTEMIS_CONFIG", "FORGE_CONFIG"]);

        let actual = ConfigReader::base_path();
        // Without either env var set the path must be either "artemis" (legacy,
        // preferred when ~/artemis exists) or ".artemis" (default new path).
        let name = actual.file_name().unwrap();
        assert!(
            name == "artemis" || name == ".artemis",
            "Expected base_path to end with 'artemis' or '.artemis', got: {:?}",
            name
        );
    }

    #[test]
    fn test_read_parses_without_error() {
        let actual = ConfigReader::default().read_defaults().build();
        assert!(actual.is_ok(), "read() failed: {:?}", actual.err());
    }

    #[test]
    fn test_legacy_layer_does_not_overwrite_defaults() {
        // Simulate what `read_legacy` does: serialize a ForgeConfig that only
        // carries session/commit/suggest (all other fields are None) and layer
        // it on top of the embedded defaults. The default values must survive.
        let legacy = ForgeConfig {
            session: Some(ModelConfig {
                provider_id: "anthropic".to_string(),
                model_id: "claude-3".to_string(),
            }),
            ..Default::default()
        };
        let legacy_toml = toml_edit::ser::to_string_pretty(&legacy).unwrap();

        let actual = ConfigReader::default()
            // Read legacy first and then defaults
            .read_toml(&legacy_toml)
            .read_defaults()
            .build()
            .unwrap();

        // Session should come from the legacy layer
        assert_eq!(
            actual.session,
            Some(ModelConfig {
                provider_id: "anthropic".to_string(),
                model_id: "claude-3".to_string(),
            })
        );

        // Default values from .artemis.toml must be retained, not reset to zero
        assert_eq!(actual.max_parallel_file_reads, 64);
        assert_eq!(actual.max_read_lines, 2000);
        assert_eq!(actual.tool_timeout_secs, 300);
        assert_eq!(actual.max_search_lines, 1000);
        assert_eq!(actual.tool_supported, true);
    }

    #[test]
    fn test_read_session_from_env_vars() {
        let _guard = EnvGuard::set_and_remove(
            &[
                ("ARTEMIS_SESSION__PROVIDER_ID", "fake-provider"),
                ("ARTEMIS_SESSION__MODEL_ID", "fake-model"),
            ],
            &["FORGE_SESSION__PROVIDER_ID", "FORGE_SESSION__MODEL_ID"],
        );

        let actual = ConfigReader::default()
            .read_defaults()
            .read_env()
            .build()
            .unwrap();

        let expected = Some(ModelConfig {
            provider_id: "fake-provider".to_string(),
            model_id: "fake-model".to_string(),
        });
        assert_eq!(actual.session, expected);
    }

    #[test]
    fn test_read_session_from_legacy_forge_env_vars() {
        let _guard = EnvGuard::set_and_remove(
            &[
                ("FORGE_SESSION__PROVIDER_ID", "legacy-provider"),
                ("FORGE_SESSION__MODEL_ID", "legacy-model"),
            ],
            &["ARTEMIS_SESSION__PROVIDER_ID", "ARTEMIS_SESSION__MODEL_ID"],
        );

        let actual = ConfigReader::default()
            .read_defaults()
            .read_env()
            .build()
            .unwrap();

        let expected = Some(ModelConfig {
            provider_id: "legacy-provider".to_string(),
            model_id: "legacy-model".to_string(),
        });
        assert_eq!(actual.session, expected);
    }
}
