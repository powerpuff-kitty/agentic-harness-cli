# Canonical evidence conformance

This is test-only integration for powerpuff-kitty/agentic-harness#198. It does not
change the runtime catalog pin, dependencies, command behavior or release status.

`tests/fixtures/evidence-v1.json` is copied byte-for-byte from canonical revision
`300a021e30db1fcb6170ffa9fcec996ec4a91f06`. `conformance-source.json` records its source
path and SHA-256. The fixture has an exact-byte Git attribute for Windows checkout.
The existing source-clone helper preserves LF bytes for the pinned audit schema.

Run after the usual locked source synchronization:

```sh
cargo test --locked --lib artifact::canonical_conformance
```

The ordinary `cargo test --locked --all-targets` CI invocation includes the tests.
They call the existing `strict_json::decode` and `artifact::validate` implementation
on all 48 canonical cases, check both acceptance and rejection stage, verify fixture
and pinned-schema digests, and demonstrate that an altered expectation is detected.
No substitute Rust validator or test-only acceptance policy is introduced.

## What this proves when the tests execute

The exact native library revision agrees with the exact corpus for the included
19 JSON transport and 29 Codebase Audit v2 shape cases. The corresponding Python
reference runner is in the canonical repository. Test logs/CI revisions are the
execution evidence; the presence of this source is not a passing-test claim.

## What it does not prove

A schema-valid report with unknown scores or incomplete scan evidence is not a
passed gate or completed project. This integration is not an installed-command
probe: command readers, actual gate thresholds, source freshness, producer trust,
Jev outcomes, host loading and release readiness require separate tests. Numeric
spelling/large-integer precision, fingerprint equality, all-consumer size/depth
limits and other evidence families also remain outside this first corpus.

The copied fixtures retain canonical ownership and MIT attribution through this
repository's existing MIT notice. Update the canonical corpus first, then review
and copy its exact bytes and provenance. Never change expectations merely to make
a failing consumer green. Broader conformance remains open under canonical #198;
command documentation and tested release combinations remain canonical #197.
