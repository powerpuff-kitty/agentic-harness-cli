# Adapter build-input byte identity

The adapter installer preserves exact reviewed payload bytes. Its bridge test must require LF bytes on every platform, not accept platform-dependent line endings merely because both forms parse as text.

Windows candidate validation exposed inherited Git checkout conversion: `@AGENTS.md` acquired CRLF and differed from the pinned Git blob. New upstream clones now set repository-local `core.autocrlf=false` and `core.eol=lf` before any checkout. Global Git settings and existing user-authored target files are not changed.

`source_bytes.verify_adapter_bytes` compares the inventory, three context payloads and MIT notice directly against their pinned checkout's Git objects before build. A converted/modified existing input is refused with a preservation-oriented diagnostic; it is never silently rewritten or treated as equivalent. The verified HEAD identity remains enforced by the existing source checker. These are build-input checks, not runtime network dependencies.

Five real local-Git regression tests cover inherited conversion defaults, exact byte preservation, converted/modified/missing files, and refusal to replace an existing clone destination. They run through the existing source-sync entrypoint. No workflow YAML change or relaxation of the Rust payload tests is required.

Git documentation: `git clone --config` sets repository configuration before initial checkout; `core.autocrlf` controls automatic checkout conversion. See https://git-scm.com/docs/git-clone and https://git-scm.com/docs/git-config. Explicit file attributes or filters can still transform bytes; the byte comparison, not configuration alone, is the acceptance check. This limited payload check is not a whole-repository reproducible-build guarantee.
