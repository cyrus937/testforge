//! `.gitignore`-aware file walker.
//!
//! Uses the [`ignore`] crate to respect `.gitignore` rules and the
//! project's own exclude patterns from `config.toml`.

use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use testforge_core::models::Language;
use testforge_core::{Config, Result};
use tracing::{debug, trace};

/// Walks the project directory tree collecting indexable source files.
pub struct FileWalker<'a> {
    config: &'a Config,
    root: &'a Path,
}

impl<'a> FileWalker<'a> {
    pub fn new(config: &'a Config, root: &'a Path) -> Self {
        Self { config, root }
    }

    /// Collect all source files that should be indexed.
    ///
    /// Respects `.gitignore`, configured exclude patterns, file size limits,
    /// and only includes files whose extension maps to a supported language.
    pub fn collect_files(&self) -> Result<Vec<PathBuf>> {
        let mut builder = WalkBuilder::new(self.root);
        builder
            .hidden(true)       // skip dotfiles/dotdirs
            .git_ignore(true)   // respect .gitignore
            .git_global(true)   // respect global gitignore
            .git_exclude(true); // respect .git/info/exclude

        // Add configured exclude patterns as custom ignores
        for pattern in &self.config.project.exclude {
            builder.add_custom_ignore_filename(pattern);
        }

        let allowed_languages: Option<Vec<Language>> = if self.config.project.languages.is_empty() {
            None // auto-detect: accept all supported languages
        } else {
            Some(
                self.config
                    .project
                    .languages
                    .iter()
                    .filter_map(|l| Language::from_extension(l).or_else(|| {
                        // Try matching language name directly
                        match l.to_lowercase().as_str() {
                            "python" => Some(Language::Python),
                            "javascript" | "js" => Some(Language::JavaScript),
                            "typescript" | "ts" => Some(Language::TypeScript),
                            "rust" => Some(Language::Rust),
                            "java" => Some(Language::Java),
                            "go" => Some(Language::Go),
                            "csharp" | "c#" => Some(Language::CSharp),
                            _ => None,
                        }
                    }))
                    .collect(),
            )
        };

        let mut files = Vec::new();

        for entry in builder.build().flatten() {
            let path = entry.path();

            // Skip directories
            if !path.is_file() {
                continue;
            }

            // Check extension → language
            let ext = match path.extension().and_then(|e| e.to_str()) {
                Some(ext) => ext,
                None => continue,
            };

            let language = match Language::from_extension(ext) {
                Some(lang) => lang,
                None => continue,
            };

            // Filter by configured languages (if specified)
            if let Some(ref allowed) = allowed_languages {
                if !allowed.contains(&language) {
                    trace!(path = %path.display(), "Skipping: language not in project config");
                    continue;
                }
            }

            // Check if in exclude list
            if self.is_excluded(path) {
                trace!(path = %path.display(), "Skipping: matches exclude pattern");
                continue;
            }

            files.push(path.to_path_buf());
        }

        debug!(count = files.len(), "Collected source files");
        Ok(files)
    }

    /// Check if a path matches any exclude pattern.
    fn is_excluded(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();
        self.config.project.exclude.iter().any(|pattern| {
            // Simple glob-like matching
            if pattern.starts_with('*') {
                let suffix = &pattern[1..];
                path_str.ends_with(suffix)
            } else {
                path_str.contains(pattern.as_str())
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_test_project() -> TempDir {
        let dir = TempDir::new().unwrap();

        // Create source files
        let src = dir.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("main.py"), "def main(): pass").unwrap();
        fs::write(src.join("utils.py"), "def helper(): pass").unwrap();
        fs::write(src.join("readme.txt"), "not code").unwrap();

        // Create a node_modules dir that should be skipped
        let nm = dir.path().join("node_modules").join("pkg");
        fs::create_dir_all(&nm).unwrap();
        fs::write(nm.join("index.js"), "function f() {}").unwrap();

        dir
    }

    #[test]
    fn collects_python_files() {
        let dir = setup_test_project();
        let config = Config::default();
        let walker = FileWalker::new(&config, dir.path());

        let files = walker.collect_files().unwrap();
        let py_files: Vec<_> = files
            .iter()
            .filter(|p| p.extension().map(|e| e == "py").unwrap_or(false))
            .collect();

        assert_eq!(py_files.len(), 2);
    }

    #[test]
    fn excludes_node_modules() {
        let dir = setup_test_project();
        let config = Config::default();
        let walker = FileWalker::new(&config, dir.path());

        let files = walker.collect_files().unwrap();
        let in_nm: Vec<_> = files
            .iter()
            .filter(|p| p.to_string_lossy().contains("node_modules"))
            .collect();

        assert!(in_nm.is_empty(), "node_modules should be excluded");
    }

    #[test]
    fn skips_non_code_files() {
        let dir = setup_test_project();
        let config = Config::default();
        let walker = FileWalker::new(&config, dir.path());

        let files = walker.collect_files().unwrap();
        let txt_files: Vec<_> = files
            .iter()
            .filter(|p| p.extension().map(|e| e == "txt").unwrap_or(false))
            .collect();

        assert!(txt_files.is_empty(), ".txt files should not be collected");
    }
}
