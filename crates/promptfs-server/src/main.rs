//! PromptFS server.
//!
//! Owns everything the core is not allowed to touch: `git2` bare-repo reads, the
//! `tokio` runtime, the `axum` REST surface, the `moka` cache, webhooks and SSE. It
//! reads bytes out of Git and hands them to `promptfs-core`, which does the rendering.
//!
//! The same binary also hosts `promptfs pull`, which is an HTTP *client* of another
//! instance rather than a second server.
//!
//! Phase 1 gives it a `POST /v1/prompts/render` endpoint over a bare fixture repo;
//! nothing is wired up yet.

fn main() {
    println!("promptfs-server: no routes yet — see docs/phase-1.md tasks 7 and 8.");
}
