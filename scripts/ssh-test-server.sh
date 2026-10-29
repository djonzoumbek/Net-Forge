#!/usr/bin/env bash
# Starts (or stops) a local OpenSSH test server in Docker, bound to 127.0.0.1 only.
#   ./scripts/ssh-test-server.sh start   -> build image, generate test keys, run
#   ./scripts/ssh-test-server.sh stop    -> remove the container
# Credentials: user "netforge", password "netforge-test", port 2222.
set -euo pipefail

NAME=netforge-sshd
PORT="${NETFORGE_SSH_TEST_PORT:-2222}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
KEYS="$ROOT/src-tauri/target/ssh-test"

case "${1:-start}" in
  start)
    mkdir -p "$KEYS"
    # Test-only keys, kept under target/ (ignored by git).
    [ -f "$KEYS/id_plain" ] || ssh-keygen -q -t ed25519 -N '' -C netforge-test -f "$KEYS/id_plain"
    [ -f "$KEYS/id_pass" ] || ssh-keygen -q -t ed25519 -N 'test-passphrase' -C netforge-test -f "$KEYS/id_pass"
    cat "$KEYS/id_plain.pub" "$KEYS/id_pass.pub" > "$KEYS/authorized_keys"
    chmod 600 "$KEYS/authorized_keys"
    docker build -q -t netforge-sshd "$ROOT/scripts/ssh-test-server" >/dev/null
    docker rm -f "$NAME" >/dev/null 2>&1 || true
    docker run -d --name "$NAME" -p "127.0.0.1:$PORT:22" \
      -v "$KEYS/authorized_keys:/home/netforge/.ssh/authorized_keys:ro" \
      netforge-sshd >/dev/null
    echo "SSH test server on 127.0.0.1:$PORT (user netforge / netforge-test)"
    echo "Keys: $KEYS/id_plain (no passphrase), $KEYS/id_pass (passphrase: test-passphrase)"
    ;;
  stop)
    docker rm -f "$NAME" >/dev/null && echo "stopped"
    ;;
  *)
    echo "usage: $0 [start|stop]" >&2
    exit 1
    ;;
esac
