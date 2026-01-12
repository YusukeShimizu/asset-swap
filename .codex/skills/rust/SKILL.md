---
name: rust-template-rust
description: Develop Rust code in this template repo (CLI + library). Favor robustness, integration tests, structured logging, and cargo fmt/clippy.
metadata:
  short-description: Rust workflow for this template
---

# Rust Template: Rust Development Skill

この Skill は、このリポジトリで Rust 実装（`src/` と `tests/`）を追加・変更する時に使う。

## Source of truth

- 仕様は `spec.md` を正とする。

## Workflow

1. `spec.md` を読み、追加・変更する振る舞いを明確にする。
2. 代表的な操作は `tests/` の Integration Test で先に表現する。
3. 実装は SRP を守り、責務ごとにモジュールを分割する。
4. ログは `tracing` を使い、`RUST_LOG` で詳細度を切り替える。
5. 仕上げに `just ci`（または同等の `cargo fmt` / `cargo clippy` / `cargo test`）を通す。

## Testing rules

- Integration Test は mock を使わない。
- 外部依存が必要な場合は、まず `spec.md` に前提条件を追記する。
