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
2. `proto/` を変更する場合は、このリポジトリの Protobuf 制約（後述）を満たす。
3. 代表的な操作は `tests/` の Integration Test で先に表現する。
4. 実装は SRP を守り、責務ごとにモジュールを分割する。
5. ログは `tracing` を使い、`RUST_LOG` で詳細度を切り替える。
6. 仕上げに `just ci`（または同等の `cargo fmt` / `cargo clippy` / `cargo test`）を通す。

## Testing rules

- Integration Test は mock を使わない。
- 外部依存が必要な場合は、まず `spec.md` に前提条件を追記する。

## Protobuf rules

`proto/` を追加・変更する場合は、次を満たす。

- Google AIP: https://google.aip.dev/ に則り、REST は `google.api.http` で厳密に設計する。
- 可能な限り `buf.validate` / Protovalidate で制約を付与する。
- service / rpc / message / field のコメントに利用方法と代表的なエラーパターンを明記する。
- `just proto_fmt` と `just proto_lint` を通す。
