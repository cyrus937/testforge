//! # TestForge Core
//!
//! Foundational types, configuration management, and error handling
//! shared across all TestForge crates.
//!
//! This crate provides:
//! - [`config::Config`] — Project configuration (parsed from `.testforge/config.toml`)
//! - [`models`] — Domain types: symbols, search results, code chunks
//! - [`error::TestForgeError`] — Unified error type for the entire project

pub mod config;
pub mod error;
pub mod models;

pub use config::Config;
pub use error::{Result, TestForgeError};
