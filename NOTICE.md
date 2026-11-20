# Credits and provenance

OpenUp builds on two earlier projects. This file records who did what, so the credit stays attached to the code.

## Garvit Singla: `ccp` (original)

<https://github.com/garvittsingla/ccp>

The original project: the initial Rust command-line tool, and the first GitHub release workflow and install script (14 November 2026).

## shivammahajan00 (GitHub: smresponsibilities): `ccp` (fork)

<https://github.com/smresponsibilities/ccp>

The fork that turned the original into a working tool (14-16 November 2026). Its commits introduced:

- Firefox CaptiveDetect-based dynamic gateway discovery and the zero-lag Windows probe pipeline
- Palo Alto (`/php/uid.php`) and Sophos/Cyberoam (`:8090/login.xml`) login handling, URL-encoding of credentials, and RFC1918 target validation
- the background daemon: atomic state file, single-instance lock, live credential reload, the bad-password circuit breaker, and boot autostart (`--now`)
- the original README, documentation and CI

## OpenUp, by Sehjal Saxena (2026)

A substantial rework of the above, including:

- a new name, command set and terminal dashboard
- logins that are verified (the internet must really come up), with a persisted "accepted but still locked" circuit breaker
- a trusted-portal list so credentials are only sent to portals the user approved
- opt-in, allow-listed Wi-Fi reconnect; a real `flock`-based instance lock; wake-from-sleep detection
- private credential storage, `openup doctor`, a rotating log, checksum-verified installers and pinned CI
- a much larger test suite, including end-to-end tests against a mock portal and the real binary

## History

The git history of this repository keeps the original commits and their authorship.

## License status

As of 19 November 2026, neither upstream repository contains a license file. Without one, the inherited portions remain under their authors' default copyright.
