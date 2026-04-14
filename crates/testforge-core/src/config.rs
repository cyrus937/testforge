//! Configuration management for TestForge.
//!
//! Configuration is read from `.testforge/config.toml` at the project root.
//! Every field has a sensible default so a minimal config file works out of the box.
//!
//! # Example
//!
//! ```toml
//! [project]
//! name = "my-app"
//! languages = ["python", "typescript"]
//!
//! [llm]
//! provider = "claude"
//! ```

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Result, TestForgeError};

/// Root configuration structure, mapping 1:1 to `.testforge/config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub project: ProjectConfig,
    pub indexer: IndexerConfig,
    pub embeddings: EmbeddingsConfig,
    pub llm: LlmConfig,
    pub generation: GenerationConfig,
    pub server: ServerConfig,
}

/// Project-level metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectConfig {
    /// Human-readable project name (defaults to directory name).
    pub name: String,

    /// Languages to index. Empty means auto-detect.
    pub languages: Vec<String>,

    /// Glob patterns to exclude from indexing.
    pub exclude: Vec<String>,
}

/// Indexer tuning knobs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct IndexerConfig {
    /// Maximum file size to index (in KB). Files above this are skipped.
    pub max_file_size_kb: u64,

    /// Whether to start the file watcher for incremental re-indexing.
    pub watch: bool,

    /// Number of parallel parsing threads (0 = auto-detect).
    pub parallelism: usize,
}

/// Embedding model configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EmbeddingsConfig {
    /// Provider: `"local"` (sentence-transformers) or `"openai"`.
    pub provider: String,

    /// Model name or path.
    pub model: String,

    /// Batch size for embedding generation.
    pub batch_size: usize,

    /// Optional API key (only for remote providers).
    pub api_key_env: Option<String>,
}

/// LLM provider configuration (for test generation).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    /// Provider: `"claude"`, `"openai"`, or `"local"`.
    pub provider: String,

    /// Model identifier.
    pub model: String,

    /// Environment variable holding the API key.
    pub api_key_env: Option<String>,

    /// Maximum tokens per generation request.
    pub max_tokens: usize,

    /// Sampling temperature.
    pub temperature: f32,
}

/// Test generation preferences.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GenerationConfig {
    /// Target test framework (e.g., `"pytest"`, `"jest"`, `"cargo-test"`).
    pub test_framework: String,

    /// Whether to generate edge-case tests.
    pub include_edge_cases: bool,

    /// Whether to generate mock/stub setups.
    pub include_mocks: bool,

    /// Automatically run generated tests after creation.
    pub auto_run: bool,

    /// Output directory for generated test files.
    pub output_dir: String,
}

/// Built-in API server settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    /// Enable CORS (useful for VS Code extension).
    pub cors: bool,
}

// ── Defaults ─────────────────────────────────────────────────────────

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            name: String::from("testforge-project"),
            languages: Vec::new(),
            exclude: vec![
                "node_modules".into(),
                ".venv".into(),
                "__pycache__".into(),
                "target".into(),
                "dist".into(),
                ".git".into(),
                "*.min.js".into(),
                "*.lock".into(),
            ],
        }
    }
}

impl Default for IndexerConfig {
    fn default() -> Self {
        Self {
            max_file_size_kb: 500,
            watch: false,
            parallelism: 0,
        }
    }
}

impl Default for EmbeddingsConfig {
    fn default() -> Self {
        Self {
            provider: "local".into(),
            model: "all-MiniLM-L6-v2".into(),
            batch_size: 64,
            api_key_env: None,
        }
    }
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: "claude".into(),
            model: "claude-sonnet-4-20250514".into(),
            api_key_env: Some("ANTHROPIC_API_KEY".into()),
            max_tokens: 4096,
            temperature: 0.2,
        }
    }
}

impl Default for GenerationConfig {
    fn default() -> Self {
        Self {
            test_framework: "pytest".into(),
            include_edge_cases: true,
            include_mocks: true,
            auto_run: false,
            output_dir: "tests/generated".into(),
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 7654,
            cors: true,
        }
    }
}

// ── Loading & Saving ─────────────────────────────────────────────────

/// Name of the hidden config directory.
pub const CONFIG_DIR: &str = ".testforge";

/// Name of the config file inside the config directory.
pub const CONFIG_FILE: &str = "config.toml";

impl Config {
    /// Discover and load config by walking up from `start` until we find `.testforge/`.
    ///
    /// Returns the parsed config and the absolute path to the project root.
    pub fn discover(start: &Path) -> Result<(Self, PathBuf)> {
        let root = Self::find_project_root(start)?;
        let config_path = root.join(CONFIG_DIR).join(CONFIG_FILE);
        let config = Self::load(&config_path)?;
        Ok((config, root))
    }

