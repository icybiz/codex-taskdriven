# Verification results

The implementation commit `2dd04bae596531603a783da90b5cdd51bf07cbeb`
was verified on Linux x86_64 before public-author metadata was rewritten. The
public implementation commits retain the same source tree.

## Release gates

- `cargo fmt --check` passed.
- The locked dependency graph resolved from the official 0.164.0 lockfile; the
  derivative added only the internal `codex-taskdriven` workspace package.
- The focused TaskDriven suite passed 20/20 on the isolated build host and
  again after import on the deployment host.
- Runtime smoke tests passed for the version, `--yolo --version`, plain
  transcripts, the code-mode host CLI, and source-build self-update refusal.
- `git diff --check` passed and the release manifest, source patch, binaries,
  logs, and imported artifacts were hash-verified.
- Controlled native resume rollouts retained thread, working directory,
  routing state, and Fast-off state on real long-running sessions.

These are focused release gates, not every upstream test on every platform.
The verified build targeted x86_64 Linux; no ARM artifact is claimed. Binary
publication remains subject to the separate gate in
[license compliance](license-compliance.md).
