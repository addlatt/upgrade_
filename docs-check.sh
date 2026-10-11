#!/usr/bin/env bash
#
# docs-check.sh - do the docs still follow the house rules?
# (.claude/skills/docs/SKILL.md says what the rules are and why)
#
#   ./docs-check.sh        check every doc, change nothing; exit 1 on any FAIL
#
# What it checks: style (no em dashes, emoji or stray bars), dated Decided
# lines, no journal paragraphs in living docs, risk and gate headings with a
# severity and a state, STATUS.md rows against the CSVs' result column,
# every R##/V## defined, relative links resolving, expiry and archive lines,
# and each doc against its budget (.claude/skills/docs/budgets.json).
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec python3 -I "$ROOT/docs/tools/docs-check.py" "$ROOT"
