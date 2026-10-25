#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

readonly BASE_COMMIT=6aaac694803004a63c6f393d153c96d22521a6b9
readonly REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

fail() {
  printf 'license check failed: %s\n' "$*" >&2
  exit 1
}

git cat-file -e "${BASE_COMMIT}^{commit}" 2>/dev/null \
  || fail "upstream base commit is unavailable"

if ! git diff --quiet "$BASE_COMMIT" -- LICENSE; then
  fail "the upstream LICENSE file changed"
fi

upstream_notice="$(git show "${BASE_COMMIT}:NOTICE")"
current_notice_prefix="$(head -n "$(printf '%s\n' "$upstream_notice" | wc -l)" NOTICE)"
if [[ "$current_notice_prefix" != "$upstream_notice" ]]; then
  fail "the upstream NOTICE content is not an exact prefix of the current NOTICE"
fi

if git diff --diff-filter=D --name-only "$BASE_COMMIT" -- | grep -q .; then
  fail "an upstream file was deleted"
fi

while IFS=$'\t' read -r status path; do
  [[ -n "$status" && -n "$path" ]] || continue
  if ! grep -Fqx "${status}"$'\t'"${path}" MODIFICATIONS.md; then
    fail "MODIFICATIONS.md does not list ${status} ${path}"
  fi
done < <(git diff --name-status "$BASE_COMMIT" --)

binary_count="$({ git diff --numstat "$BASE_COMMIT" -- || true; } \
  | awk '$1 == "-" || $2 == "-" { count++ } END { print count + 0 }')"
if [[ "$binary_count" != 0 ]]; then
  fail "the derivative diff contains binary files"
fi

unexpected_packages="$({ git diff --unified=0 "$BASE_COMMIT" -- codex-rs/Cargo.lock || true; } \
  | sed -n 's/^+name = "\([^"]*\)"/\1/p' \
  | grep -vx 'codex-taskdriven' || true)"
if [[ -n "$unexpected_packages" ]]; then
  fail "Cargo.lock adds external packages: ${unexpected_packages//$'\n'/, }"
fi

git diff --check "$BASE_COMMIT" -- \
  || fail "the derivative diff contains whitespace errors"

printf '%s\n' \
  "license check passed" \
  "base=${BASE_COMMIT}" \
  "license=unchanged" \
  "notice=upstream-prefix-retained" \
  "modifications=fully-listed" \
  "binary-additions=0" \
  "new-external-cargo-packages=0"
