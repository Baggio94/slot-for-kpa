# slot-relay

The small server that lets two slot. consoles play Multiplayer over the
Internet. The host opens a room and gets a four-digit code; the friend types
the code; the relay then copies bytes between the two consoles. Both consoles
only dial *out*, so home routers, CGNAT and mobile data need no setup.

The relay never looks inside the link: ROM, core and BIOS checks, the
cartridge exchange and the game itself are slot.'s own KMLB protocol, exactly
as on Wi-Fi. Protocol details: `crates/slot-android/src/link_relay.rs`.

## Install on a server (one command)

On the server (Oracle Cloud VM, Raspberry Pi, any Linux VPS), over SSH:

```sh
curl -fsSL https://raw.githubusercontent.com/Baggio94/slot-for-android/main/tools/slot-relay/install.sh | sudo bash
```

It builds the relay, installs it as a service on TCP 7212, opens the port in
the server's firewall and prints the public address. Run it again to update.
On a cloud VM, also open TCP 7212 in the provider's network rules (Oracle:
the VCN's Security List, Ingress Rule, source 0.0.0.0/0, destination port
7212).

## Run it by hand

Standard library only, no dependencies.

```sh
cd tools/slot-relay
cargo build --release
./target/release/slot-relay 0.0.0.0:7212
```

Open TCP port 7212 in the server's firewall. Any small Linux VPS will do: a
link is a few kilobytes a second. Pick one close to the players (latency is
what matters, not bandwidth).

### As a service (systemd)

```ini
# /etc/systemd/system/slot-relay.service
[Unit]
Description=slot. Internet Multiplayer relay
After=network-online.target

[Service]
ExecStart=/usr/local/bin/slot-relay 0.0.0.0:7212
Restart=always
DynamicUser=yes
LimitNOFILE=16384

[Install]
WantedBy=multi-user.target
```

```sh
sudo cp target/release/slot-relay /usr/local/bin/
sudo systemctl enable --now slot-relay
journalctl -u slot-relay -f
```

## Point the app at it

The APK build reads `SLOT_RELAY_ADDR` (`host` or `host:port`, port defaults
to 7212). In GitHub: *Settings → Secrets and variables → Actions →
Variables → New repository variable* `SLOT_RELAY_ADDR`, then re-run the build.

## Limits

Defaults in `Limits::default()` (`src/main.rs`): a room waits 15 minutes for
its friend, a linked pair silent for 30 minutes is closed, 4 open rooms per
address, 300 rooms, and 12 wrong codes per address per 10 minutes before
that address is refused for a while.

## Test

```sh
cargo test
```

The tests include the app's own client (`link_relay.rs`), so both ends are
checked against each other.
