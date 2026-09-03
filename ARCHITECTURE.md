# CLI Architecture

The CLI consumes two pinned authoring sources at build time:

1. `agentic-harness` for complete root boilerplates, `modules/`, presets, and schemas.
2. `agentic-harness-agents` for skills and agent procedures.

`include_dir` embeds the selected canonical content into the native release binary. Generated projects therefore do not depend on GitHub or the authoring repositories at runtime.

`upstream.lock.json` records the exact revisions expected by CI and release builds. Updating either authoring source requires an explicit lock update and a full CLI validation run.

The composition engine copies the selected materialized boilerplate directly, then installs selected packs, policies, and skills. Canonical metadata such as `boilerplate.json` is build-time source information and is not leaked into generated projects.

The CLI owns mechanics, not product truth. If a deterministic check requires a new canonical rule, add that rule to `agentic-harness` first, adapt agent procedures if needed, then update the CLI implementation.
