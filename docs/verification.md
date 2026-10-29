# Verification results

The initial implementation commit `2dd04bae596531603a783da90b5cdd51bf07cbeb`
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

## Maintenance update: 0.164.0-aicoder2.11

Published fixes:

- `14272272604bc2891fc83e3ed74efc8cf1f0af75`: model-neutral phase guidance
  and focused routing regressions.
- `11ac2a6605303ca2122f0f542edcfe8c8705dc02`: current MCP approval-metadata
  API in the captured-model regression test.

The source code matches the tested hardened revision
`48a97f95216c68c5730a6e0401f656146a5ef63b`; publication documentation and
public-author metadata differ. The two focused routing tests passed, and the
x86_64 release build completed with zero compiler errors. Its existing
17-warning set was unchanged.

A resumed thread successfully selected the configured implementation route,
then completed eleven regular phase transitions without the stale routing
conflict. A second long-running thread retained its history and phase mappings
after the update. Fast remained off in both sessions.

This resolves [issue #1](https://github.com/icybiz/codex-taskdriven/issues/1).
Binary redistribution remains subject to the separate license gate above.

## Maintenance update: 0.164.0-aicoder2.12 (2026-10-29)

The TaskDriven source fix separates per-thread activation from shared route
storage. It removes model-picker toggle writes to the shared activation flag,
keeps the phase tool available to explicitly active TaskDriven threads even
when a legacy session writes `enabled = false`, and respects fixed-mode
selection at startup. Profile and route edits remain shared. Feature,
Guardian, and fixed-mode gates are unchanged.

Verified on Linux x86_64 using the pinned toolchain, without paid model calls:

- Core registry regressions: 2 passed, covering concurrent fixed/TaskDriven
  sessions, version 1/version 2 routes, and feature/Guardian gates.
- Complete TaskDriven package tests: 21 passed.
- Focused TaskDriven TUI tests: 12 passed, including three sessions sharing
  one home, startup precedence, unchanged shared files, profile edits, and
  disabling routing even when the shared file is malformed.
- Formatting and whitespace checks passed. Runtime source files match the
  tested implementation; publication-only documentation and author metadata
  are separate.

The published source fix is
[`cfd122ec303d92ccf3d01dd1b0d86a06931ffc6a`](https://github.com/icybiz/codex-taskdriven/commit/cfd122ec303d92ccf3d01dd1b0d86a06931ffc6a).
Its runtime code matches the internally tested and built revision
`4b51f6da4d23337f3c277b9ef86216fc2e704c39`.

The corresponding hardened x86_64 release was rebuilt in 4,977 seconds with
zero compiler errors and no OOM events. The previous 17-warning set was
unchanged. Both rebuilt binaries were checksum-verified after import, then
passed local package smoke tests for the CLI/version, plain transcripts,
code-mode host, and centrally managed self-update refusal. The release
manifest records the 2 core, 21 TaskDriven, and 12 TUI test cases and the
SHA-256 hashes of their logs.

The locally verified packaged Codex SHA-256 is
`435ff8b12b22840026598c9d11ac1fa9ccd3e51f4db9ee050c45652e35af591f`.
This resolves [issue #2](https://github.com/icybiz/codex-taskdriven/issues/2).

This is not a claim that existing running installations have been updated.
Older running binaries retain the old behavior until they are upgraded; no
session was restarted as part of this release build. No ARM build or binary
redistribution is claimed here. Binary publication remains subject to the
separate license gate above.
