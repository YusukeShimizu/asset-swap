---
name: rust-template-execplan
description: Create and maintain ExecPlans (design → implementation) following this repo's standard.
metadata:
  short-description: ExecPlan workflow
---

Use this skill when the task is complex.
Use this skill when requirements are ambiguous and benefit from a living plan.

## Source of truth

The ExecPlan standard for this repo is in:

- `.codex/skills/execplan/references/PLANS.md`

## Workflow

1. Read the full reference `PLANS.md` above (it is intentionally strict).
2. Create a single ExecPlan file as instructed (formatting rules matter).
3. Keep the ExecPlan as a living document: update Progress/Decisions/Discoveries as you implement.
4. Do not ask the user for “next steps” mid-execution; proceed milestone by milestone.

## Acceptance

An ExecPlan is done only when it enables a complete novice to reproduce an observable outcome.
The novice must be able to use only the repo and the plan.
