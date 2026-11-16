#!/bin/bash
# Cloud sessions only: installs what Tauri needs to compile on Linux and fetches the npm and cargo
# dependencies before the session starts, so none of it runs (or prints) inside the session.
# All output goes to /tmp/oxbow-session-start.log; only a one-line result is printed.
set -euo pipefail

[ "${CLAUDE_CODE_REMOTE:-}" = "true" ] || exit 0

cd "${CLAUDE_PROJECT_DIR:-$(dirname "$0")/../..}"
log=/tmp/oxbow-session-start.log
: >"$log"

step() {
  if ! "$@" >>"$log" 2>&1; then
    echo "session-start: '$*' failed, see $log"
    exit 1
  fi
}

packages=(libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev)
if ! dpkg -s "${packages[@]}" >/dev/null 2>&1; then
  sudo=""
  [ "$(id -u)" = 0 ] || sudo=sudo
  step $sudo apt-get update -qq
  step env DEBIAN_FRONTEND=noninteractive $sudo apt-get install -y -qq "${packages[@]}"
fi

# npm ci, not npm install: install rewrites package-lock.json when it is out of step.
[ -d app/node_modules ] || step sh -c 'cd app && npm ci --no-audit --no-fund'
step cargo fetch --locked
# Fresh origin/preview and origin/main, so scripts/check.sh compares against the real fork point
# (preview, or main in older clones). Explicit refspecs, as a single-branch clone has no
# remote-tracking ref for the other one; one at a time, so a missing preview doesn't stop main.
for branch in preview main; do
  git fetch -q origin "+refs/heads/${branch}:refs/remotes/origin/${branch}" >>"$log" 2>&1 || true
done

echo "session-start: ready (Tauri packages, npm and cargo dependencies)"
