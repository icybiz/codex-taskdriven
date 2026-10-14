# Verification results

The implementation tree was verified on Linux x86_64 before publication
preparation. The verified tree is
`5588fff754fabf54203279cee0272080ed519950`; rewriting local author metadata for
the public branch did not change that tree.

## Release gates

| Area | Result |
| --- | ---: |
| TUI focused release gate | 57/57 passed |
| Core focused release gate | 332/332 passed |
| Routing protocol | 4/4 passed |
| App server | 137/137 passed |
| Guardian | 47/47 passed |
| CLI | 489/489 passed |
| Code mode host | 76/76 passed |

Formatting, quality checks, packaging checks, and a local smoke test also
passed. Focused checks separately covered TaskDriven configuration, work-phase
routing, captured model context, resume behavior, Guardian score generations,
plain transcripts, hardened settings, startup draft recovery, daemon package
selection, and late background network reviews.

These counts describe the release gates used for this derivative, not every
upstream test on every supported platform. The verified artifact targeted
x86_64 Linux. No ARM artifact is claimed by this repository.

Publication preparation subsequently changed only documentation, attribution,
and four user-facing message strings with their matching assertion. Those
changes require a focused TUI check before a tagged source release. Binary
publication remains subject to the separate license gate in
[license compliance](license-compliance.md).
