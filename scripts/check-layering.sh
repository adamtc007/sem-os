#!/usr/bin/env bash
# check-layering.sh — layering guard for the sem-os library.
# Run from the repo root (~/dev/sem-os/).
#
# Rule: sem-os crates must NOT reference any ob-poc domain or app crate.
# sem-os is a pure library; ob-poc-types, ob-poc-boundary, dsl-runtime, etc.
# are application-layer concerns that must never appear in library code.
set -uo pipefail

fail=0
note() { printf '  \033[31mFORBIDDEN EDGE\033[0m  %s\n' "$1"; fail=1; }

OBPOC_DOMAIN='ob_poc_types|ob_poc_boundary|ob_poc_sage|ob_poc_journey|ob_poc_domain|ob_poc_authoring|entity_gateway|dsl_runtime|ob_workflow|ob_agentic|ob_poc_web|inspector_projection|playbook_core|playbook_lower'

echo "== sem-os layering guard =="

for crate_src in crates/sem_os_types/src crates/sem_os_core/src crates/sem_os_ontology/src crates/sem_os_policy/src; do
  [ -d "$crate_src" ] || continue
  hits="$(grep -rnE "\b(${OBPOC_DOMAIN})\b" "$crate_src" 2>/dev/null \
    | grep -vE ':[0-9]+:[[:space:]]*//' || true)"
  [ -n "$hits" ] && note "$(basename $(dirname $crate_src)) references ob-poc domain/app:
$hits"
done

if [ "$fail" -eq 0 ]; then
  echo "  OK — sem-os is free of ob-poc domain/app dependencies."
else
  echo ""
  echo "== Layering guard FAILED =="
fi
exit "$fail"
