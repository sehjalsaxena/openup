# OpenUp

> Open up campus captive portals: signs you in safely, and keeps you signed in.

Walk from your hostel to a classroom and your laptop hops to a new access point, drops the connection, and shows a login page again. **OpenUp** signs you back in, quickly and safely, and shows you what it's doing in a terminal dashboard.

```text
╭ OPENUP ────────────────────────────────────────────────────────────────────────────────── v0.1.0 ╮
│* ONLINE    * Keeper running                                                                      │
╰──────────────────────────────────────────────────────────────────────────────────────────────────╯
╭ Status ─────────────────────────────────────────────╮╭ Trusted portals ──────────────────────────╮
│Internet    online  (172.16.4.21)                    ││> 172.16.68.6                              │
│Keeper      running  (pid 4242)                      ││  10.20.0.1                                │
│Account     alice                                    ││                                           │
│Gateway     http://172.16.68.6:8090/login.xml        │╰───────────────────────────────────────────╯
│Logins      12  (last 2023-11-14 22:13:20Z)          │╭ Wi-Fi it may rejoin ──────────────────────╮
│Autopilot   active                                   ││> College_WiFi                             │
│Last event  Online and unlocked                      ││                                           │
╰─────────────────────────────────────────────────────╯╰───────────────────────────────────────────╯
╭ Activity ────────────────────────────────────────────────────────────────────────────────────────╮
│09:01:02 Keeper started (pid 4242)                                                                │
│09:01:05 Login verified at http://172.16.68.6:8090                                                │
│10:30:00 Blocked login: portal host 10.9.9.9 is not trusted                                       │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
╰──────────────────────────────────────────────────────────────────────────────────────────────────╯
 o sign in  k keeper  t trust  c login  w +wifi  x remove  Tab focus  ? help  q quit
```
*That is the real dashboard, rendered by the same code you run (`cargo test render_readme_screenshot -- --ignored --nocapture` regenerates it).*

## What it does

- **Dashboard.** Run `openup` for a live view: network state, keeper status, trusted portals, Wi-Fi list and an activity log. Sign in, start the keeper, approve a portal or save your login with one key.
- **Keeper.** A background process that notices when you roam or wake from sleep and signs you back in, within seconds.
- **Verified sign-ins.** A portal page that merely *says* "success" is not enough. OpenUp checks that the internet really works before it counts a login.
- **Safe by default.** Your password only goes to portals you approved. If the portal rejects it (or says "logged in" but stays locked), OpenUp stops instead of hammering your account into a lockout.
- **Knows your portals.** Detects Palo Alto (`/php/uid.php`) and Sophos/Cyberoam (`:8090/login.xml`) portals from the network's own redirect. No gateway addresses are hardcoded.
- **Runs natively on Windows, macOS and Linux.** No WSL, no console pop-ups.

## Quick start

```text
openup setup        # guided: your login, your portal, your Wi-Fi, the keeper
openup              # open the dashboard
```

Prefer individual steps?

```text
openup creds                # save your username and password
openup now                 # on campus Wi-Fi: sign in (asks you to approve the portal once)
openup startup on --now     # start the keeper now and at every login
```

If something seems off: `openup doctor` checks everything and says what to do.

### Install

Prebuilt binaries are published by the release workflow when a `v*` tag is pushed.

```powershell
# Windows (PowerShell)
irm https://raw.githubusercontent.com/sehjalsaxena/openup/main/install.ps1 | iex
```
```bash
# macOS / Linux / WSL
curl -fsSL https://raw.githubusercontent.com/sehjalsaxena/openup/main/install.sh | bash
```

Package managers:
```bash
# macOS / Linux (Homebrew)
brew install sehjalsaxena/tap/openup

# Windows (Scoop)
scoop install https://raw.githubusercontent.com/sehjalsaxena/openup/main/packaging/scoop/openup.json

# Arch Linux (AUR)
yay -S openup-bin
```

The installers download the `.sha256` published next to each release asset and **refuse to install on a mismatch** (`OPENUP_SKIP_VERIFY=1` overrides). That catches corrupted or tampered downloads; it can't help if the release itself were compromised.

Or build it yourself (Rust 1.89+):

```bash
git clone https://github.com/sehjalsaxena/openup.git
cd openup
cargo build --release      # binary: target/release/openup
```

## The dashboard

Run `openup` in a terminal at least 60x18.

| Key | Action |
| :-- | :-- |
| `o` | Sign in now (asks first if the portal is new) |
| `k` | Start or stop the background keeper |
| `t` | Trust the portal that was blocked, then sign in |
| `c` | Save or replace your username and password (the password is masked) |
| `w` | Add a Wi-Fi network OpenUp may rejoin by itself |
| `x` / `Del` | Remove the selected portal or network (asks first) |
| `Tab` | Switch between the portals and Wi-Fi lists; `Up`/`Down` move |
| `r` | Refresh now |
| `?` | Help |
| `q` / `Ctrl+C` | Quit (the keeper keeps running) |

Signing in, probing the network and starting the keeper happen in the background, so the screen never freezes while a portal is slow.

## Commands

