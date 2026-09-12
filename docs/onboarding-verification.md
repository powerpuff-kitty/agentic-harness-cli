# Executed onboarding and public-source verification

Tracking: CLI #56; canonical public-scope/onboarding issues #82/#83.

## What runs

`sync-upstream.py` verifies exact clean source checkouts, executes the pinned canonical public-text validator, and compares the canonical quick-start fixture with `tests/fixtures/public-quick-start.json`. It also checks the CLI README's quick-start argument arrays. Consumers keep their documented target paths/order; both sets are explicitly reviewed in `scripts/onboarding.py`.

`verify-install.py` exercises the custom-prefix binary and source launcher outside the checkout. `verify-candidate.py` applies the same 17-probe suite to the copied/downloaded native candidate with the restricted runtime PATH and proxy environment. The existing Linux network-namespace invocation runs it with networking disabled. Existing workflow entrypoints are reused; no workflow YAML change is needed.

The checks cover:

- both documented init/validate/audit sequences and exact expected exits;
- binary and generated-project source identities matching the reviewed lock;
- null overall/testing/production measurements and explicit unexecuted test discovery;
- two upgrades preserving edited product truth, local notes and existing file bytes;
- unsupported doctor, ADR and filesystem-migration syntax returning structured exit-2 diagnostics without mutation;
- approved literal public identifiers in generated text and paths.

The fresh context fixture intentionally has no GitHub Actions workflow, so its audit returns findings with exit 1. That is a tested outcome, not a failed runner or a production-readiness assertion. New fixture behavior must be reviewed with the expected contract.

## Local verification

```sh
python3 -m unittest discover -s scripts -p 'test_onboarding.py'
./scripts/sync-upstream.sh
cargo build --locked --release --bin ah
python3 scripts/verify-install.py target/release/ah
python3 scripts/verify-candidate.py target/release/ah --report /tmp/onboarding-candidate.json
```

The 25 Python regression tests use a simulated CLI to validate failure handling. They are not evidence that a real binary or model passed. Real candidate reports include the executed probes, fixture SHA-256, embedded source identities and binary hash; use the packaged provenance for the exact CLI source commit. A failure prevents a successful candidate report. Candidate evidence must be regenerated after source/pin changes.

## Safety and scope

Markdown is parsed only for comparison. Only fixed reviewed argument arrays execute, with `shell=False`, in disposable synthetic directories against an explicitly supplied trusted binary. This is not the general approved-project check runner planned in #55. Shell-free invocation is not a sandbox; network and process isolation depend on the calling environment.

The public-name check is intentionally narrow. It does not detect every disclosure, binary/image content, Git history, old PR diffs or cached metadata. It does not include a private-name denylist; synthetic negative fixtures never store real internal identifiers. Source pins still require human review, and no release is authorized by a passing smoke suite.
