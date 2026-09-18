# Computed-import coverage regression

The canonical #86 reading-list adoption probe exposed a false-completeness result: `import(name)` produced no edge or coverage gap, so architecture analysis could report `complete: true` and `passed: true` despite an unresolved dynamic dependency.

The syntax visitor now records the original source line as unsupported coverage for computed imports. It still resolves literal dynamic imports, and does not invent a dependency target or classify asynchronous edges as synchronous cycles. Existing graph consumers carry the gap into `unresolved_local_imports`, `complete: false` and `passed: false`.

Validation includes identifier and interpolated-template targets, a literal control case, an executable CLI regression, and the canonical runnable fixture's adoption probe. This is a conservative coverage correction within the existing evidence shape; it does not add computed-target resolution or change source pins.
