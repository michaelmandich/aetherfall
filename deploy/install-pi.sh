#!/usr/bin/env bash
# Install (or update) the Aetherfall server on this Raspberry Pi, from a
# checkout of the repository. Builds natively (see docs/multiplayer.md for
# the time it takes and for cross-compiling instead), then puts the binary
# and the content under /opt/aetherfall and installs the systemd unit.
#
#   sudo apt install -y build-essential pkg-config libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev
#   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh     # once
#   ./deploy/install-pi.sh
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build --profile server -p af_server

sudo useradd --system --home /opt/aetherfall --shell /usr/sbin/nologin aetherfall 2>/dev/null || true
sudo mkdir -p /opt/aetherfall/assets
sudo install -m 755 target/server/af_server /opt/aetherfall/af_server
sudo rm -rf /opt/aetherfall/assets/data
sudo cp -r assets/data /opt/aetherfall/assets/data
sudo chown -R aetherfall:aetherfall /opt/aetherfall
sudo install -m 644 deploy/aetherfall-server.service /etc/systemd/system/aetherfall-server.service
if [ ! -f /etc/aetherfall/server.env ]; then
  sudo mkdir -p /etc/aetherfall
  sudo install -m 600 deploy/server.env.example /etc/aetherfall/server.env
  echo "Edit /etc/aetherfall/server.env (Supabase keys), then: sudo systemctl enable --now aetherfall-server"
fi
sudo systemctl daemon-reload
if systemctl is-active --quiet aetherfall-server; then
  sudo systemctl restart aetherfall-server
  echo "aetherfall-server restarted"
fi
