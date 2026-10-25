# Fork features

This document describes user-visible changes relative to upstream
`rust-v0.164.0`. `MODIFICATIONS.md` is the authoritative file-level record.

## TaskDriven model routing

TaskDriven provides a versioned external configuration, a TUI editor in the
`/model` flow, phase-level model transitions, active-model reporting, and
resume persistence. Version 2 adds adaptive profiles and plain-language use
cases while retaining version 1 fixed mappings and a lossless migration path.
See [Using TaskDriven](taskdriven.md).

## Model-bound reviews

Guardian and MCP review paths retain the model context captured by the action
being reviewed. Late or parallel work cannot silently borrow another model's
review state. Late background network reviews use current review settings
while retaining the original execution and environment association.

## Plain transcripts

The interactive TUI can append prompts and visible Codex responses to a plain
text file:

```shell
codex --plain-transcript /absolute/path/to/transcript.txt
```

The same path may be set as `plain_transcript` in the user-owned
`$CODEX_HOME/config.toml`. Project configuration cannot redirect it. Recording
stops with a visible warning after an open or write failure.

Plain transcripts can contain source code, prompts, model responses, and local
paths. Protect them as sensitive user data and do not commit them publicly.

## Trusted unrestricted-mode configuration

`yolo = true` in user-owned configuration applies the same unrestricted policy
as the explicit CLI option. Project configuration is not trusted to enable it,
and explicit CLI sandbox or approval options take precedence.

## Interruption recovery

On supported Linux systems, the TUI records a private run marker. After a
reboot or unclean exit, the next start identifies the resumable native thread
and shows the exact `/resume` command. A clean exit removes its marker.

## Source-built update behavior

An available upstream version is treated as a rebuild signal rather than
self-replacing a locally modified binary, so an official binary cannot
silently remove fork features.

## Additional product fixes

- An alternate local daemon package does not follow the latest channel unless
  that choice was explicit.
- Thread and turn protocol responses retain the selected routing policy.
- TUI surfaces display the model actually used after fallback or transition.
- Plain transcript and draft recovery paths validate ownership and provenance
  before accepting sensitive settings.
