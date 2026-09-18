# Experimental governance evidence evaluation

`governance_verdict::evaluate` is a pure Rust library implementing the semantic rejection rules under CLI #55 and canonical #85. It consumes existing v1 governance artifacts; it adds no CLI command, filesystem access, network lookup, execution approval or producer authentication.

The caller supplies independently obtained before/after source and policy digests, exact scope, required rule/capability pairs, current time and maximum age. Any change between snapshots rejects the set. Report scope compares as a set, without treating a wider or narrower scope as equivalent. Observation age is inclusive at the policy limit; future observations and invalid numeric timestamps fail.

Trust is explicitly injected as exact report-byte digests bound to producer IDs and versions. Constructing a `TrustedReport` asserts that the caller has established trust; it does not establish it. Report fields cannot grant themselves trust. The eventual consumer must obtain this context from an approved mechanism outside the imported report, such as authenticated transport/signatures or explicit caller review. A checksum alone is not authentication. No default producer allowlist is introduced.

References are opaque names mapped to caller-resolved immutable bytes and expected digests. Every claimed reference must resolve in that supplied map and match its expected bytes; duplicate names or reference lists fail. The evaluator never follows a URL or opens a repository path. This verifies identity/existence within the caller's supplied snapshot, not whether reference contents substantiate a claim. The producer trust decision must cover that assertion.

Every report must be structurally valid, trusted and current, including reports whose claims are not required. Duplicate/conflicting rule/capability claims across or within reports fail rather than letting order choose a winner. Verified claims require a mechanism and reference; delivered/enforced claims additionally require a versioned host. Required controls must match both rule and capability with status `verified`. Declared, delivered, checked and enforced never substitute for each other.

Limits: 64 reports, 1 MiB per report, 8 MiB total report bytes; 64 resolved references and 8 MiB total reference bytes; canonical claim/control/scope/text bounds. Strict JSON parsing rejects duplicate keys, including escaped aliases. Typed rejection variants do not include untrusted text.

A successful verdict means only that required governance assertions satisfy the supplied trust/context conditions. Empty requirements can be satisfied by an empty set. `completion_verified` is always false: command outcomes, producer authentication, actual host enforcement, trusted policy loading, clock trust and filesystem freshness acquisition remain integration responsibilities. Changed inputs reverted between snapshots remain undetectable.

## Verification

`cargo test --locked --test governance_verdict` exercises synthetic reports and reference bytes only. It covers trust binding, current inputs, exact scope/capability, age boundaries, nonverified claims, duplicate/conflicting evidence, missing/changed references, required host/mechanism data, malformed JSON and resource bounds. No native host/model session or signed-evidence trial is claimed.

## Integration sequence

1. Select the owner-controlled report trust mechanism; never read trust grants from imported artifacts.
2. Load and validate current project policy, capture before identity, resolve bounded references, validate reports and capture after identity.
3. Combine this scoped verdict with authenticated check-run validation and RunLedger outcomes in a distinct versioned completion artifact. Reject stale post-invocation reports and keep existing audit/gate semantics unchanged.
4. Test the actual consuming command against seeded source/config drift, malformed evidence and trust failures; then run cross-platform candidate CI.

Rollback removes the public library module and its tests/docs. Existing schemas, source pins, commands and executor behavior are unchanged.

Local verification on 2026-09-18 (macOS x86_64, Rust 1.94.1): all 190 Rust tests passed, including 16 new evaluator tests; Clippy with warnings denied and formatting passed. Canonical catalog validation passed. No remote CI or signed-producer evidence was observed for this slice.

The first consuming command is now `checks complete`, described in [caller-approved completion](check-completion.md). ADR-010 selects exact caller-approved report digests through a reviewed manifest. The pure evaluator remains independent of I/O/authentication; the command acquires current snapshots and combines it with run validation. Signed producers remain deferred.
