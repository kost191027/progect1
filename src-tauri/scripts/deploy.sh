#!/bin/bash
set -euo pipefail

# RKN (Recursive Kinetic Network) - Stealth Gateway Deploy Script
# Выполняется сервером по SSH

IMAGE="${RKN_IMAGE:?RKN_IMAGE is required}"
CONTAINER_NAME="${RKN_CONTAINER_NAME:?RKN_CONTAINER_NAME is required}"
CONFIG_DIR="/opt/rkn"
ACTIVE_CONFIG="$CONFIG_DIR/config.json"
CANDIDATE_CONFIG="$CONFIG_DIR/config.candidate.json"
BACKUP_CONFIG="$CONFIG_DIR/config.previous.json"
ACTIVE_CONTAINER_FILE="$CONFIG_DIR/container_name"
LEGACY_CONTAINER_NAME="sys-network-helper"

PREVIOUS_CONTAINER=""
ROLLBACK_CONTAINER=""
NEW_CONTAINER_CREATED=0

rollback() {
    local exit_code=$?

    if [ "$exit_code" -eq 0 ]; then
        return
    fi

    echo "[ROLLBACK] Deployment failed. Restoring previous state..."

    if [ "$NEW_CONTAINER_CREATED" -eq 1 ]; then
        docker rm -f "$CONTAINER_NAME" >/dev/null 2>&1 || true
    fi

    if [ -f "$BACKUP_CONFIG" ]; then
        cp "$BACKUP_CONFIG" "$ACTIVE_CONFIG" || true
    fi

    if [ -n "$ROLLBACK_CONTAINER" ] && docker inspect "$ROLLBACK_CONTAINER" >/dev/null 2>&1; then
        docker rename "$ROLLBACK_CONTAINER" "$PREVIOUS_CONTAINER" >/dev/null 2>&1 || true
        docker start "$PREVIOUS_CONTAINER" >/dev/null 2>&1 || true
        echo "$PREVIOUS_CONTAINER" > "$ACTIVE_CONTAINER_FILE" || true
    fi

    rm -f "$CANDIDATE_CONFIG"

    exit "$exit_code"
}

trap rollback EXIT

echo "[INFO] Starting RKN Gateway deployment..."
echo "[INFO] Using pinned image: $IMAGE"
echo "[INFO] Target container name: $CONTAINER_NAME"

# 1. Проверяем наличие Docker
if ! command -v docker &> /dev/null; then
    echo "[INFO] Docker not found. Installing..."
    curl -fsSL https://get.docker.com -o get-docker.sh
    sh get-docker.sh
    rm get-docker.sh
else
    echo "[INFO] Docker is already installed."
fi

# 2. Подготовка директорий и pull pinned image
echo "[INFO] Setting up config directories..."
mkdir -p "$CONFIG_DIR"

if [ ! -f "$CANDIDATE_CONFIG" ]; then
    echo "[ERROR] Candidate config was not uploaded."
    exit 1
fi

echo "[INFO] Pulling pinned sing-box image..."
docker pull "$IMAGE"

if [ -f "$ACTIVE_CONTAINER_FILE" ]; then
    PREVIOUS_CONTAINER="$(cat "$ACTIVE_CONTAINER_FILE" 2>/dev/null || true)"
fi

if [ -z "$PREVIOUS_CONTAINER" ] && docker inspect "$LEGACY_CONTAINER_NAME" >/dev/null 2>&1; then
    PREVIOUS_CONTAINER="$LEGACY_CONTAINER_NAME"
fi

if [ -n "$PREVIOUS_CONTAINER" ] && ! docker inspect "$PREVIOUS_CONTAINER" >/dev/null 2>&1; then
    PREVIOUS_CONTAINER=""
fi

if [ -f "$ACTIVE_CONFIG" ]; then
    cp "$ACTIVE_CONFIG" "$BACKUP_CONFIG"
fi

if [ -n "$PREVIOUS_CONTAINER" ]; then
    ROLLBACK_CONTAINER="${PREVIOUS_CONTAINER}-rollback"
    echo "[INFO] Preparing rollback snapshot from container $PREVIOUS_CONTAINER..."
    docker rm -f "$ROLLBACK_CONTAINER" >/dev/null 2>&1 || true
    docker stop "$PREVIOUS_CONTAINER" >/dev/null 2>&1 || true
    docker rename "$PREVIOUS_CONTAINER" "$ROLLBACK_CONTAINER"
fi

# 3. Активация новой конфигурации
mv "$CANDIDATE_CONFIG" "$ACTIVE_CONFIG"

# 4. Запуск нового контейнера sing-box
echo "[INFO] Starting core container..."
docker rm -f "$CONTAINER_NAME" >/dev/null 2>&1 || true
docker run -d --name "$CONTAINER_NAME" \
    --network host \
    -v "$ACTIVE_CONFIG:/etc/sing-box/config.json" \
    --restart always \
    "$IMAGE" run -c /etc/sing-box/config.json
NEW_CONTAINER_CREATED=1

# 5. Простая проверка
if [ "$(docker inspect -f '{{.State.Running}}' "$CONTAINER_NAME")" = "true" ]; then
    echo "$CONTAINER_NAME" > "$ACTIVE_CONTAINER_FILE"

    if [ -n "$ROLLBACK_CONTAINER" ] && docker inspect "$ROLLBACK_CONTAINER" >/dev/null 2>&1; then
        echo "[INFO] Removing old rollback container snapshot..."
        docker rm -f "$ROLLBACK_CONTAINER" >/dev/null 2>&1 || true
    fi

    NEW_CONTAINER_CREATED=0
    trap - EXIT
    echo "[SUCCESS] RKN Deploy Script Finished & Container is UP!"
else
    echo "[ERROR] Container failed to start. Rollback needed."
    exit 1
fi
