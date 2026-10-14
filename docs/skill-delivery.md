# Reviewed skill delivery

The CLI embeds the exact Skills revision from `upstream.lock.json`. Skill folders
live directly at that source root; README and installation guidance are excluded
from the embedded payload. Each copied skill includes its own license and local
resources, so a consumer does not need the original checkout to read its guides.

The delivery fixture covers sixteen procedures: the original audit/security/design
reviews, three financial validation methods, three onchain/contract/DeFi methods
odds-analysis, forecast-calibration and settlement-rule-review, plus four
content briefing/editing/repurposing and GEO measurement procedures. The domain folders add report-template assets and JSON
acceptance specifications alongside their reference guides. Those specifications
are copied as resources, not executed or treated as observed outcomes.

For a fresh financial-research context project:

```sh
ah init ./research --boilerplate base --skill market-data-validation --skill financial-model-review --skill backtest-validation
ah validate ./research
```

Upgrades preserve existing bytes and report conflicts. They do not automatically
replace older/customized instructions or references. Review differences explicitly;
restoring a missing resource does not certify preserved custom files as identical
to upstream. The target project continues to own its context and acceptance rules.

## Executable verification

`tests/fixtures/flagship-skill-delivery.json` records the exact reviewed Skills commit
and SHA-256 values for all 79 files across these sixteen folders. The probe is shared
by actual-output checks, source/custom-prefix installs and copied/downloaded
candidates. The executed binary needs no runtime source checkout or model. The
base context template's optional skills-index README is identified by its exact
reviewed hash, not mistaken for an installed skill; other unexpected files fail.

The 46 calls check source identity; single-skill and combined composition; actual
project validation; two upgrades preserving a customized guide and owner notes;
restoration of missing reference, asset-template and JSON-case resources; and
non-mutating rejection of an unknown skill. Project notices retain the exact MIT
text. Hash checks establish delivery identity, not financial reasoning quality.

Eleven regression tests use synthetic files and a mocked wrong-version response.
They are separate from real candidate execution. Candidate reports record source/
fixture identity, call outcomes, `model_execution: not-run` and false host-delivery
status. A failed probe prevents a successful candidate report. This internal
verification record does not change a public canonical execution schema.

Current Skills source uses complete folder copies, without collection bundle
manifests, a ZIP packager or automatic host configuration. Historical package
formats remain historical. See the [source installation guide](https://github.com/powerpuff-kitty/skills/blob/main/INSTALL.md).

Scope: owned synthetic projects, with no user-project install, execution of copied
skill helpers, broker/wallet access or live agent/model session. Source/candidate
delivery checks do not establish native host loading or task-outcome reliability.
Reproducible model evaluation remains tracked separately in
[skills #26](https://github.com/powerpuff-kitty/skills/issues/26).

The separate [managed lifecycle](skills-installation.md) adds explicit updates and rollback; the preservation checks above continue to cover project composition.
