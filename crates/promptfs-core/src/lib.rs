//! # promptfs-core
//!
//! Everything that turns a prompt *file* into the string sent to a model, and nothing
//! else. This crate is linked into three very different processes — `promptfs-server`,
//! a customer's Python interpreter via the `pyo3` wheel, and the Studio preview
//! compiled to wasm — which is what makes "what Studio shows is what your app sends" a
//! structural property rather than a promise checked by tests.
//!
//! Consequences, all of them load-bearing (see `AGENTS.md`, invariants 3, 6 and 8):
//!
//! - **No I/O.** Reading a bundle, a snapshot or a Git blob belongs to the caller. The
//!   core takes bytes and returns values.
//! - **No async runtime, no blocking.** Every function here is synchronous and pure.
//! - **No `git2`.** `libgit2` is ~3 MB plus a system dependency inside a wheel, for code
//!   that never opens a repository.
//! - **The minijinja `Environment` is constructed here, once**, with autoescape
//!   explicitly off. No caller builds its own.
//!
//! Phase 1 fills this crate in, module by module, per `docs/phase-1.md`.

pub mod error;
pub mod meta;

pub use error::PromptError;
pub use meta::PromptMeta;
