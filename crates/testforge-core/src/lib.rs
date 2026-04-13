//! # testforge-core
//!
//! Shared types, configuration, and error handling for the TestForge
//! ecosystem.  Every other crate in the workspace depends on this one.

pub mod config;
pub mod error;
pub mod models;

// Re-export the most commonly used items at the crate root.
pub use config::TestForgeConfig;
pub use error::{Result, TestForgeError};
pub use models::{CodeSymbol, IndexStats, IndexedFile, Language, SearchResult, SymbolKind};