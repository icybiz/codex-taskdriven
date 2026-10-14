# Fork features

This document describes the user-visible changes relative to upstream
`rust-v0.161.0`. `MODIFICATIONS.md` is the authoritative file-level record.

## TaskDriven model routing

TaskDriven provides a versioned external task configuration, a TUI editor in
the `/model` flow, step-level model transitions, active-model reporting, and
resume persistence. Model and reasoning choices are validated against the live
catalog. See [Using TaskDriven](taskdriven.md).

## Model-bound reviews

Guardian and MCP review paths retain the model context captured by the action
being reviewed. Late or parallel work cannot silently borrow a different
model's review state. Late background network reviews use the current review
settings while retaining the original execution and environment association.

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
paths. Protect them as sensitive user data and do not commit them to a public
repository.

## Trusted unrestricted-mode configuration

`yolo = true` in the user-owned configuration applies the same unrestricted
execution policy as the explicit CLI option. Project-level configuration is
not trusted to enable it, and explicit CLI sandbox or approval options take
precedence.

## Interruption recovery

On supported Linux systems, the TUI records a private run marker. After a
reboot or unclean process exit, the next start identifies the resumable native
thread and shows the exact `/resume` command. A clean exit removes its marker.

## Source-built update behavior

The fork treats an available upstream version as a rebuild signal rather than
self-replacing a locally modified binary. This prevents an official upstream
binary from silently removing the fork's features.

## Additional product fixes

- An alternate local daemon package no longer starts following the latest
  channel unless that choice was explicit.
- Thread and turn protocol responses retain the selected routing policy.
- TUI surfaces display the model actually used after policy fallback or a
  TaskDriven transition.
- Plain transcript and draft recovery paths validate ownership and provenance
  before accepting sensitive settings.
