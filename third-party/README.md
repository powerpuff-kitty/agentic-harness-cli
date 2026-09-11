# Supplemental dependency licenses

Some published crate archives omit their repository-root license file. These
unmodified files fill that gap. `license-sources.json` identifies each exact crate
version, its `.cargo_vcs_info.json` commit, original URL and SHA-256. The collector
rejects changed pins or modified text. New dependency versions require review;
there is no runtime or packaging-time download of supplemental licenses.

`scripts/dependency-notices.py` preserves license, notice, copyright and author
files from every resolved Cargo package, including build and target-specific
packages. It also includes `COPYRIGHT-library.html` from the installed Rust
compiler distribution. This is attribution evidence, not an inferred license for
the CLI or its embedded authored sources, and not automatic legal approval.

The current authored-source license decision is tracked in
[#53](https://github.com/powerpuff-kitty/agentic-harness-cli/issues/53).
