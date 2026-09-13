# Flagship skill delivery

The CLI embeds the reviewed agents revision from `upstream.lock.json`. The first self-contained flagship procedures are `codebase-audit`, `security-review` and `design-system-compliance`. Their skill folders contain required local decision guides, report templates and a repository-specific `bundle.json` declaration. Compliance also contains an exact copy of the shared Design Intelligence boundaries. A copied skill does not need the original collection checkout to read these references.

For a fresh context project:

```sh
ah init ./review-project --boilerplate base --skill codebase-audit --skill security-review --skill design-system-compliance
ah validate ./review-project
```

Existing project upgrades preserve existing bytes and report conflicts; they do not automatically replace an older or customized SKILL.md or reference with the new upstream version. Review differences explicitly. Adding missing references is not proof that preserved custom procedures are identical to the current bundle. The target project continues to own its context and acceptance rules.

## Executable verification

`tests/fixtures/flagship-skill-delivery.json` records the exact reviewed agents commit and SHA-256 values for all 13 files in these three folders. The same probe is used by actual-output checks, source/custom-prefix installation and copied/downloaded candidates. It requires no runtime source checkout, external tool or model.

The 16 calls verify source identity; fresh single-skill and combined composition; actual project validation; two explicit upgrades preserving a customized guide and owner notes; restoration of a missing reference; and non-mutating rejection of an unknown skill. The existing project notice must retain the exact MIT text. Hash checks compare delivered source bytes, not semantic quality.

Nine probe regression tests use synthetic files and a mocked wrong-version process response. They are separate from real candidate execution. Candidate reports include the source/fixture identity, call outcomes and explicit `model_execution: not-run` and false host-delivery status. A failed probe prevents a successful candidate report. No public canonical execution schema is changed by this internal verification record.

The agents repository separately validates standalone ZIP dependency closure and real collection-package contents. Its `bundle.json`/`bundle-lock.json` format is not part of the Agent Skills standard. Standalone ZIP packaging is not a new CLI command; use the agents repository's documented packager for that path.

Scope: trusted synthetic projects only; no user-project installation, project command execution, live agent session or model-behavior evaluation. No workflow YAML or dependency change is required. Migration and verified-completion procedure depth remain under agents #25; actual model outcomes are tracked separately under agents #26.
