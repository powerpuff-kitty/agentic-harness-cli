# CLI Architecture

The CLI consumes two pinned authoring sources at build time:

1. `agentic-harness` for templates, packs, policies, profiles, presets, and schemas.
2. `agentic-harness-agents` for skills.

`include_dir` embeds these resources into the native release binary. Generated projects therefore do not depend on GitHub or the authoring repositories at runtime.

`upstream.lock.json` records the exact revisions expected by CI/release builds. Updating a canonical source requires an explicit lock update and a CLI validation run.

The CLI owns mechanics, not product truth. If a deterministic check requires a new canonical rule, add the rule to `agentic-harness` first and then update the CLI implementation.
