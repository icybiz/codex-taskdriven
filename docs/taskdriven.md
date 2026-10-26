# Using TaskDriven

TaskDriven lets one Codex thread use different models and reasoning settings
for different phases of work. A phase is a route, not a permission profile;
routing never relaxes sandbox, approval, or tool rules.

## Enable the feature

Add these flags to the user-owned `$CODEX_HOME/config.toml`:

```toml
[features]
step_model_switching = true
autonomous_work_phase = true
```

Start Codex, open `/model`, choose **TaskDriven**, and enable it. New
installations use the adaptive version 2 format. Existing version 1 files keep
their exact fixed mappings until **Switch to adaptive Standard…** is selected;
the migration first creates `taskdriven.toml.v1.bak` without overwriting a
different existing backup.

## Adaptive work styles

| Work style | Emphasis |
| --- | --- |
| Standard | Balanced automatic routing for most work. |
| Economy | Prefer the lightest sufficient route and escalate on evidence. |
| Responsive | Prefer short interactive steps and low initial latency. |
| Quality focus | Escalate earlier and spend more effort on verification. |

The work style combines with one of eight use cases: Automatic, Find and fix a
bug, Build something new, Build a website or web app, Analyze data or automate
work, Build an interactive app or game, Improve an existing project, or
Research and prepare a decision.

Adaptive routing classifies recognized live-catalog models into efficient,
balanced, and frontier roles, then chooses the least expensive role sufficient
for the current phase and policy. Unknown future model names remain available
for explicit pins but are not guessed into an automatic role. Any phase can be
pinned to a specific catalog model and supported reasoning effort.

## Default phases

| Phase | Intended work |
| --- | --- |
| Writing | Drafting, rewriting, documentation, explanations, and user-facing prose. |
| Investigation | Reading code and documents, research, diagnosis, bug discovery, and design. This replaces the earlier informal name “Reading.” |
| Implementation | Changing code, fixing bugs, refactoring, and authoring tests. |
| Verification | Building, running tests, reviewing changes, interpreting results, and reporting evidence. |

The TUI can change the initial phase, add a phase as
`phase-id | Label | optional description`, remove phases, and set or clear
model pins.

## Configuration file

The TUI stores the configuration atomically in
`$CODEX_HOME/taskdriven.toml`. A representative adaptive file is:

```toml
version = 2
enabled = true
mode = "adaptive"
initial_phase = "investigation"

[[phases]]
id = "writing"
label = "Writing"
description = "Drafting, rewriting, and user-facing prose"

[[phases]]
id = "investigation"
label = "Investigation"
description = "Reading, diagnosis, research, and design"

[adaptive]
profile = "builtin:standard@1"
use_case = "auto"
confirmation = "never"
allow_paid_speed_tier = false

[adaptive.pins.verification]
model = "MODEL_FROM_YOUR_LIVE_CATALOG"
reasoning_effort = "high"
```

Prefer editing through `/model`; it validates models and reasoning levels
against the live catalog. Phase IDs begin with a lowercase letter and contain
only lowercase letters, digits, `-`, or `_`.

## Runtime and failure behavior

When work changes phase, the agent calls `set_work_phase` as a dedicated
control step. A successful switch takes effect on the next model request in
the same thread. Codex records the actual model, rejects stale tools generated
before the transition, and persists routing on resume.

The active TaskDriven configuration, materialized `taskdriven_routing` block,
and successful `set_work_phase` result determine each phase's model and effort.
Generic phase instructions do not prescribe a model family or reasoning level.
A valid route remains authoritative when it differs from older instructions
retained in the conversation. Phase changes still require a control step when
two phases happen to select the same model.

TaskDriven fails closed: invalid configuration cannot be enabled, unavailable
models or reasoning levels prevent activation, and rejected transitions are
reported instead of silently using the wrong route. Selecting an ordinary
model returns the thread to fixed model selection. Subagents remain fixed
unless their own session explicitly enables another supported routing policy.
