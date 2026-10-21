#!/usr/bin/env bash
# Runs the checks quietly: each one's full output goes to target/check/<name>.log, and only a
# one-line summary, or the errors of the checks that failed, reach the terminal.
#   scripts/check.sh            the checks for what changed since main (auto)
#   scripts/check.sh ui         Svelte front end: npm test, svelte-check
#   scripts/check.sh core       oxbow-core: cargo fmt, clippy, cargo test -p oxbow-core
#   scripts/check.sh tauri      app/src-tauri: cargo fmt, clippy, cargo test -p oxbow-app
#   scripts/check.sh all        everything
# Several groups can be given at once: scripts/check.sh ui core
set -uo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
logs="$root/target/check"
mkdir -p "$logs"
cd "$root"

# What changed against main: committed on this branch, staged, unstaged and untracked.
changed_files() {
  # The newer fork point of the two, as either ref can be behind in a fresh clone.
  local bases base
  bases=$(for ref in origin/main main; do git merge-base HEAD "$ref" 2>/dev/null; done)
  [ -n "$bases" ] || return 1
  base=$(git merge-base --independent $bases | head -n 1)
  { git diff --name-only "$base"; git ls-files --others --exclude-standard; } | sort -u
}

groups=("$@")
if [ ${#groups[@]} -eq 0 ] || [ "${groups[0]}" = auto ]; then
  if files=$(changed_files); then
    groups=()
    grep -qE '^app/(src/|tests/|package|tsconfig|svelte\.config|vite\.config|index\.html)' <<<"$files" && groups+=(ui)
    grep -qE '^(crates/|Cargo\.|rustfmt\.toml)' <<<"$files" && groups+=(core)
    grep -qE '^(app/src-tauri/|Cargo\.)' <<<"$files" && groups+=(tauri)
    if [ ${#groups[@]} -eq 0 ]; then
      echo "check: nothing to check (no app or core files changed since main)"
      exit 0
    fi
  else
    groups=(all)
  fi
fi

want_ui=0 want_core=0 want_tauri=0
for g in "${groups[@]}"; do
  case $g in
    ui) want_ui=1 ;;
    core) want_core=1 ;;
    tauri) want_tauri=1 ;;
    all) want_ui=1 want_core=1 want_tauri=1 ;;
    *) echo "check: unknown group '$g' (ui, core, tauri, all)" >&2; exit 2 ;;
  esac
done

passed=() failed=()

# run <name> <filter> <command…>: runs the command with its output in the log; on failure prints
# the lines the filter picks (the last 30 lines when it picks none), at most 60.
run() {
  local name=$1 filter=$2
  shift 2
  local log="$logs/$name.log"
  if "$@" >"$log" 2>&1; then
    passed+=("$name")
    return
  fi
  failed+=("$name")
  echo "✗ $name (full log: ${log#"$root"/})"
  local picked
  picked=$("$filter" <"$log" | head -n 60)
  [ -n "$picked" ] || picked=$(tail -n 30 "$log")
  sed 's/^/  /' <<<"$picked"
}

# Rust errors and warnings with their location and the source lines under them.
rust_errors() { grep -E -A6 '^(error|warning)(\[|:)' | grep -vE '^(warning|error): (build failed|could not compile|.* generated [0-9]+ warnings?)'; }
# Failing tests: their names, panics and assertion lines.
rust_tests() { grep -E '^(---- |thread .* panicked| *(left|right):|assertion|error(\[|:)|test .* FAILED)'; }
# Node's test runner: what the failing test files printed (the assertion and where) and their names.
node_tests() {
  grep -E '^(# |not ok )' \
    | grep -vE '^# (Subtest:|tests|suites|pass|fail|cancelled|skipped|todo|duration_ms) |^# Node\.js |^# +at .*(node:internal|node_modules)'
}
# svelte-check: errors and warnings with their file.
svelte_errors() { grep -E -B1 -A2 '^(Error|Warn)'; }

ensure_node_modules() {
  [ -d app/node_modules ] || (cd app && npm ci --no-audit --no-fund) >"$logs/npm-install.log" 2>&1
}

if [ $want_ui = 1 ]; then
  if ensure_node_modules; then
    run npm-test node_tests sh -c 'cd app && npm test'
    run svelte-check svelte_errors sh -c 'cd app && npm run check -- --output human'
  else
    failed+=(npm-install)
    echo "✗ npm-install (full log: target/check/npm-install.log)"
    tail -n 20 "$logs/npm-install.log" | sed 's/^/  /'
  fi
fi

if [ $want_core = 1 ] || [ $want_tauri = 1 ]; then
  run fmt cat cargo fmt --all --check -- --color never
  if [ $want_tauri = 1 ]; then
    run clippy rust_errors cargo clippy --workspace --all-targets -- -D warnings
  else
    run clippy rust_errors cargo clippy -p oxbow-core --all-targets -- -D warnings
  fi
  [ $want_core = 1 ] && run test-core rust_tests cargo test -p oxbow-core
  [ $want_tauri = 1 ] && run test-tauri rust_tests cargo test -p oxbow-app
fi

if [ ${#failed[@]} -eq 0 ]; then
  echo "check: all green (${passed[*]})"
  exit 0
fi
echo "check: FAILED ${failed[*]}${passed[*]:+; passed ${passed[*]}}"
exit 1
