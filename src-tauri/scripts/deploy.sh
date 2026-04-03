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
docker rm -f rkn-sing-box || true
docker network prune -f || true

# 3. Создание директорий и базового конфигурационного файла (VLESS Reality / ShadowTLS)
echo "[INFO] Setting up config directories..."
mkdir -p /opt/rkn

# Временно кладем заглушку (в будущем Rust-программа будет сама генерировать сюда полновесный JSON)
cat << 'EOF' > /opt/rkn/config.json
{
  "log": {
    "disabled": false,
    "level": "info",
    "timestamp": true
  },
  "inbounds": [
    {
      "type": "shadowtls",
      "tag": "in-stls",
      "listen": "::",
      "listen_port": 443,
      "version": 3,
      "server_name": "www.microsoft.com",
      "handshake": {
        "server": "104.21.35.210",
        "server_port": 443
      },
      "detour": "in-reality"
    },
    {
      "type": "vless",
      "tag": "in-reality",
      "listen": "127.0.0.1",
      "listen_port": 8443,
      "users": []
    }
  ],
  "outbounds": [
    {
      "type": "direct",
      "tag": "direct"
    }
  ]
}
EOF

# 4. Запуск контейнера sing-box с пробросом портов и монтированием конфига
echo "[INFO] Starting core container..."
docker run -d --name rkn-sing-box \
    --network host \
    -v /opt/rkn/config.json:/etc/sing-box/config.json \
    --restart always \
    ghcr.io/sagernet/sing-box:latest run -c /etc/sing-box/config.json

# 5. Простая проверка
if [ "$(docker inspect -f '{{.State.Running}}' rkn-sing-box)" = "true" ]; then
    echo "[SUCCESS] RKN Deploy Script Finished & Container is UP!"
else
    echo "[ERROR] Container failed to start. Rollback needed."
    exit 1
fi
