#!/usr/bin/env bash
# Creates a small repository with branches, merges, tags and unpushed work,
# similar to the History design, for trying Oxbow out:
#   scripts/make-demo-repo.sh /tmp/acme-api && cargo tauri dev -- -- /tmp/acme-api
set -euo pipefail

dest=${1:?usage: make-demo-repo.sh <folder>}
rm -rf "$dest" "$dest.remote.git"
mkdir -p "$dest"
cd "$dest"

export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null
clock=$(( $(date +%s) - 3 * 86400 ))
who() { export GIT_AUTHOR_NAME="$1" GIT_AUTHOR_EMAIL="$2" GIT_COMMITTER_NAME="$1" GIT_COMMITTER_EMAIL="$2"; }
tick() {
  clock=$((clock + ${1:-1500}))
  export GIT_AUTHOR_DATE="$clock +0300" GIT_COMMITTER_DATE="$clock +0300"
}
commit() { # file, line, message
  mkdir -p "$(dirname "$1")"
  printf '%s\n' "$2" >> "$1"
  git add -A
  tick
  git commit -q -m "$3"
}
alex() { who "Alexander" "alexander@example.com"; }
maria() { who "Maria Ivanova" "maria@example.com"; }
dmitry() { who "Dmitry Sokolov" "dmitry@example.com"; }
bot() { who "renovate[bot]" "bot@renovateapp.com"; }

git init -q -b main
dmitry; commit src/routes.rs "fn routes() {}" "Remove deprecated v1 login route"
alex;   commit .github/ci.yml "rust: 1.90" "Update CI to Rust 1.90"
maria;  commit src/middleware.rs "log request id" "Log request ids in middleware"
dmitry; commit src/config.rs "use figment;" "Switch config loading to figment"
alex;   commit src/http.rs "struct RetryPolicy;" "Add retry policy to http client"
git tag -a v0.9.0 -m "Release 0.9.0"

git checkout -q -b fix/retry-backoff
dmitry; commit src/http_test.rs "fn retry_is_stable() {}" "Fix flaky retry test in http client"
dmitry; commit src/http.rs "jitter: true" "Add jittered backoff to retries"
git checkout -q main
git checkout -q -b docs/readme
maria;  commit docs/arch.md "diagram" "Add architecture diagram"
git checkout -q main
tick; git merge -q --no-ff fix/retry-backoff -m "Merge pull request #405 from acme/fix/retry-backoff"
git tag -a v0.9.1 -m "Release 0.9.1"
alex;   commit Cargo.toml 'tokio = "1.47"' "Bump tokio to 1.47"
git checkout -q docs/readme
maria;  commit README.md "## Quick start" "Rewrite README quick start"
git checkout -q main
tick; git merge -q --no-ff docs/readme -m "Merge pull request #412 from acme/docs/readme"
git branch -q -D fix/retry-backoff docs/readme

git checkout -q -b feature/billing-webhooks
maria;  commit src/billing/webhook.rs "fn endpoint() {}" "Add webhook endpoint skeleton"
git checkout -q main
git checkout -q -b auth/1-models
alex;   commit src/auth/models.rs "struct Session;" "Add Session and RefreshToken models"
git checkout -q main
git checkout -q -b fix/login-timeout
maria;  commit src/login.rs "timeout = 30" "Increase login request timeout to 30 s"
git checkout -q auth/1-models
git checkout -q -b auth/2-api
alex;   commit src/auth/rotation.rs "fn rotate() {}" "Validate refresh token rotation"
git checkout -q feature/billing-webhooks
maria;  commit src/billing/verify.rs "fn verify() {}" "Verify Stripe webhook signatures"
git checkout -q main
git checkout -q -b feature/rate-limit-v2
dmitry; commit src/limit.rs "sliding window" "Sliding window limiter for /login"
git checkout -q feature/billing-webhooks
git checkout -q -b feature/billing-ui
dmitry; commit ui/invoices.tsx "export {}" "Scaffold invoice list component"
git checkout -q auth/2-api
alex;   commit src/auth/api.rs "/v2/sessions" "Expose /v2/sessions endpoint"
git checkout -q main
git checkout -q -b renovate/serde-1.x
bot;    commit Cargo.toml 'serde = "1.0.228"' "Bump serde to 1.0.228"
git checkout -q auth/2-api
git checkout -q -b auth/3-ui
alex;   commit ui/client.ts "refresh()" "Wire token refresh into API client"
git checkout -q feature/billing-ui
dmitry; commit ui/settings.tsx "export {}" "Add billing settings page"
git checkout -q feature/billing-webhooks
maria;  commit src/billing/webhook.rs "invoice.payment_failed" "Handle invoice.payment_failed webhook"
git checkout -q auth/3-ui
alex;   commit ui/login.tsx "<SessionBanner />" "Add session expiry banner to login screen"
git checkout -q main
maria;  commit docs/limits.md "X-RateLimit-Remaining" "Document rate limit headers"
bot;    commit Cargo.toml 'axum = "0.8.4"' "Bump axum to 0.8.4"
dmitry; commit src/db.rs "503 when pool exhausted" "Return 503 when the DB pool is exhausted"

# Publish everything except the newest work on auth/3-ui and fix/login-timeout.
git init -q --bare "$dest.remote.git"
git remote add origin "$dest.remote.git"
git push -q origin main feature/billing-webhooks feature/billing-ui feature/rate-limit-v2 \
  renovate/serde-1.x auth/1-models auth/2-api "auth/3-ui~2:refs/heads/auth/3-ui" --tags
git checkout -q auth/3-ui

# Work in progress set aside on auth/3-ui.
alex
echo "<SessionBanner expiresIn={300} />" > ui/login.tsx
git stash push -q -m "Try a countdown in the expiry banner"
echo "Created $dest"
