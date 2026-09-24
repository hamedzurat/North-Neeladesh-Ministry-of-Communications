# Deploy the Cabinet Frontend over Wi‑Fi

The Raspberry Pi Cabinet Frontend and the laptop backend communicate over the
same Wi‑Fi/LAN network. The backend remains authoritative; the Pi only reads
hardware, sends input snapshots, captures voice, and plays returned audio.

## 1. Find the laptop address

Find the laptop's address on the Wi‑Fi network. Use that address as
`BACKEND_HOST` below, not the Pi's address.

The backend must listen on LAN interfaces:

```sh
just backend-debug address=0.0.0.0:7878 voice_address=0.0.0.0:7879
```

Allow TCP port `7878` and UDP port `7879` through the laptop firewall if one is
enabled. Keep the debug command and dashboard ports (`7880`/`7881`) restricted
to the laptop unless remote debugging is intentional.

## 2. Deploy the Python bundle

Set the Pi SSH target and the laptop Wi‑Fi address:

```sh
PI_HOST=taki@192.168.1.34 \
BACKEND_HOST=192.168.1.8 \
./scripts/deploy_cabinet_frontend.sh
```

Replace both values for the local network. The deployment copies the Python
project, setup script, and authored story audio under `assets/stories` to the Pi,
then prints the exact setup command. Run `just story-audio` first if the
generated recordings are missing.

## 3. Install and configure the Pi service

Run the printed command, or run it directly:

```sh
ssh taki@192.168.1.34 \
  'NN_BACKEND_HOST=192.168.1.8 \
   /home/taki/Desktop/cabinet-frontend/scripts/setup_cabinet_frontend_pi.sh'
```

The setup script verifies the authored audio bundle, installs the locked Python environment, checks the Pi's
`pw-record`/`aplay` tools, configures GPIO/audio permissions, and writes one
systemd service:

```text
north-neeladesh-cabinet-frontend.service
```

`NN_BACKEND_HOST` configures both backend channels automatically:

```text
game:  192.168.1.8:7878   TCP
voice: 192.168.1.8:7879   UDP
```

Use these variables only when the ports or hosts must differ:

```sh
NN_BACKEND_ADDRESS=192.168.1.8:7878
NN_VOICE_BACKEND_ADDRESS=192.168.1.8:7879
```

Voice capture, control, transport, and playback run inside the Cabinet
Frontend process. Do not install or start a separate voice-relay service.

## 4. Start and verify

On the Pi:

```sh
sudo systemctl start north-neeladesh-cabinet-frontend.service
journalctl -u north-neeladesh-cabinet-frontend.service -f
```

From the development machine:

```sh
PI_HOST=taki@192.168.1.34 ./scripts/status_pi.sh
```

The logs should show the cabinet frontend connecting to the backend. Pressing
PTT, Police, or EMS should produce the listening/capture path immediately in
the same service. The backend then performs STT, classification/dialogue, and
TTS.

## 5. Wi‑Fi troubleshooting

- Confirm the laptop and Pi are on the same network and can reach each other.
- From the Pi, check TCP connectivity to the laptop's `7878` port.
- Check the laptop firewall for TCP `7878` and UDP `7879`.
- Confirm the backend was started with `0.0.0.0`, not only `127.0.0.1`.
- Inspect `journalctl -u north-neeladesh-cabinet-frontend.service -f`.
- The frontend reconnects after a backend or Wi‑Fi interruption; restart the
  service only when hardware or configuration also changed.

The frontend defaults to localhost for local development. For a Pi deployment,
always set `NN_BACKEND_HOST` during setup so the generated service retains the
correct laptop address across reboots.
