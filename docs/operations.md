# Binary installation and recovery

Use a candidate whose [release gates](release-checklist.md) passed for its exact source and upstream pins. Supported assets are `ah-linux-x86_64`, `ah-macos-x86_64`, `ah-macos-arm64` and `ah-windows-x86_64.exe`. The source installer supports macOS and Linux; the complete binary bundles also support Windows. No Rust, Node, Python or network access is needed to run an installed binary.

Download the platform ZIP and its `.zip.sha256` from the same reviewed GitHub Actions run or published release. Verify the ZIP before extraction. For additional notice/provenance checks from a source checkout, use `python3 scripts/package-candidate.py PATH.zip --verify-archive --require-authored-licenses`. Keep all extracted notice and provenance files with the binary. Checksums detect changed bytes relative to their sidecars; obtain both through the trusted repository page.

## macOS and Linux

The example uses the Apple Silicon asset. Substitute the asset name for Intel macOS or Linux and a new version directory for each update. Run these commands in the download directory:

```sh
set -e
shasum -a 256 -c ah-macos-arm64.zip.sha256
test ! -e "$HOME/.local/lib/agentic-harness/0.1.0"
mkdir -p "$HOME/.local/lib/agentic-harness/0.1.0" "$HOME/.local/bin"
unzip ah-macos-arm64.zip -d "$HOME/.local/lib/agentic-harness/0.1.0"
chmod +x "$HOME/.local/lib/agentic-harness/0.1.0/ah-macos-arm64"
"$HOME/.local/lib/agentic-harness/0.1.0/ah-macos-arm64" --version
ln -sfn "$HOME/.local/lib/agentic-harness/0.1.0/ah-macos-arm64" "$HOME/.local/bin/ah"
export PATH="$HOME/.local/bin:$PATH"
ah --help
```

Stop if checksum verification or `--version` fails. Do not overwrite an existing version directory with different bytes. If `~/.local/bin/ah` is a regular file from an earlier installation, preserve a copy before replacing it. Linux also supports `sha256sum -c` for the checksum step. Persist the PATH entry in your shell configuration if needed.

## Windows

In PowerShell, verify the archive in the download directory, then retain the complete bundle in a version directory:

```powershell
$expected = (Get-Content .\ah-windows-x86_64.exe.zip.sha256).Split(' ')[0]
$actual = (Get-FileHash .\ah-windows-x86_64.exe.zip -Algorithm SHA256).Hash
if ($actual -ne $expected) { throw 'Archive checksum mismatch' }
$installDir = Join-Path $env:LOCALAPPDATA 'AgenticHarness\0.1.0'
if (Test-Path $installDir) { throw 'Choose a new version directory' }
Expand-Archive .\ah-windows-x86_64.exe.zip -DestinationPath $installDir
Copy-Item (Join-Path $installDir 'ah-windows-x86_64.exe') (Join-Path $installDir 'ah.exe')
& (Join-Path $installDir 'ah.exe') --version
if ($LASTEXITCODE -ne 0) { throw 'Candidate verification failed' }
$env:Path = "$installDir;$env:Path"
ah --help
```

Add the version directory to your user PATH through Windows environment settings after verification. The original asset name is retained for checksum/provenance verification; `ah.exe` is the command alias. Keep previous version directories for rollback.

## Project updates and rollback

Before `ah upgrade`, commit or copy the project, including hidden `.agentic` and `.agents` directories. Run `ah validate PROJECT`, save the upgrade JSON report and review `conflicts` and `preserved`. Upgrades retain custom content; they do not reconcile conflicts automatically. Validate again afterward. Legacy layouts require the [explicit migration procedure](cli-contracts.md#project-upgrades-and-recovery).

To roll back the CLI, restore the previous symlink/file on macOS/Linux or restore the previous version directory in Windows PATH. Verify its recorded checksum and `--version` before use. To roll back project changes, restore the pre-upgrade project backup, then validate it. A binary rollback alone does not restore project files. For the first installation, uninstall the CLI and restore the backup rather than assuming an older release exists.

Uninstall by removing only the installed `ah` link/file and the selected version directory; remove its PATH entry if no longer used. The source installer places the command at `PREFIX/bin/COMMAND`. Uninstalling must leave user projects intact.

## Operating the CLI in automation

Pin the binary version/checksum and preserve the emitted `--version` source identities with CI evidence. Capture JSON stdout separately from diagnostics on stderr. Exit 1 is a finding or failed validation/policy; exit 2 is invalid input or execution failure. Architecture/design/agentic report generation may exit 0 with advisory findings or incomplete coverage. Use explicit gates on supported evidence rather than interpreting a successful inspection command as production approval.

When reporting a defect, include command arguments, version/provenance, platform, exit status and a minimal sanitized fixture. Do not attach repository secrets. See [SECURITY.md](../SECURITY.md) for private reporting.
