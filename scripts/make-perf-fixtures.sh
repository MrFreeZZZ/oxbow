#!/usr/bin/env bash
# Creates the repositories Oxbow's reads are measured on (see docs/measuring-reads.md):
#   history-100k    100,000 commits, 30 side branches (half merged back), 20 tags
#   worktrees-20    a repository with 20 more worktrees, every third with uncommitted changes
#   worktrees-50    the same with 50
#   submodules-20   20 submodules: clean, changed, behind, ahead, on a branch, not cloned
# Usage: scripts/make-perf-fixtures.sh <folder> [history|worktrees|submodules ...]
set -euo pipefail

dest=${1:?usage: make-perf-fixtures.sh <folder> [history|worktrees|submodules ...]}
shift
parts=("$@")
[ ${#parts[@]} -gt 0 ] || parts=(history worktrees submodules)
mkdir -p "$dest"
dest=$(cd "$dest" && pwd)

export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null
export GIT_AUTHOR_NAME="Perf Fixture" GIT_AUTHOR_EMAIL="perf@example.com"
export GIT_COMMITTER_NAME="Perf Fixture" GIT_COMMITTER_EMAIL="perf@example.com"
export GIT_AUTHOR_DATE="1700000000 +0000" GIT_COMMITTER_DATE="1700000000 +0000"

# `files <folder> <count>`: that many small source files in 40 folders.
files() {
  local dir=$1 count=$2 i
  for ((i = 0; i < count; i++)); do
    mkdir -p "$dir/src/m$((i % 40))"
    printf 'fn f%d() -> u32 {\n    %d\n}\n' "$i" "$i" >"$dir/src/m$((i % 40))/f$i.rs"
  done
}

history() {
  local repo=$dest/history-100k
  rm -rf "$repo"
  git init -q -b main "$repo"
  # One fast-import stream: main gets 100k commits, each changing one of 2,000 files; a side
  # branch leaves main every 3,000 commits and every other one is merged back 1,000 later.
  awk 'BEGIN {
    t = 1600000000; mark = 0
    for (i = 1; i <= 100000; i++) {
      mark++; main[i] = mark
      print "commit refs/heads/main"
      print "mark :" mark
      printf "committer Perf Fixture <perf@example.com> %d +0000\n", t + i * 60
      msg = "Change file " (i % 2000) " (" i ")"
      print "data " length(msg); print msg
      if (i > 1) print "from :" main[i - 1]
      if (i % 3000 == 1000 && (i - 1000) / 3000 % 2 == 1 && side[i - 1000] != "") print "merge :" side[i - 1000]
      body = "line " i
      printf "M 644 inline src/f%d.txt\ndata %d\n%s\n\n", i % 2000, length(body), body
      if (i % 3000 == 0) {
        b = i / 3000
        prev = main[i]
        for (k = 1; k <= 3; k++) {
          mark++
          printf "commit refs/heads/topic/side-%02d\n", b
          print "mark :" mark
          printf "committer Perf Fixture <perf@example.com> %d +0000\n", t + i * 60 + k
          msg = "Side work " b "." k
          print "data " length(msg); print msg
          print "from :" prev
          printf "M 644 inline side/b%d.txt\ndata %d\n%s\n\n", b, length(msg), msg
          prev = mark
        }
        side[i] = prev
      }
      if (i % 5000 == 0) { printf "tag v%d\nfrom :%d\ntagger Perf Fixture <perf@example.com> %d +0000\ndata 7\nrelease\n", i / 5000, main[i], t + i * 60 }
    }
  }' | git -C "$repo" fast-import --quiet
  git -C "$repo" checkout -q main
  echo "history-100k: $(git -C "$repo" rev-list --count --all) commits"
}

worktrees() {
  local n repo i name
  for n in 20 50; do
    repo=$dest/worktrees-$n
    rm -rf "$repo"
    mkdir -p "$repo"
    git init -q -b main "$repo/main"
    files "$repo/main" 2000
    git -C "$repo/main" add -A
    git -C "$repo/main" commit -q -m "Start"
    for ((i = 1; i <= n; i++)); do
      name=$(printf 'wt-%02d' "$i")
      git -C "$repo/main" worktree add -q -b "work/$name" "../$name"
      if ((i % 3 == 0)); then
        printf '// edited\n' >>"$repo/$name/src/m1/f1.rs"
        printf '// edited\n' >>"$repo/$name/src/m2/f2.rs"
        printf '// edited\n' >>"$repo/$name/src/m3/f3.rs"
        printf 'new\n' >"$repo/$name/notes-$i.txt"
        printf 'new\n' >"$repo/$name/src/new-$i.rs"
      fi
    done
    echo "worktrees-$n: $(git -C "$repo/main" worktree list | wc -l) worktrees"
  done
}

submodules() {
  local repo=$dest/submodules-20 src=$dest/submodules-20-src i name dir
  rm -rf "$repo" "$src"
  mkdir -p "$src"
  for ((i = 1; i <= 20; i++)); do
    name=$(printf 'lib-%02d' "$i")
    git init -q -b main "$src/$name"
    files "$src/$name" 300
    git -C "$src/$name" add -A
    git -C "$src/$name" commit -q -m "Start $name"
    for k in 1 2 3; do
      printf 'step %d\n' "$k" >>"$src/$name/CHANGES"
      git -C "$src/$name" add -A
      git -C "$src/$name" commit -q -m "Step $k of $name"
    done
  done
  git init -q -b main "$repo"
  files "$repo" 200
  for ((i = 1; i <= 20; i++)); do
    name=$(printf 'lib-%02d' "$i")
    git -C "$repo" -c protocol.file.allow=always submodule add -q "$src/$name" "libs/$name"
  done
  git -C "$repo" add -A
  git -C "$repo" commit -q -m "Start with 20 submodules"
  for ((i = 1; i <= 20; i++)); do
    name=$(printf 'lib-%02d' "$i")
    dir=$repo/libs/$name
    case $i in
      5 | 6 | 7 | 8) # uncommitted changes
        printf '// edited\n' >>"$dir/src/m1/f1.rs"
        printf 'new\n' >"$dir/scratch.txt" ;;
      9 | 10 | 11 | 12) # an older commit checked out: behind the recorded one
        git -C "$dir" checkout -q HEAD~2 ;;
      13 | 14 | 15) # a commit of its own: ahead of the recorded one
        git -C "$dir" checkout -q -b local-work
        printf 'local\n' >>"$dir/CHANGES"
        git -C "$dir" commit -q -am "Local work in $name" ;;
      16 | 17) # not cloned
        git -C "$repo" submodule deinit -q -f "libs/$name" ;;
      18 | 19) # on its branch
        git -C "$dir" checkout -q main ;;
    esac
  done
  echo "submodules-20: $(git -C "$repo" submodule status | wc -l) submodules"
}

for part in "${parts[@]}"; do
  case $part in
    history | worktrees | submodules) "$part" ;;
    *) echo "make-perf-fixtures: unknown part '$part' (history, worktrees, submodules)" >&2; exit 2 ;;
  esac
done
