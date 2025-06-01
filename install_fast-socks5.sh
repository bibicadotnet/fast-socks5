#!/bin/bash

# Color definitions
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Get terminal width
WIDTH=$(tput cols)

# Function to strip ANSI color codes (for correct text length calculation)
strip_ansi() {
    echo -e "$1" | sed 's/\x1B\[[0-9;]*[JKmsu]//g'
}

# Function to center text considering ANSI codes
center_text() {
    local input="$1"
    local stripped=$(strip_ansi "$input")
    local text_length=${#stripped}
    local padding=$(( (WIDTH - text_length) / 2 ))
    printf "%*s%s\n" "$padding" "" "$input"
}

echo ""
printf "${GREEN}%s${NC}\n" "$(printf '%*s' "$WIDTH" '' | tr ' ' '#')"
center_text "$(printf "${GREEN}🚀 Fast Socks5 Proxy Installer (Auto Mode) 🚀${NC}")"
printf "${GREEN}%s${NC}\n" "$(printf '%*s' "$WIDTH" '' | tr ' ' '#')"
echo ""

# Disable firewall
apt remove iptables-persistent -y >/dev/null 2>&1
ufw disable >/dev/null 2>&1
iptables -F >/dev/null 2>&1

# Create working directory
WORKDIR="$HOME/fast-socks5"
mkdir -p "$WORKDIR"
cd "$WORKDIR" || exit

# Check and install Docker
if ! command -v docker &> /dev/null; then
    echo "Installing Docker..."
    curl -fsSL https://get.docker.com | sh >/dev/null 2>&1
    systemctl enable --now docker >/dev/null 2>&1
fi

# Generate random credentials
PROXY_USER=$(tr -dc A-Za-z0-9 </dev/urandom | head -c 12)
PROXY_PASSWORD=$(tr -dc A-Za-z0-9 </dev/urandom | head -c 16)
HOST_PORT=$((RANDOM%10000+10000))
CONTAINER_PORT=$((RANDOM%10000+10000))

# Get server IP
IP=$(curl -4 -s ip.sb)

# Remove old files if exist
rm -f compose.yml

# Create compose.yml
cat > compose.yml <<EOF
services:
  fast-socks5:
    image: bibica/fast-socks5-server-silent
    container_name: fast-socks5
    restart: always
    ports:
      - "$HOST_PORT:$CONTAINER_PORT/tcp"
      - "$HOST_PORT:$CONTAINER_PORT/udp"
    environment:
      - PROXY_PORT=$CONTAINER_PORT
      - PROXY_USER=$PROXY_USER
      - PROXY_PASSWORD=$PROXY_PASSWORD
      - ALLOW_UDP=true
      - PUBLIC_ADDR=$IP
    logging:
      driver: "none"
EOF

# Stop and remove old container
docker compose down >/dev/null 2>&1
docker rm -f fast-socks5 >/dev/null 2>&1

# Start service
echo "Starting Fast Socks5 service..."
docker compose up -d --build --remove-orphans --force-recreate >/dev/null 2>&1

# Display results
echo ""
printf "${YELLOW}%s${NC}\n" "$(printf '%*s' "$WIDTH" '' | tr ' ' '=')"
echo ""
center_text "$(printf "${GREEN}⚡ Telegram Fast Socks5 Proxy Information ⚡${NC}")"
echo ""
center_text "$(printf "${BLUE}🔗 tg://socks?server=$IP&port=$HOST_PORT&user=$PROXY_USER&pass=$PROXY_PASSWORD${NC}")"
echo ""
printf "${YELLOW}%s${NC}\n" "$(printf '%*s' "$WIDTH" '' | tr ' ' '=')"
echo ""
printf "${GREEN}🚀 Fast Socks5 Proxy Information:${NC}\n"
printf "  🌐 ${WHITE}Server IP:${NC} ${BLUE}%s${NC}\n" "$IP"
printf "  🚪 ${WHITE}Port:${NC} ${BLUE}%s${NC}\n" "$HOST_PORT"
printf "  👤 ${WHITE}Username:${NC} ${BLUE}%s${NC}\n" "$PROXY_USER"
printf "  🔑 ${WHITE}Password:${NC} ${BLUE}%s${NC}\n" "$PROXY_PASSWORD"
printf "  📡 ${WHITE}Protocols:${NC} ${GREEN}TCP ✅${NC} ${GREEN}UDP ✅${NC}\n"
printf "  📝 ${WHITE}Logging:${NC} ${RED}Disabled${NC}\n"
echo ""
printf "${YELLOW}%s${NC}\n" "$(printf '%*s' "$WIDTH" '' | tr ' ' '=')"
echo ""
printf "⚙️ ${WHITE}Configuration directory:${NC} ${YELLOW}%s${NC}\n" "$WORKDIR"
echo ""
