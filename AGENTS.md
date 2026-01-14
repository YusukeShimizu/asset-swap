# AGENTS

This repository is a Rust project template.

## Development Principles

1. `spec.md` is the source of truth.
2. Before starting work, check `.codex/skills/` and follow any applicable Skill instructions.
3. Reproduce the development environment with Nix Flakes (`nix develop`).
4. Manage environment variables with direnv (`.envrc`).
5. Express representative operations first as integration tests (`tests/`).
6. Integration tests must not use mocks.
7. Keep implementations single-responsibility and keep terminology consistent.
8. Use `tracing` for logs and control verbosity with `RUST_LOG`.
9. When you update Markdown, run `textlint`.
10. Run the primary quality gate via `just ci`.

## Writing and Proofreading Rules

- Decide the logical structure first (for example: overview → details).
- Keep one meaning per sentence.
- Ensure subjects and predicates match.
- Keep terminology consistent.
- Keep numbers, units, and symbol formatting consistent.
- Distinguish facts from speculation.
- Remove unnecessary words and write concisely.
- If you update Mermaid, confirm it can be converted with `mmdc`.