    /// Load configuration from a specific TOML file.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(TestForgeError::ConfigNotFound {
                path: path.to_path_buf(),
            });
        }

        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        config.validate()?;
        Ok(config)
    }

    /// Write the configuration to a TOML file.
    pub fn save(&self, path: &Path) -> Result<()> {
        let content = toml::to_string_pretty(self)
            .map_err(|e| TestForgeError::internal(format!("Failed to serialize config: {e}")))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Initialize a new `.testforge/` directory at the given project root.
    ///
    /// Creates the config directory and writes a default `config.toml`.
    /// Returns the path to the created config file.
    pub fn init(project_root: &Path, project_name: Option<&str>) -> Result<PathBuf> {
        let config_dir = project_root.join(CONFIG_DIR);
        std::fs::create_dir_all(&config_dir)?;

        let config_path = config_dir.join(CONFIG_FILE);
        if config_path.exists() {
            tracing::warn!("Configuration already exists at {}", config_path.display());
            return Ok(config_path);
        }

        let mut config = Config::default();
        if let Some(name) = project_name {
            config.project.name = name.to_string();
        } else {
            // Use directory name as project name
            if let Some(dir_name) = project_root.file_name().and_then(|n| n.to_str()) {
                config.project.name = dir_name.to_string();
            }
        }

        config.save(&config_path)?;

        // Create data subdirectories
        std::fs::create_dir_all(config_dir.join("index"))?;
        std::fs::create_dir_all(config_dir.join("cache"))?;

        tracing::info!("Initialized TestForge at {}", config_path.display());
        Ok(config_path)
    }

    /// Walk up from `start` looking for a `.testforge/` directory.
    fn find_project_root(start: &Path) -> Result<PathBuf> {
        let start = std::fs::canonicalize(start).unwrap_or_else(|_| start.to_path_buf());
        let mut current = start.as_path();

        loop {
            if current.join(CONFIG_DIR).is_dir() {
                return Ok(current.to_path_buf());
            }
            match current.parent() {
                Some(parent) => current = parent,
                None => {
                    return Err(TestForgeError::ConfigNotFound {
                        path: start.join(CONFIG_DIR).join(CONFIG_FILE),
                    });
                }
            }
        }
    }

    /// Validate configuration invariants.
    fn validate(&self) -> Result<()> {
        if self.indexer.max_file_size_kb == 0 {
            return Err(TestForgeError::ConfigInvalid {
                message: "indexer.max_file_size_kb must be > 0".into(),
            });
        }
        if self.server.port == 0 {
            return Err(TestForgeError::ConfigInvalid {
                message: "server.port must be > 0".into(),
            });
        }

        let valid_llm_providers = ["claude", "openai", "local"];
        if !valid_llm_providers.contains(&self.llm.provider.as_str()) {
            return Err(TestForgeError::ConfigInvalid {
                message: format!(
                    "Unknown LLM provider '{}'. Valid: {}",
                    self.llm.provider,
                    valid_llm_providers.join(", ")
                ),
            });
        }

        let valid_embed_providers = ["local", "openai"];
        if !valid_embed_providers.contains(&self.embeddings.provider.as_str()) {
            return Err(TestForgeError::ConfigInvalid {
                message: format!(
                    "Unknown embeddings provider '{}'. Valid: {}",
                    self.embeddings.provider,
                    valid_embed_providers.join(", ")
                ),
            });
        }

        Ok(())
    }

    /// Resolve the effective number of indexing threads.
    pub fn effective_parallelism(&self) -> usize {
        if self.indexer.parallelism == 0 {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
        } else {
            self.indexer.parallelism
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn default_config_is_valid() {
        let config = Config::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn init_creates_config_directory() {
        let dir = TempDir::new().unwrap();
        let result = Config::init(dir.path(), Some("test-project"));
        assert!(result.is_ok());

        let config_path = result.unwrap();
        assert!(config_path.exists());
        assert!(dir.path().join(CONFIG_DIR).join("index").exists());
        assert!(dir.path().join(CONFIG_DIR).join("cache").exists());
    }

    #[test]
    fn roundtrip_save_load() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");

        let original = Config::default();
        original.save(&path).unwrap();

        let loaded = Config::load(&path).unwrap();
        assert_eq!(original.project.name, loaded.project.name);
        assert_eq!(original.server.port, loaded.server.port);
    }

    #[test]
    fn discover_walks_up_directories() {
        let dir = TempDir::new().unwrap();
        Config::init(dir.path(), Some("root-project")).unwrap();

        let nested = dir.path().join("src").join("deep").join("module");
        std::fs::create_dir_all(&nested).unwrap();

        let (config, root) = Config::discover(&nested).unwrap();
        assert_eq!(config.project.name, "root-project");
        assert_eq!(root, std::fs::canonicalize(dir.path()).unwrap());
    }

    #[test]
    fn validation_rejects_zero_port() {
        let mut config = Config::default();
        config.server.port = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn validation_rejects_unknown_provider() {
        let mut config = Config::default();
        config.llm.provider = "unknown".into();
        assert!(config.validate().is_err());
    }
}
