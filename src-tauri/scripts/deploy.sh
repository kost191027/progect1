#!/bin/bash
set -e

# RKN (Recursive Kinetic Network) - Stealth Gateway Deploy Script
# Выполняется сервером по SSH

echo "[INFO] Starting RKN Gateway deployment..."

# 1. Проверяем наличие Docker
if ! command -v docker &> /dev/null; then
    echo "[INFO] Docker not found. Installing..."
    curl -fsSL https://get.docker.com -o get-docker.sh
    sh get-docker.sh
    rm get-docker.sh
else
    echo "[INFO] Docker is already installed."
fi

# 2. Зачистка прошлого окружения (Clean Slate Rule)
echo "[INFO] Cleaning up old instances..."
docker rm -f sys-network-helper || true
docker network prune -f || true

# 3. Создание директорий (JSON конфиг загружается по SSH)
echo "[INFO] Setting up config directories..."
mkdir -p /opt/rkn

# 4. Запуск контейнера sing-box (Stealth mode - sys-network-helper)
echo "[INFO] Starting core container..."
docker run -d --name sys-network-helper \
    --network host \
    -v /opt/rkn/config.json:/etc/sing-box/config.json \
    --restart always \
    ghcr.io/sagernet/sing-box:latest run -c /etc/sing-box/config.json

# 5. Простая проверка
if [ "$(docker inspect -f '{{.State.Running}}' sys-network-helper)" = "true" ]; then
    echo "[SUCCESS] RKN Deploy Script Finished & Container is UP!"
else
    echo "[ERROR] Container failed to start. Rollback needed."
    exit 1
fi
