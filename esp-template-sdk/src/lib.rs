//! esp-template-sdk — the versioned template contract for esp-generate.

pub mod config;
pub mod contract;
pub mod plugin;
pub mod process;
pub mod sweep;
pub mod template;

pub use process::{Facts, ProcessError, Renderer, is_safe_relative_path};
