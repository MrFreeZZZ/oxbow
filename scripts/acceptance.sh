#!/usr/bin/env bash
# Runs the acceptance scenarios (app/e2e/scenarios.mjs; docs/acceptance.md says what each checks)
# on the real app: built with `tauri build`, opened under Xvfb and driven through tauri-driver,
# with real git on fixtures made afresh. Linux only, as tauri-driver has no macOS WebDriver; the
# Mac checklist in docs/acceptance.md covers the rest.
#   scripts/acceptance.sh                  build the app, then run every scenario
#   scripts/acceptance.sh staging two      only the scenarios whose name has one of these words
#   OXBOW_APP=target/release/oxbow scripts/acceptance.sh    use a build already made
# Prints one line a scenario; logs and a picture of each failure go to target/acceptance/.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
out="$root/target/acceptance"
rm -rf "$out"
mkdir -p "$out"

missing=()
for tool in tauri-driver WebKitWebDriver Xvfb node git; do
  command -v "$tool" >/dev/null || missing+=("$tool")
done
if [ ${#missing[@]} -gt 0 ]; then
  echo "acceptance: missing ${missing[*]}." >&2
  echo "  sudo apt-get install webkit2gtk-driver xvfb; cargo install tauri-driver --locked" >&2
  exit 2
fi

app=${OXBOW_APP:-}
if [ -z "$app" ]; then
  echo "acceptance: building the app (log: target/acceptance/build.log)…"
  if ! (cd "$root/app" && npx tauri build --no-bundle) >"$out/build.log" 2>&1; then
    tail -n 20 "$out/build.log" >&2
    exit 1
  fi
  app="$root/target/release/oxbow"
fi
app=$(cd "$(dirname "$app")" && pwd)/$(basename "$app")

# Everything the app keeps (settings, recent repositories, drafts) goes to a folder of this run.
home=$(mktemp -d)
pids=()
cleanup() {
  for pid in "${pids[@]}"; do kill "$pid" 2>/dev/null || true; done
  wait 2>/dev/null || true
  rm -rf "$home" 2>/dev/null || true
}
trap cleanup EXIT

if [ -z "${DISPLAY:-}" ]; then
  export DISPLAY=:97
  Xvfb "$DISPLAY" -screen 0 1600x1000x24 -nolisten tcp >"$out/xvfb.log" 2>&1 &
  pids+=($!)
  sleep 1
fi

export HOME="$home/home" XDG_CONFIG_HOME="$home/config" XDG_DATA_HOME="$home/data" XDG_CACHE_HOME="$home/cache"
mkdir -p "$HOME" "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_CACHE_HOME"
export GIT_CONFIG_GLOBAL="$HOME/.gitconfig"
git config --global user.name "Acceptance"
git config --global user.email "acceptance@example.com"
git config --global init.defaultBranch main
# The fixtures' submodules come from folders, which git refuses for submodules by default.
git config --global protocol.file.allow always

tauri-driver --port 4444 >"$out/driver.log" 2>&1 &
pids+=($!)
for _ in $(seq 50); do
  curl -s http://127.0.0.1:4444/status >/dev/null 2>&1 && break
  sleep 0.2
done

OXBOW_APP="$app" OXBOW_HOME="$home" OXBOW_OUT="$out" node "$root/app/e2e/run.mjs" "$@"
