# PromptFS

Git-native, DB-less prompt manager. Prompts live in your Git repo as Markdown +
YAML frontmatter + Jinja2; PromptFS resolves, renders and canary-routes them over a
low-latency REST API, with the Studio UI embedded in a single Rust binary.

- **No database.** Git is the only source of truth.
- **Stateless.** Any instance comes up cold and serves correctly.
- **Fast.** < 5 ms p99 on the cached render path.

Status: early development, phase 1 (core engine). See [AGENTS.md](AGENTS.md) for the
architecture and the decisions already locked in.

## License

MIT