| Command | What it does |
| :-- | :-- |
| `openup` | Open the dashboard (prints help when there is no terminal) |
| `openup now` | Sign in now, verify, and exit |
| `openup setup` | Guided first-run setup |
| `openup creds` | Save or replace your username and password (also resumes a paused keeper) |
| `openup status` | Show what OpenUp is doing |
| `openup doctor` | Check credentials, keeper, network, portal trust and Wi-Fi; exits non-zero on failures |
| `openup keeper start` / `stop` | Run the keeper in the background / stop it |
| `openup keeper run [--once] [--dry-run]` | Run the keeper in the foreground (`--dry-run` probes without sending credentials) |
| `openup startup on [--now]` / `off` | Start the keeper at every login (Windows Run key, macOS LaunchAgent, Linux systemd user unit) |
| `openup portals` | List portals that may receive your password, and any the keeper blocked |
| `openup portals trust [HOST]` | Allow a portal (no HOST: the one the keeper blocked) |
| `openup portals forget HOST` | Stop trusting a portal |
| `openup wifi add SSID` / `remove SSID` / `list` / `clear` | The only Wi-Fi networks OpenUp may rejoin by itself |
| `openup notify [on\|off\|test]` | Manage or preview desktop notifications |
| `openup log [-n N]` | Last N (default 30) log lines |
| `openup --version` / `--help` | |

## How the keeper works

1. **Roaming:** every 2 seconds it checks your local IPv4 route (a local socket check, no packets). When the address changes between hostel and college, it probes and signs in immediately.
2. **Wake from sleep:** if more than about 10 seconds of wall-clock time passed between two checks, it re-probes straight away.
3. **Keep-alive:** while online it re-checks every 30 seconds.
4. **Portal detection:** a plain-HTTP request to Firefox's captive-portal endpoint; the redirect reveals the login page.
5. **Verified sign-in:** after submitting your login it re-probes for up to about 4 seconds.
6. **Circuit breaker:** it halts if the portal rejects your password, or if the portal says "logged in" three times but the internet stays locked (the count survives restarts). `openup creds` or the dashboard's `c` resumes it. Other failures back off 5 s, 10 s, 20 s ... up to 5 minutes.
7. **Wi-Fi (opt-in):** only with `openup wifi add "<SSID>"`: when there is no network it tries those networks about every 10 s, backing off to 2 min. It never turns the Wi-Fi radio on and never joins a network you didn't list.
8. **Log:** everything is recorded in `openup.log` (rotating). Passwords are never logged.

## Security model

OpenUp finds your login page from whatever redirect the network sends. Without a safeguard, *any* private-network portal could receive your password.

- **Trusted portals only.** Credentials only go to hosts in `trusted_gateways.txt`. The first time you sign in interactively on a new portal, OpenUp asks. The keeper never asks: it records the blocked host, shows it in `openup status` and the dashboard, and waits for you to approve it.
- **Roaming between two campus sites** means approving each portal once.
- **Storage.** Credentials live in `~/.config/openup/credentials.txt` (folder `0700`, file `0600` on Unix; loose permissions are repaired when read; on Windows they rely on your profile folder's permissions). They are plain text, so use a password you don't reuse elsewhere.
- **Escape hatch:** `OPENUP_TRUST_ANY=1` sends credentials to any portal. Not recommended.
- **Known limit.** Campus portals use self-signed certificates, so certificate checks are off for the portal connection, and Sophos logins are plain HTTP by design. An attacker who can impersonate a gateway you *already trust* (for example by ARP spoofing on the same LAN) could still capture your login. Trust lists stop the far more common case: signing in to the wrong network.

Files live in `~/.config/openup` (override with `OPENUP_CONFIG_DIR`): `credentials.txt`, `settings.json`, `trusted_gateways.txt`, `state.json`, `daemon.lock`, `openup.log`.

## Developing

```bash
cargo test --all-targets            # 123 tests
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

- `src/connect.rs`: the "get me online" flow shared by the CLI and the dashboard (progress and trust questions are callbacks, so it is testable).
- `src/tui/`: `app.rs` (pure state and key handling), `ui.rs` (rendering), `mod.rs` (event loop, background threads).
- `src/daemon.rs`: the keeper loop. `src/probe.rs`: portal detection and login. `src/trust.rs`, `src/config.rs`, `src/wifi.rs`, `src/instance.rs`, `src/logger.rs`, `src/state.rs`.
- Tests use a small mock captive portal (`tests/common`) and also run the real binary (`tests/cli_integration.rs`).
- All naming lives in `src/brand.rs`.

Environment variables: `OPENUP_CONFIG_DIR`, `OPENUP_PROBE_URL` (override the connectivity probe, used by the tests), `OPENUP_TRUST_ANY`. Installers: `OPENUP_VERSION`, `OPENUP_REPO`, `OPENUP_DOWNLOAD_BASE`, `OPENUP_SKIP_VERIFY`, `INSTALL_DIR`.

## Uninstall

```powershell
# Windows (PowerShell)
openup startup off
openup keeper stop
Remove-Item -Recurse -Force "$env:USERPROFILE\.openup", "$env:USERPROFILE\.config\openup" -ErrorAction SilentlyContinue
```
```bash
# macOS / Linux
openup startup off
openup keeper stop
rm -rf ~/.local/bin/openup ~/.config/openup ~/Library/LaunchAgents/io.github.sehjalsaxena.openup.plist ~/.config/systemd/user/openup.service
```

## Credits

OpenUp is a substantial rework by **Sehjal Saxena** of two earlier projects, and it would not exist without them:

- **Garvit Singla** ([garvittsingla/ccp](https://github.com/garvittsingla/ccp)): the original `ccp` project, its first release workflow and install script.
- **shivammahajan00** ([smresponsibilities/ccp](https://github.com/smresponsibilities/ccp)): the fork that built the Firefox-CaptiveDetect gateway discovery and zero-lag Windows pipeline, Palo Alto and Sophos login handling, the background daemon with atomic state, instance lock and circuit breaker, autostart, and the original docs and CI.

The git history keeps their original commits. Details are in [NOTICE.md](NOTICE.md).

## License

See [NOTICE.md](NOTICE.md).
