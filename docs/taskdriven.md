# Using TaskDriven

TaskDriven lets one Codex thread use different model and reasoning settings for
different kinds of work. A task is a named route, not a permission profile.
Switching tasks never relaxes the sandbox, approval policy, or tool rules.

## Enable the feature

TaskDriven currently depends on two feature flags. Add them to the user-owned
Codex configuration in `$CODEX_HOME/config.toml`:

```toml
[features]
step_model_switching = true
autonomous_work_phase = true
```

Start Codex, open `/model`, choose **TaskDriven**, and enable it. The same
screen lets you:

- choose a model and supported reasoning effort for every task;
- select the initial task;
- add a task as `task-id | Label | optional description`; and
- remove tasks that are no longer useful.

The picker uses the live model catalog. New model versions can therefore be
assigned as soon as the account and Codex catalog expose them. A configuration
cannot be enabled if it refers to a missing model or an unsupported reasoning
effort.

## Default tasks

The initial configuration contains four editable routes:

| Task | Intended work |
| --- | --- |
| Writing | Drafting, rewriting, documentation, explanations, and user-facing prose. |
| Investigation | Reading code and documents, research, diagnosis, bug discovery, and design. This is the route previously described informally as “Reading.” |
| Implementation | Changing code, fixing bugs, refactoring, and creating or updating tests. |
| Verification | Building, running existing tests, reviewing changes, interpreting results, and reporting evidence. |

These meanings are guidance for the agent. You can edit the tasks and their
descriptions to match another workflow.

## Configuration file

The TUI stores the versioned configuration atomically in
`$CODEX_HOME/taskdriven.toml`. A representative file is:

```toml
version = 1
enabled = true
initial_task = "investigation"

[[tasks]]
id = "writing"
label = "Writing"
description = "Drafting, rewriting, and user-facing prose"
model = "MODEL_FROM_YOUR_LIVE_CATALOG"
reasoning_effort = "high"

[[tasks]]
id = "investigation"
label = "Investigation"
description = "Reading, diagnosis, research, and design"
model = "MODEL_FROM_YOUR_LIVE_CATALOG"
reasoning_effort = "high"

[[tasks]]
id = "implementation"
label = "Implementation"
description = "Code changes, fixes, and test authoring"
model = "ANOTHER_MODEL_FROM_YOUR_LIVE_CATALOG"
reasoning_effort = "high"

[[tasks]]
id = "verification"
label = "Verification"
description = "Builds, tests, reviews, and result reporting"
model = "MODEL_FROM_YOUR_LIVE_CATALOG"
reasoning_effort = "high"
```

Prefer editing through `/model`; it validates models and reasoning levels
against the current catalog. Manual edits are validated when Codex next loads
the file. Task IDs must begin with a lowercase letter and may contain lowercase
letters, digits, `-`, and `_`.

## Runtime behavior

When the work changes category, the agent calls `set_work_phase` as a dedicated
control step. A successful call selects the configured model and reasoning
effort for the next model request in the same thread. Codex records the actual
model used, rejects stale tools generated before a phase transition, and keeps
the routing state when the thread is resumed.

Choosing an ordinary model explicitly switches the thread back to fixed model
selection. Subagents remain fixed unless their own session explicitly enables
another supported routing policy.

## Failure behavior

TaskDriven fails closed:

- an invalid configuration disables the TaskDriven choice and reports the
  validation error;
- an unavailable model or reasoning level prevents activation;
- a rejected phase change is reported instead of continuing under the wrong
  route; and
- model routing never changes permissions or approval requirements.
