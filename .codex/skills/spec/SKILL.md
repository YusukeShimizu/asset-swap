---
name: rust-template-spec
description: Maintain this template's specification (`spec.md`) so code, tests, and CI stay aligned.
metadata:
  short-description: Work on spec.md
---

# `spec.md` 作成・更新ガイドライン

`spec.md` は、このテンプレートに含める要件と不変条件を定義する。
実装（Rust）・テスト・CI は、必ず `spec.md` と整合していなければならない。

本リポジトリの `spec.md` は、arXiv:2508.14511v2 に準拠し、Concept Specifications と
Synchronizations で構造化して記述する。

## Format rules

### Concept Specifications

- `concept <NAME>` を単位として記述する。
- 各 concept は次の順序で構成する（必須）。
  - `purpose`
  - `state`
  - `actions`
  - `operational principle`
- `actions` は「入力レコード `[...]`」「出力レコード `=> [...]`」を基本形とする。
- `operational principle` は代表シナリオ（テスト化しやすい形）で書く。

### Synchronizations

- `sync <NAME>` を単位として記述する。
- 形は `when { ... } (where { ... }) then { ... }` とする。
  - `where` は必要なときだけ書く。
- `when` の action と `then` の action は、Concept Specifications に存在しなければならない。

## Writing rules

- 用語は Rust 実装・Integration Test・`justfile` と一致させる。
- 代表的な品質ゲートは `just ci` に集約し、spec には同期（Sync）として表現する。
- 文章は textlint を前提に、一文を短く保つ（必要なら文を分割する）。

## Environment rules

- 環境変数は direnv（`.envrc`）で管理する。
  - リポジトリ内の `.envrc` は安全な設定のみを置く。
  - ローカル専用の上書きや秘密情報は `.envrc.local`（gitignore）に置く。

## Update checklist

- `spec.md` の変更に合わせて Integration Test（`tests/`）を更新する。
- `README.md`/`docs/` の開発手順も整合させる（手順が変わる場合のみ）。
- `nix develop -c just ci` が通る状態で終える。
- Markdown を更新したら `nix develop -c textlint <file...>` を実行する。
