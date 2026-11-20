# Changelog

## 0.1.0: first release as OpenUp

OpenUp continues the `ccp` project (see [NOTICE.md](NOTICE.md)). Everything in `ccp` 0.2.0 carries over, with a new name, new commands and a dashboard.

### New
- **Terminal dashboard** (`openup`): live network, keeper, portals, Wi-Fi and activity log; sign in, start/stop the keeper, approve portals and save your login from the keyboard.
- `openup setup` (guided first run) and `openup doctor` (checks everything, non-zero exit on failures).
- Manual sign-ins are now recorded in the log.
- `src/brand.rs`: all naming in one place.

### Renamed commands

| `ccp` | `openup` |
| :-- | :-- |
| `ccp` | `openup now` (and `openup` opens the dashboard) |
| `ccp set` | `openup creds` |
| `ccp start` / `stop` | `openup keeper start` / `stop` |
| `ccp daemon [--once] [--dry-run]` | `openup keeper run [--once] [--dry-run]` |
| `ccp status` | `openup status` |
| `ccp autostart enable [--now]` / `disable` | `openup startup on [--now]` / `off` |
| `ccp trust [HOST]` / `untrust HOST` / `trust --list` | `openup portals trust [HOST]` / `portals forget HOST` / `portals` |
| `ccp wifi ...` | `openup wifi ...` (unchanged) |
| `ccp logs` | `openup log` |

Files moved from `~/.config/ccp` to `~/.config/openup`; environment variables `CCP_*` became `OPENUP_*`; the log file is `openup.log`. Credentials are not migrated automatically: run `openup setup`.

### Changed
- One config folder (`~/.config/openup`, or `OPENUP_CONFIG_DIR`); the old multi-location lookup is gone.
- Starting and stopping the keeper no longer prints from library code, so the dashboard can use them.
- The status row for the circuit breaker is "Autopilot"; long values are clipped instead of wrapped.
- Installer and release assets are named `openup-<os>-<arch>`.
- Autostart identifiers: Windows value `OpenUp`, macOS label `io.github.sehjalsaxena.openup`, Linux unit `openup.service`.

### Inherited from ccp 0.2.0
Verified logins, trusted-portal guard, persisted circuit breaker, opt-in Wi-Fi, real `flock` lock, wake-from-sleep detection, private credential files, checksum-verified installers, pinned CI.
