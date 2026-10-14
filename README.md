# Codex TaskDriven

This is an unofficial community-maintained derivative of
[OpenAI Codex CLI](https://github.com/openai/codex). It adds configurable
task-to-model routing and a set of related resilience and safety improvements.
It is not affiliated with or endorsed by OpenAI.

The current branch is based on upstream tag `rust-v0.161.0`, commit
`979011409de0a60b52f179721948e65531d26144`. The upstream Apache-2.0 license,
copyright notices, and third-party notices are retained. See
[MODIFICATIONS.md](MODIFICATIONS.md) and
[license compliance](docs/license-compliance.md) for the exact derivative-work
record.

---

## What this fork adds

- **TaskDriven routing:** route writing, investigation, implementation,
  verification, or custom tasks to independently selected models and reasoning
  efforts.
- **Live editing:** edit tasks and their model assignments from the `/model`
  picker. Available models come from the live model catalog, so future model
  versions do not require hard-coding into the picker.
- **Correct model context:** preserve the active model through resume,
  Guardian review, MCP review, status display, and step-level transitions.
- **Resilience:** recover interrupted native sessions and optionally write a
  plain-text transcript to a user-controlled path.
- **Safety hardening:** do not allow project-level configuration to silently
  enable unrestricted execution or redirect plain transcripts.
- **Product fixes:** retain daemon package-channel selection and use current
  review settings for late background network reviews.

See [Using TaskDriven](docs/taskdriven.md) and the
[complete fork feature notes](docs/fork-features.md). The tested release gates
are recorded in [verification results](docs/verification.md).

## Building this fork

The OpenAI installers and package-manager commands further down this README
install the official upstream release, not this fork. To build this source tree:

```shell
cd codex-rs
cargo build --release --bin codex
./target/release/codex --version
```

No binary release is published from this repository until its complete
third-party license bundle and release manifest pass the checks described in
[license compliance](docs/license-compliance.md).

## Quickstart

### Installing and running Codex CLI

Run the following on Mac or Linux to install Codex CLI:

```shell
curl -fsSL https://chatgpt.com/codex/install.sh | sh
```

Run the following on Windows to install Codex CLI:

```shell
powershell -ExecutionPolicy ByPass -c "irm https://chatgpt.com/codex/install.ps1 | iex"
```

The standalone installers download from `https://releases.openai.com/codex` by default and fall back to GitHub Releases if a metadata or asset download is unavailable. To force GitHub Releases, set `CODEX_INSTALLER_USE_RELEASES_OPENAI_COM` to `false` (`0` and `no` are also accepted):

```shell
curl -fsSL https://chatgpt.com/codex/install.sh | CODEX_INSTALLER_USE_RELEASES_OPENAI_COM=false sh
```

```powershell
$env:CODEX_INSTALLER_USE_RELEASES_OPENAI_COM='false'; irm https://chatgpt.com/codex/install.ps1 | iex
```

Codex CLI can also be installed via the following package managers:

```shell
# Install using npm
npm install -g @openai/codex
```

```shell
# Install using Homebrew
brew install --cask codex
```

Then simply run `codex` to get started.

<details>
<summary>You can also go to the <a href="https://github.com/openai/codex/releases/latest">latest GitHub Release</a> and download the appropriate binary for your platform.</summary>

Each GitHub Release contains many executables, but in practice, you likely want one of these:

- macOS
  - Apple Silicon/arm64: `codex-aarch64-apple-darwin.tar.gz`
  - x86_64 (older Mac hardware): `codex-x86_64-apple-darwin.tar.gz`
- Linux
  - x86_64: `codex-x86_64-unknown-linux-musl.tar.gz`
  - arm64: `codex-aarch64-unknown-linux-musl.tar.gz`

Each archive contains a single entry with the platform baked into the name (e.g., `codex-x86_64-unknown-linux-musl`), so you likely want to rename it to `codex` after extracting it.

</details>

### Using Codex with your ChatGPT plan

Run `codex` and select **Sign in with ChatGPT**. We recommend signing into your ChatGPT account to use Codex as part of your Plus, Pro, Business, Edu, or Enterprise plan. [Learn more about what's included in your ChatGPT plan](https://help.openai.com/en/articles/11369540-codex-in-chatgpt).

You can also use Codex with an API key, but this requires [additional setup](https://developers.openai.com/codex/auth#sign-in-with-an-api-key).

## Docs

- [**Codex Documentation**](https://developers.openai.com/codex)
- [**Contributing**](./docs/contributing.md)
- [**Installing & building**](./docs/install.md)
- [**Open source fund**](./docs/open-source-fund.md)

This repository is licensed under the [Apache-2.0 License](LICENSE).
