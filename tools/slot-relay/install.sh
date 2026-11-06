#!/usr/bin/env bash
# Install or update slot-relay on a Linux server: Oracle Cloud (Oracle Linux
# or Ubuntu, AMD or ARM), a Raspberry Pi, or any VPS. Run it again to update.
#
#   curl -fsSL https://raw.githubusercontent.com/Baggio94/slot-for-android/main/tools/slot-relay/install.sh | sudo bash
#
# Builds from source on the server (a minute or two), installs a systemd
# service on TCP 7212 and opens that port in the server's own firewall.
# On a cloud VM the port must ALSO be opened in the provider's network rules
# (Oracle: the subnet's Security List), which this script cannot do.
set -euo pipefail

PORT="${SLOT_RELAY_PORT:-7212}"
BRANCH="${SLOT_RELAY_BRANCH:-main}"
REPO="${SLOT_RELAY_REPO:-https://github.com/Baggio94/slot-for-android}"
HOME_DIR=/opt/slot-relay
export RUSTUP_HOME="$HOME_DIR/rustup" CARGO_HOME="$HOME_DIR/cargo"

say() { printf '\n==> %s\n' "$*"; }
[ "$(id -u)" -eq 0 ] || { echo "Run this with sudo." >&2; exit 1; }

say "Installing build tools"
if command -v dnf >/dev/null; then
    dnf install -y -q git gcc curl
elif command -v yum >/dev/null; then
    yum install -y -q git gcc curl
elif command -v apt-get >/dev/null; then
    export DEBIAN_FRONTEND=noninteractive
    apt-get update -qq
    apt-get install -y -qq git gcc libc6-dev curl
else
    echo "Unknown Linux: install git, gcc and curl, then run this again." >&2; exit 1
fi

say "Installing Rust (private to slot-relay, in $HOME_DIR)"
mkdir -p "$HOME_DIR"
if [ ! -x "$CARGO_HOME/bin/cargo" ]; then
    curl -fsSL https://sh.rustup.rs | sh -s -- -y -q --profile minimal \
        --default-toolchain stable --no-modify-path
fi
export PATH="$CARGO_HOME/bin:$PATH"

say "Fetching slot. ($BRANCH)"
if [ -d "$HOME_DIR/src/.git" ]; then
    git -C "$HOME_DIR/src" fetch -q --depth 1 origin "$BRANCH"
    git -C "$HOME_DIR/src" reset -q --hard FETCH_HEAD
else
    rm -rf "$HOME_DIR/src"
    git clone -q --depth 1 --branch "$BRANCH" "$REPO" "$HOME_DIR/src"
fi

say "Building slot-relay"
cd "$HOME_DIR/src/tools/slot-relay"
RUSTUP_TOOLCHAIN=stable cargo build -q --release --locked
install -m 0755 target/release/slot-relay /usr/local/bin/slot-relay

say "Installing the service"
cat > /etc/systemd/system/slot-relay.service <<EOF
[Unit]
Description=slot. Internet Multiplayer relay
After=network-online.target
Wants=network-online.target

[Service]
ExecStart=/usr/local/bin/slot-relay 0.0.0.0:$PORT
Restart=always
RestartSec=2
DynamicUser=yes
LimitNOFILE=16384
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes

[Install]
WantedBy=multi-user.target
EOF
systemctl daemon-reload
systemctl enable -q slot-relay
systemctl restart slot-relay

say "Opening TCP $PORT in this server's firewall"
if command -v firewall-cmd >/dev/null && firewall-cmd --state >/dev/null 2>&1; then
    firewall-cmd -q --permanent --add-port="$PORT/tcp"
    firewall-cmd -q --reload
    echo "firewalld: opened"
elif command -v ufw >/dev/null && ufw status 2>/dev/null | grep -q "Status: active"; then
    ufw allow "$PORT/tcp" >/dev/null
    echo "ufw: opened"
elif command -v iptables >/dev/null && iptables -S INPUT 2>/dev/null | grep -q -- "-j REJECT"; then
    # Oracle's Ubuntu images end INPUT with a REJECT rule: accept before it.
    iptables -C INPUT -p tcp --dport "$PORT" -j ACCEPT 2>/dev/null \
        || iptables -I INPUT 1 -p tcp --dport "$PORT" -j ACCEPT
    if command -v netfilter-persistent >/dev/null; then netfilter-persistent save >/dev/null 2>&1 || true
    elif [ -d /etc/iptables ]; then iptables-save > /etc/iptables/rules.v4; fi
    echo "iptables: opened"
else
    echo "No active firewall found on this server."
fi

say "Checking"
sleep 1
systemctl is-active --quiet slot-relay || { journalctl -u slot-relay -n 20 --no-pager; exit 1; }
reply="$(timeout 3 bash -c "exec 3<>/dev/tcp/127.0.0.1/$PORT; printf 'SLRY\002H' >&3; head -c 1 <&3" 2>/dev/null || true)"
if [ "$reply" = "C" ]; then echo "slot-relay answers on port $PORT."; else
    echo "slot-relay runs but did not answer as expected." >&2; fi
ip="$(curl -fsS -4 --max-time 5 https://ifconfig.me 2>/dev/null || true)"
[ -n "$ip" ] || ip="this server's public IP"

cat <<EOF

Done. slot-relay is running and starts with the server.
  Public address : $ip
  Port           : $PORT (TCP)
  Logs           : journalctl -u slot-relay -f

Still to do:
  1. Cloud VM: open TCP $PORT for 0.0.0.0/0 in the provider's network rules
     (Oracle: Networking > Virtual cloud networks > your VCN > Security
     Lists > Default > Add Ingress Rule, Destination port $PORT).
  2. GitHub: set the Actions variable SLOT_RELAY_ADDR to $ip
     and re-run the APK build.
EOF
