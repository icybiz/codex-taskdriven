# Taskdriven model regression fixture

The complete Sol and Astra records were extracted without modifying their values
from the account catalog captured on 2026-09-10 for this prototype. This fixture
contains model metadata only. It contains no account identifiers or credentials.

Source cache SHA256: `821aa73e70bf4e0eaa105975a3ddcb3e8c7fae8d6e72c1897e83bac369048707`.
In particular Sol has `node_repl_auto_review_required = false`; Astra has `true`.
Their different Guardian messages and classifier defaults are intentionally kept.
Tests must not normalize these records to make a phase switch pass.
