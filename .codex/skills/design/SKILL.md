---
name: rust-template-design
description: Write or update design docs (constraints, concepts, flows) and keep terminology consistent with Rust code.
metadata:
  short-description: Design doc authoring
---

Use this skill when you are authoring or updating design documentation (example: `design.md`).

## Model (lightweight WYSIWID)

- A **Concept** is an independent capability (example: “CLI parsing”, “Config loading”, “HTTP client”).
- A **Flow** connects Concepts into a story.
  Example: “Run a command”, “Load config and execute”.

## Writing rules (must follow)

- Start with invariants (“Security & Architectural Constraints”).
  Use RFC 2119 language (MUST / MUST NOT).
  Include a brief rationale.
- Keep the structure flat and scannable.
  Prefer short lists, signatures, and data-shape bullets.
- Use a ubiquitous language.
  Keep terminology consistent across docs, JSON fields, CLI flags, and Rust identifiers.
- Define each term of art.
  Assume the reader is new to the repo.

## Suggested section order (for new design docs)

1. Security & Architectural Constraints
2. Concepts
3. Flows

If a flow is complex, add a Mermaid sequence diagram or a state chart.

## Validation

- Ensure the doc maps to concrete modules/binaries in the repo.
  When the doc is a plan, map it to paths in the ExecPlan.
- If the doc implies behavior changes, reflect them in the ExecPlan.
  Update the Rust implementation too (see `$rust-template-rust`).
