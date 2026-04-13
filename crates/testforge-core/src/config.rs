//! Project configuration management.
//!
//! TestForge reads its settings from `.testforge/config.toml` at the
//! project root.  Every field has a sensible default so users can start
//! with a bare-minimum file (or none at all).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Result, TestForgeError};
use crate::models::Language;

// ─── Top-Level Config ───────────────────────────────────────────────

/// Root configuration for a TestForge project.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TestForgeConfig {
    pub project: ProjectConfig,
    pub indexer: IndexerConfig,
    pub embeddings: EmbeddingsConfig,
    pub llm: LlmConfig,
    pub generation: GenerationConfig,
    pub server: ServerConfig,
}

impl Default for TestForgeConfig {
    fn default() -> Self {
        Self {
            project: ProjectConfig::default(),
            indexer: IndexerConfig::default(),
            embeddings: EmbeddingsConfig::default(),
            llm: LlmConfig::default(),
            generation: GenerationConfig::default(),
            server: ServerConfig::default(),
        }
    }
}

impl TestForgeConfig {
    /// Load configuration from a TOML file, falling back to defaults
    /// for any missing fields.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(TestForgeError::ConfigNotFound {
                path: path.to_path_buf(),
            });
        }

        let content = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&content)?;
        config.validate()?;
        Ok(config)
    }

    /// Load from the conventional location (`.testforge/config.toml`)
    /// relative to `project_root`.  Returns defaults if the file
    /// doesn't exist.
    pub fn load_or_default(project_root: &Path) -> Result<Self> {
        let path = project_root.join(".testforge").join("config.toml");
        if path.exists() {
            Self::load(&path)
        } else {
            Ok(Self::default())
        }
    }

    /// Persist the current config to disk.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self).map_err(|e| {
            TestForgeError::ConfigInvalid {
                message: e.to_string(),
            }
        })?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Validate cross-field constraints.
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
        Ok(())
    }
}

// ─── Section: Project ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectConfig {
    pub name: String,
    pub languages: Vec<Language>,
    pub exclude: Vec<String>,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            name: String::from("my-project"),
            languages: vec![Language::Python],
            exclude: vec![
                "node_modules".into(),
                ".venv".into(),
                "__pycache__".into(),
                ".git".into(),
                "target".into(),
                "dist".into(),
                "build".into(),
                ".testforge".into(),
            ],
        }
    }
}

// ─── Section: Indexer ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct IndexerConfig {
    /// Skip files larger than this (in KB).
    pub max_file_size_kb: u64,

    /// Re-index automatically when files change.
    pub watch: bool,

    /// Number of parallel indexing workers.
    pub parallelism: usize,
}

impl Default for IndexerConfig {
    fn default() -> Self {
        Self {
            max_file_size_kb: 500,
            watch: false,
            parallelism: num_cpus(),
        }
    }
}

// ─── Section: Embeddings ────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EmbeddingsConfig {
    /// Provider: `"local"` (sentence-transformers) or `"openai"`.
    pub provider: String,

    /// Model identifier.
    pub model: String,

    /// API key for remote providers (reads from env if suffixed with `_ENV`).
    pub api_key: Option<String>,

    /// Dimension of the embedding vectors (auto-detected if omitted).
    pub dimension: Option<usize>,

    /// Whether to cache embeddings on disk.
    pub cache_enabled: bool,

    /// Directory to store cached embeddings.
    pub cache_dir: Option<PathBuf>,
}

impl Default for EmbeddingsConfig {
    fn default() -> Self {
        Self {
            provider: "local".into(),
            model: "all-MiniLM-L6-v2".into(),
            api_key: None,
            dimension: None,
            cache_enabled: true,
            cache_dir: None,
        }
    }
}

// ─── Section: LLM ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    /// Provider: `"claude"`, `"openai"`, or `"local"`.
    pub provider: String,

    /// Model identifier.
    pub model: String,

    /// Environment variable name that holds the API key.
    pub api_key_env: String,

    /// Max tokens for generation requests.
    pub max_tokens: u32,

    /// Temperature for generation (0.0–1.0).
    pub temperature: f32,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: "claude".into(),
            model: "claude-sonnet-4-20250514".into(),
            api_key_env: "ANTHROPIC_API_KEY".into(),
            max_tokens: 4096,
            temperature: 0.2,
        }
    }
}

// ─── Section: Generation ────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GenerationConfig {
    /// Target test framework (e.g. `"pytest"`, `"jest"`, `"cargo_test"`).
    pub test_framework: String,

    /// Generate edge-case tests automatically.
    pub include_edge_cases: bool,

    /// Generate mock objects for dependencies.
    pub include_mocks: bool,

    /// Run generated tests immediately after creation.
    pub auto_run: bool,

    /// Output directory for generated test files.
    pub output_dir: PathBuf,
}

impl Default for GenerationConfig {
    fn default() -> Self {
        Self {
            test_framework: "pytest".into(),
            include_edge_cases: true,
            include_mocks: true,
            auto_run: false,
            output_dir: PathBuf::from("tests/generated"),
        }
    }
}

// ─── Section: Server ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 7654,
        }
    }
}

// ─── Helpers ────────────────────────────────────────────────────────

fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        let cfg = TestForgeConfig::default();
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn roundtrip_toml() {
        let cfg = TestForgeConfig::default();
        let serialized = toml::to_string_pretty(&cfg).unwrap();
        let deserialized: TestForgeConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(cfg.server.port, deserialized.server.port);
    }

    #[test]
    fn partial_toml_fills_defaults() {
        let partial = r#"
[project]
name = "acme"

[server]
port = 9999
"#;
        let cfg: TestForgeConfig = toml::from_str(partial).unwrap();
        assert_eq!(cfg.project.name, "acme");
        assert_eq!(cfg.server.port, 9999);
        // rest is default
        assert_eq!(cfg.embeddings.provider, "local");
    }
}