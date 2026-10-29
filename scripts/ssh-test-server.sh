#!/usr/bin/env bash
# Starts (or stops) a local OpenSSH test server in Docker, bound to 127.0.0.1 only.
#   ./scripts/ssh-test-server.sh start   -> build image, generate test keys, run
#   ./scripts/ssh-test-server.sh stop    -> remove the containers
#   ./scripts/ssh-test-server.sh rotate-keys -> new host keys (simulates a reinstall
#                                           or a man-in-the-middle)
# Credentials: user "netforge", password "netforge-test".
# Port 2222: modern OpenSSH. Port 2223: legacy-only algorithms (old Cisco/Huawei).
set -euo pipefail

NAME=netforge-sshd
LEGACY_NAME=netforge-sshd-legacy
PORT="${NETFORGE_SSH_TEST_PORT:-2222}"
LEGACY_PORT="${NETFORGE_SSH_LEGACY_PORT:-2223}"
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
    docker rm -f "$NAME" "$LEGACY_NAME" >/dev/null 2>&1 || true
    docker run -d --name "$NAME" -p "127.0.0.1:$PORT:22" \
      -v "$KEYS/authorized_keys:/home/netforge/.ssh/authorized_keys:ro" \
      netforge-sshd >/dev/null
    docker run -d --name "$LEGACY_NAME" -p "127.0.0.1:$LEGACY_PORT:22" \
      netforge-sshd /usr/sbin/sshd -D -e -f /etc/ssh/sshd_legacy_config >/dev/null
    echo "SSH test server on 127.0.0.1:$PORT (user netforge / netforge-test)"
    echo "Legacy-only SSH server on 127.0.0.1:$LEGACY_PORT (same credentials)"
    echo "Keys: $KEYS/id_plain (no passphrase), $KEYS/id_pass (passphrase: test-passphrase)"
    ;;
  stop)
    docker rm -f "$NAME" "$LEGACY_NAME" >/dev/null && echo "stopped"
    ;;
  rotate-keys)
    # New host keys, then SIGHUP: sshd re-executes itself and loads them.
    docker exec "$NAME" sh -c 'rm -f /etc/ssh/ssh_host_* && ssh-keygen -A && kill -HUP 1'
    echo "host keys of $NAME regenerated"
    ;;
  *)
    echo "usage: $0 [start|stop|rotate-keys]" >&2
    exit 1
    ;;
esac
