# License compliance

This repository is a derivative of OpenAI Codex CLI and is distributed under
the unmodified Apache License 2.0 in the top-level `LICENSE` file.

## Source publication requirements

The publication process enforces these rules:

1. Keep the upstream `LICENSE` file unchanged and at the top level.
2. Retain the complete upstream `NOTICE` content and append derivative-work
   attribution without changing the license terms.
3. Retain applicable copyright, patent, trademark, attribution, and
   third-party notice files.
4. Identify changed files in `MODIFICATIONS.md` and retain the Git history
   recording each change.
5. Describe the project as an unofficial derivative and do not imply OpenAI
   sponsorship or endorsement.

These requirements follow
[Apache License 2.0, section 4](https://www.apache.org/licenses/LICENSE-2.0)
and the
[Apache licensing and distribution FAQ](https://www.apache.org/foundation/license-faq.html).

## Current dependency result

Relative to upstream commit
`6aaac694803004a63c6f393d153c96d22521a6b9`, the fork adds the internal Rust
workspace package `codex-taskdriven`. It uses dependencies already present in
the upstream workspace and adds no external package to `Cargo.lock`. No
upstream attribution or license line was removed, and the derivative change
set adds no binary file.

The upstream Rust workspace's accepted SPDX policy remains in
`codex-rs/deny.toml`.

## Binary release gate

Source publication and binary redistribution have different audit scope. A
binary release remains blocked until its exact archive passes all of these
checks:

- generate an SBOM or equivalent dependency inventory from the release build;
- collect license texts and mandatory notices for all linked and bundled
  components;
- include the top-level Apache `LICENSE`, `NOTICE`, and modification notice;
- retain native voice-library notices, the source manifest, and corresponding
  license texts whenever those libraries are packaged;
- verify platform runtime redistribution terms separately;
- record source commit, build inputs, artifact hashes, and target platform;
  and
- inspect the final archive rather than relying only on the source tree.

Until that gate succeeds, this repository publishes source only.

This document records project controls; it is not a substitute for formal
legal advice.
