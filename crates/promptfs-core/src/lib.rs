//! # promptfs-core
//!
//! Everything that turns a prompt *file* into the string sent to a model, and nothing else.
//! The server, the `pyo3` wheel and the wasm Studio preview link this same code, which is what
//! makes "what Studio shows is what your app sends" structural instead of tested.
//!
//! Consequences, all load-bearing (`AGENTS.md`, invariants 3, 6 and 8):
//!
//! - **No I/O.** Reading a bundle, a snapshot or a Git blob belongs to the caller.
//! - **No async runtime, no blocking.** Every function here is synchronous and pure.
//! - **No `git2`.** ~3 MB plus a system dependency inside a wheel, for code that never opens a
//!   repository.
//! - **The minijinja `Environment` is built here, once**, autoescape explicitly off.

pub mod compile;
pub mod error;
pub mod meta;
pub mod parse;

#[cfg(test)]
mod fixtures;

pub use compile::CompiledPrompt;
pub use error::PromptError;
pub use meta::PromptMeta;
pub use parse::split_frontmatter;
