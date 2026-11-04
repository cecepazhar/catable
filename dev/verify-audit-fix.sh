#!/usr/bin/env bash
# Evidence run for task-audit-fix.md (Phase 0). Prints raw command output, nothing summarised.
#
#   dev/verify-audit-fix.sh            # fmt, clippy, unit tests, frontend checks
#   dev/verify-audit-fix.sh --it       # also: docker stack + integration tests (real PG/MySQL/MariaDB/SQLite)
#
# Paste the whole output back as the evidence for the items in the task file.
set -u
cd "$(dirname "$0")/.."

run() { echo; echo "\$ $*"; "$@"; echo "[exit $?]"; }

run cargo fmt --check
run cargo clippy --workspace --all-targets -- -D warnings
run cargo test -p catable-core
run cargo test -p catable-app

# `npm run check` reports 11 errors that already exist at HEAD (see task-audit-fix.md, "Status"):
# anything beyond those 11 is new.
(cd frontend && run npm run check)
(cd frontend && run npm test)

if [[ "${1:-}" == "--it" ]]; then
    dev/it-setup.sh
    run docker compose -f dev/compose.test.yml up -d --wait
    set -a
    # shellcheck disable=SC1091
    . dev/.env
    set +a
    run cargo test -p catable-core --features it
fi
