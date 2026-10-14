# License compliance

This repository is a derivative of OpenAI Codex CLI and is distributed under
the unmodified Apache License 2.0 found in the top-level `LICENSE` file.

## Source publication requirements

The publication process enforces the following rules:

1. Keep the upstream `LICENSE` file unchanged and at the top level.
2. Retain the complete upstream `NOTICE` content and append derivative-work
   attribution without changing the license terms.
3. Retain all copyright, patent, trademark, attribution, and third-party notice
   files that apply to the distributed source.
4. Identify changed files in `MODIFICATIONS.md` and retain the Git history that
   records each change.
5. Describe the project as an unofficial derivative and do not imply OpenAI
   sponsorship or endorsement.

The requirements come from
[Apache License 2.0, section 4](https://www.apache.org/licenses/LICENSE-2.0)
and the
[Apache licensing and distribution FAQ](https://www.apache.org/foundation/license-faq.html).

## Current dependency result

Relative to upstream commit
`979011409de0a60b52f179721948e65531d26144`, the fork adds one internal Rust
workspace package, `codex-taskdriven`. It uses dependencies already present in
the upstream workspace and adds no external package to `Cargo.lock`. No
upstream attribution or license line was removed, and the derivative change set
adds no binary file.

The upstream Rust workspace already defines its accepted SPDX license set in
`codex-rs/deny.toml`. That policy remains part of this source tree.

## Binary release gate

Source publication and binary redistribution have different practical audit
scope. A binary release remains blocked until its exact archive passes all of
these checks:

- generate an SBOM or equivalent dependency inventory from the release build;
- collect the license texts and mandatory notices for all linked and bundled
  components;
- include the top-level Apache `LICENSE`, `NOTICE`, and modification notice;
- retain the native voice-library notices, source manifest, and corresponding
  license texts whenever those libraries are packaged;
- verify any platform runtime redistribution terms separately;
- record source commit, build inputs, artifact hashes, and target platform; and
- inspect the final archive rather than relying only on the source tree.

Until that gate succeeds, this repository publishes source only.

This file documents the project's compliance controls. It is not a substitute
for legal advice where a distributor needs a formal legal opinion.
