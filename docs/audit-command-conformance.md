# Audit command-reader conformance

The `gate` and `compare` command paths decode the original audit bytes through
`strict_json::decode` before existing artifact validation. Duplicate keys are
rejected at every object depth, including escaped aliases and unknown extension
fields. The existing legacy/v2 shape rules, score thresholds and result versions
are unchanged. A failed policy gate still exits 1; malformed or ambiguous input
exits 2 without a gate/comparison result on stdout.

The regression corpus is `tests/fixtures/audit-command-v1.json`: 30 synthetic input
files and 87 command invocations. It distinguishes 33 duplicate-key rejections,
27 malformed/non-finite rejections, and 27 preservation checks for finite values,
null measurements, fractional thresholds, incomplete coverage and legacy audits.

`tests/audit_commands.rs` is discovered by normal `cargo test --all-targets`. It
runs copied `ah` executables outside the checkout with isolated temporary homes.
It checks both compare operands and retains normal gate/compare semantics rather
than redefining a structurally valid report as project completion.

The Python probe can exercise an independently built/downloaded executable:

```sh
python3 scripts/test_audit_command_probe.py
python3 scripts/check-audit-commands.py /absolute/path/to/ah --report audit-command-report.json
```

The report records the executed binary SHA-256 and per-case outcomes. It refuses
to overwrite an existing report. All fixture files are temporary; no application
scripts, hosted model calls, credentials or user-project writes are needed.
The probe does execute the explicitly supplied binary: inspect its provenance
before use. Environment isolation is not an operating-system sandbox.

## Boundaries

This fix changes three JSON-decoder call sites only. It does not add new file-size
limits, protect against hostile filesystem races, authenticate report producers,
verify source freshness, change decision-provider behavior or authorize releases.
Other command families need separate conformance evidence. Passing these fixtures
is not proof of complete schema/numeric/limit parity.

A comparison may still summarize an incomplete report; it is descriptive, not a
completion gate. A gate without explicit thresholds retains its documented
artifact-validity/explicit-completeness scope. Neither behavior is silently changed.
