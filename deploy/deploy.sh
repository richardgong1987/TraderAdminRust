#!/usr/bin/env bash
# One command to deploy abs_admin on an Ubuntu server, the first time and after every `git pull`:
#   bash deploy/deploy.sh        (as root, from any folder)
# It installs what is missing (Docker, Ollama, the Gemma model), creates deploy/.env with random
# passwords, starts MySQL once and leaves it running, then rebuilds and restarts the app.
# Each step skips what is already done, so running it again is safe. See README.md in this folder.

# Ubuntu's `sh` is dash, which lacks what this script uses, so `sh deploy.sh` re-runs it under bash.
if [ -z "${BASH_VERSION:-}" ]; then exec bash "$0" "$@"; fi
set -euo pipefail

DEPLOY_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(dirname "$DEPLOY_DIR")"
ENV_FILE="$DEPLOY_DIR/.env"
APP_URL="http://127.0.0.1:8000"
OLLAMA_URL="http://127.0.0.1:11434"
# Docker names volumes <Compose project>_<volume>; see docker-compose.mysql.yml.
MYSQL_VOLUME="abs_admin_db_mysql_data"
PULL_LOG="/root/pull.log"

step() { printf '\n==> %s\n' "$*"; }
fail() { printf '\nERROR: %s\n' "$*" >&2; exit 1; }

apt_install() {
  apt-get update
  # No "restart services?" dialog halfway through a deploy.
  DEBIAN_FRONTEND=noninteractive NEEDRESTART_MODE=a apt-get install -y "$@"
}

# Returns success once the URL answers, failure after the given number of seconds.
wait_for_url() {
  local url=$1 seconds=$2
  for _ in $(seq "$seconds"); do
    curl -fsS -o /dev/null "$url" 2>/dev/null && return 0
    sleep 1
  done
  return 1
}

# application.json5 is the one place the model is set.
configured_model() {
  local model
  model=$(sed -n 's/^ *ollama_model: *"\([^"]*\)".*/\1/p' "$REPO_DIR/application.json5")
  [ -n "$model" ] || fail "ollama_model not found in application.json5"
  echo "$model"
}

install_docker() {
  step "Docker"
  if docker compose version >/dev/null 2>&1 && docker buildx version >/dev/null 2>&1; then
    echo "Already installed: $(docker --version)"
  else
    # buildx: the Dockerfile uses build cache mounts, which only BuildKit supports.
    apt_install docker.io docker-compose-v2 docker-buildx
  fi
  systemctl enable --now docker
}

# gemma-install.md, Steps 4 and 5.
install_ollama() {
  step "Ollama"
  if command -v ollama >/dev/null; then
    echo "Already installed"
  else
    # The installer needs zstd to unpack its download.
    apt_install curl zstd
    curl -fsSL https://ollama.com/install.sh | sh
  fi
  local override=/etc/systemd/system/ollama.service.d/override.conf
  if [ ! -f "$override" ]; then
    # No OLLAMA_HOST: Ollama has no login, so it stays on 127.0.0.1.
    mkdir -p "$(dirname "$override")"
    cat > "$override" <<'EOF'
[Service]
Environment="OLLAMA_CONTEXT_LENGTH=8192"
Environment="OLLAMA_KEEP_ALIVE=24h"
EOF
    systemctl daemon-reload
    systemctl restart ollama
  fi
  systemctl enable --now ollama
  wait_for_url "$OLLAMA_URL/api/version" 30 \
    || fail "Ollama is not answering at $OLLAMA_URL. See: journalctl -u ollama -n 50"
  ollama --version
}

# gemma-install.md, Step 6.
pull_model() {
  step "Model $MODEL"
  if ollama show "$MODEL" >/dev/null 2>&1; then
    echo "Already downloaded"
  elif pgrep -f "ollama pull $MODEL" >/dev/null; then
    echo "Still downloading. Progress: tail -f $PULL_LOG"
  else
    # Hours at this server's ~10 Mbps, so it runs in the background while the deploy goes on.
    nohup ollama pull "$MODEL" > "$PULL_LOG" 2>&1 &
    echo "Download started in the background. Progress: tail -f $PULL_LOG"
  fi
}

create_env_file() {
  step "Secrets in $ENV_FILE"
  if [ -f "$ENV_FILE" ]; then
    echo "Using the existing file"
    return
  fi
  # MySQL reads its passwords on its first start only, so new ones would lock the app out of the data.
  if docker volume inspect "$MYSQL_VOLUME" >/dev/null 2>&1; then
    fail "$ENV_FILE is missing, but MySQL already has data in volume $MYSQL_VOLUME. Put the old .env back."
  fi
  # Hex only, because the passwords go into a mysql:// URL. The umask makes the file private from the
  # moment it exists; the subshell keeps it from affecting the rest of the script.
  (
    umask 077
    cat > "$ENV_FILE" <<EOF
# Created by deploy.sh on $(date +%F). Keep a copy in a password manager. Never commit this file.
# What each value means: .env.example
MYSQL_ROOT_PASSWORD=$(openssl rand -hex 24)
MYSQL_PASSWORD=$(openssl rand -hex 24)
JWT_SECRET=$(openssl rand -hex 24)
EOF
  )
  echo "Created with new random passwords. Save a copy now: cat $ENV_FILE"
}

start_mysql() {
  step "MySQL"
  # Leaves a running MySQL alone unless docker-compose.mysql.yml changed. --wait returns once it is healthy.
  docker compose -f docker-compose.mysql.yml up -d --wait
}

start_app() {
  step "App"
  echo "Building. The first build downloads about 1 GB and compiles for several minutes; later ones reuse the cache."
  docker compose up -d --build
  if ! wait_for_url "$APP_URL/admin/" 120; then
    docker compose ps
    docker compose logs --tail 30 app
    fail "The app is not answering at $APP_URL. Its log files: docker compose exec app sh -c 'tail -n 50 target/logs/*.log'"
  fi
  echo "Running at $APP_URL"
  # Each build leaves the previous image behind, untagged.
  docker image prune -f >/dev/null
}

print_next_steps() {
  echo
  echo "Done. After a reboot, Docker starts MySQL and the app again, and systemd starts Ollama."
  if ! ollama show "$MODEL" >/dev/null 2>&1; then
    echo "$MODEL is still downloading (tail -f $PULL_LOG). Until it finishes, AI test runs end in ERROR."
  fi
  cat <<EOF

Open the admin page: http://<this server's public IP>:8000
First deploy only: log in as 00000000000 / 123456 and change the password straight away.

Deploy an update:  cd $REPO_DIR && git pull && bash deploy/deploy.sh
EOF
}

main() {
  [ "$(id -u)" -eq 0 ] || fail "Run as root: sudo bash $0"
  cd "$DEPLOY_DIR"
  MODEL=$(configured_model)
  install_docker
  install_ollama
  pull_model
  create_env_file
  start_mysql
  start_app
  print_next_steps
}

main "$@"
