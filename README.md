# PromptFS

Git-native, DB-less prompt manager. Prompts live in your Git repo as Markdown +
YAML frontmatter + Jinja2; PromptFS resolves, renders and canary-routes them, with the
Studio UI embedded in a single Rust binary.

- **No database.** Git is the only source of truth.
- **Stateless.** Any instance comes up cold and serves correctly.
- **In-process.** The SDK embeds the same compiled engine the server runs, so resolving
  a prompt costs no network call and your render variables never leave your process.
- **Not a dependency of your uptime.** An SDK with a synced bundle keeps serving while
  PromptFS is down.
- **Fast.** < 5 ms p99 on the server's cached render path; microseconds in-process.

Status: early development, phase 1 (core engine). See [AGENTS.md](AGENTS.md) for the
architecture and the decisions already locked in, and the
[phase 1 milestone](https://github.com/netsirius/promptfs/milestone/1) for what's in
progress.

## License

MIT
