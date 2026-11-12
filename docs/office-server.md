# Office server

One always-on machine on the office network runs `mattcut-server`. Every MattCut app signs in to
it, and MattCut stops editing when it cannot reach it (off the office Wi-Fi).

## Install

1. Build (or copy) `mattcut-server` to the machine: `cargo build --release -p mattcut-server`
   (the binary is `../mattcraft/target/release/mattcut-server`; on Windows build it on a Windows
   PC, it is `mattcut-server.exe`).
2. Run it once. It creates its config and prints the **office key** and **address**:

   ```
   MattCut office server 0.1.0 — "My Office"
     config:     ~/Library/Application Support/MattCut Server/mattcut-server.json
     projects:   ~/Library/Application Support/MattCut Server/Projects
     address:    192.168.1.20:7700
     office key: 3f9a…
   ```

3. Give the office key to your editors (once). The app finds the server by itself on the same
   network; if it does not, they type the address.
4. Allow the ports through the machine's firewall: **TCP 7700** (app connections) and
   **UDP 7701** (discovery). Do **not** forward these ports on the office router.

## Config (`mattcut-server.json`)

| Key | Meaning |
|---|---|
| `office_name` | Shown in the apps |
| `office_key` | Shared secret. `mattcut-server --new-key` replaces it (everyone signs in again) |
| `port` | TCP port (7700) |
| `storage_dir` | Where shared projects and `.versions` are kept. Put it on a backed-up disk |
| `media_share` | Shown to editors, e.g. `smb://nas.local/Footage` or `\\NAS\Footage` |
| `allowed_users` | Names allowed to sign in; empty = anyone with the key |
| `lease_seconds` | How long an app keeps working without the server (default 300) |
| `allow_public_networks` | `false`: serve private LAN addresses only (keep it false) |

Restart the server after editing the config. `mattcut-server --show-key` prints the key.

## Start it automatically

**macOS (launchd)** — save as `~/Library/LaunchAgents/com.mattcut.server.plist`, then
`launchctl load ~/Library/LaunchAgents/com.mattcut.server.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>com.mattcut.server</string>
  <key>ProgramArguments</key><array><string>/usr/local/bin/mattcut-server</string></array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardOutPath</key><string>/tmp/mattcut-server.log</string>
  <key>StandardErrorPath</key><string>/tmp/mattcut-server.log</string>
</dict></plist>
```

**Windows** — Task Scheduler ▸ Create Task ▸ "Run whether user is logged on or not", trigger
"At startup", action `C:\MattCut\mattcut-server.exe`.

## What it keeps

* `Projects/<name>.fcproj` — the current version of each shared project.
* `Projects/.versions/<name>@<time>.fcproj` — the 20 previous saves of each (restore by copying
  one back over the current file while nobody has the project open).
* `Projects/.authors/` — who saved last.

## Security notes

* Only private network addresses are served (10.x, 172.16–31.x, 192.168.x, link-local).
* The office key is checked in constant time; sessions are random 256-bit tokens that expire with
  the lease.
* Traffic is not encrypted (office LAN only). Use WPA2/WPA3 on the office Wi-Fi.
