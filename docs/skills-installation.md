# Managed skill installation

`ah skills` manages the embedded Skills snapshot separately from Harness project
composition. It does not initialize `.agentic`, modify provider settings, grant
permissions or fetch a newer GitHub revision. Update the CLI/source pin to obtain
newer content; `ah skills list` reports the embedded revision and domain membership.

```sh
ah skills list
ah skills install --target /path/to/project/.opencode/skills --domain engineering --skill geo-measurement --dry-run
ah skills install --target /path/to/project/.opencode/skills --domain engineering --skill geo-measurement
ah skills status --target /path/to/project/.opencode/skills
ah skills update --target /path/to/project/.opencode/skills --dry-run
ah skills update --target /path/to/project/.opencode/skills
ah skills rollback --target /path/to/project/.opencode/skills --backup BACKUP_ID --dry-run
ah skills rollback --target /path/to/project/.opencode/skills --backup BACKUP_ID
```

Use the effective global host skills directory as `--target` for a global install.
Target is always explicit; use canonical paths instead of symlink aliases.
Repeat `--skill` and `--domain` to combine selections, or choose `--all`. Presets
live in [the domain catalog](../catalog/skill-domains.json), overlap deliberately
and deduplicate. Skill sources remain flat. No domain grants permissions or runs
bundled helpers. Inspect other discovery locations for duplicate IDs before
installing, especially when mixing project and global copies.

Installation/update reads `.ah-skills/state.json` in the target. It tracks each
file hash and each skill's source revision. The existing `powerpuff-kitty/skills`
state collection ID remains stable across the GitHub rename to `ai-skills`. Byte-identical unmanaged copies may
be adopted. Differing unmanaged files and edited managed files are conflicts;
any conflict blocks the apply before skill/state writes. `--dry-run` reports
conflicts without applying them; a successful preview exit is not a ready verdict.
Unchanged managed content can update; missing resources are restored. Status
shows incoming/current content differences and missing/local/unmanaged states.
Updates without selectors use installed managed skills. Existing unrelated,
previously selected and obsolete upstream files are preserved; narrowing a domain
is not uninstall. No override flag silently discards custom edits.

Every mutation records a backup journal with prior bytes, state and expected
post-change hashes in `.ah-skills/backups/`. The returned backup ID selects a
rollback. Rollback preflights all current hashes and backup integrity; subsequent
edits cause refusal. Revert later operations first. A failed apply attempts to
restore only the bytes it wrote; interruption or concurrent modification can
require journal-based recovery. Individual file writes are atomic, not a
filesystem-wide transaction. Do not run concurrent lifecycle operations on one
target. Empty created directories and journals can remain after rollback.
Backups may contain local content; keep them private to the selected scope.

The older `ah init/upgrade --skill` path continues to preserve differing existing
project files. Use `ah skills` when explicit managed updates are desired; it
cannot infer ownership of differing older unmanaged copies.

## Native OpenCode verification

```sh
python3 scripts/opencode_skill_probe.py --opencode /absolute/path/to/opencode
```

This standard-library probe creates a disposable Git project and private persistent
OpenCode V2 server, copies complete skill folders, and compares registered IDs,
paths and Markdown bodies. Configuration plugins load asynchronously: bounded
polls reuse the same server. Starting a fresh `--standalone` process per poll can
repeat an initially empty inventory. The probe terminates its own server and
removes temporary content; it does not invoke models or modify an existing host.

`--source` chooses a source checkout; `--agents-export` optionally verifies an
existing generated profile/command export in the same disposable host. Registry
checks establish registered instruction content, not model adherence. See current
[OpenCode V2 skills documentation](https://opencode.ai/v2/docs/skills/).


Retained [native registry evidence](evidence/skills-lifecycle-2026-10-14/native-skills.json),
[isolated lifecycle exercise](evidence/skills-lifecycle-2026-10-14/lifecycle.json)
and [source identities](evidence/skills-lifecycle-2026-10-14/source-manifest.json)
record the 2026-10-14 check. The source consumer's CI validates metadata for all
62 skills and domain coverage; its real binary delivery fixture checks 172 exact
files across 30 representative procedures. These checks do not certify reasoning
quality or prove every host version.

## npm channel

Native release bundles remain the maintained CLI distribution. No npm launcher is
published. A future launcher should package or verify exact-version platform assets,
preserve the native binary's commands and avoid automatic host configuration. See
the [distribution assessment](https://github.com/powerpuff-kitty/ai-agents/blob/main/docs/DISTRIBUTION.md).
Skills already support direct Git discovery through external tooling; catalog npm
packages are not required for the managed lifecycle.
