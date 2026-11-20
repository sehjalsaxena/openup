//! Command-line front end: argument dispatch and the non-dashboard commands.

use openup::brand;
use openup::config::{self, Settings};
use openup::connect::{self, Outcome};
use openup::daemon;
use openup::instance;
use openup::logger;
use openup::probe::{self, ProbeResult};
use openup::state::DaemonState;
use openup::trust::{self, TrustStore};
use openup::tui;
use std::io::{self, IsTerminal, Write};
use std::path::Path;

pub type CliResult = Result<(), Box<dyn std::error::Error>>;

const USAGE: &str = "\
openup - opens campus captive portals for you

Usage:
  openup                        Open the dashboard (needs a terminal)
  openup now                    Sign in now
  openup setup                  Guided first-run setup
  openup creds                  Save or replace your username and password
  openup status                 Show what OpenUp is doing
  openup doctor                 Check everything and explain what's wrong

  openup keeper start | stop    Run OpenUp in the background
  openup keeper run [--once] [--dry-run]
                                Run the keeper in the foreground
  openup startup on [--now]     Start the keeper at every login (off: openup startup off)

  openup portals                List portals that may receive your password
  openup portals trust [HOST]   Allow a portal (no HOST: the one the keeper blocked)
  openup portals forget HOST    Stop trusting a portal

  openup wifi add|remove SSID   Let OpenUp rejoin this Wi-Fi network by itself
  openup wifi list | clear      Show / clear that list (empty = OpenUp never touches Wi-Fi)

  openup log [-n N]             Show the last N log lines (default 30)
  openup --version | --help";

pub fn run(args: Vec<String>) -> CliResult {
    let dir = config::config_dir();
    let rest = &args[args.len().min(1)..];

    match args.first().map(String::as_str) {
        None => {
            if is_interactive() {
                tui::run(&dir)?;
                Ok(())
            } else {
                println!("{USAGE}");
                Ok(())
            }
        }
        Some("now") => cmd_open(&dir),
        Some("setup") => cmd_setup(&dir),
        Some("creds") => cmd_creds(&dir),
        Some("status") => {
            daemon::print_status(&dir);
            Ok(())
        }
        Some("doctor") => cmd_doctor(&dir),
        Some("keeper") => cmd_keeper(&dir, rest),
        Some("startup") => cmd_startup(&dir, rest),
        Some("portals") => cmd_portals(&dir, rest),
        Some("wifi") => cmd_wifi(&dir, rest),
        Some("log") => cmd_log(&dir, rest),
        Some("--version" | "-V" | "version") => {
            println!("{} {}", brand::APP_NAME, env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("--help" | "-h" | "help") => {
            println!("{USAGE}");
            Ok(())
        }
        Some(other) => {
            eprintln!("{USAGE}");
            Err(format!("Unknown command '{other}'.").into())
        }
    }
}

fn is_interactive() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

// ---------------------------------------------------------------------------
// Prompts
// ---------------------------------------------------------------------------

fn ask(prompt: &str) -> io::Result<String> {
    print!("{prompt}");
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

fn confirm(question: &str, default_yes: bool) -> bool {
    let hint = if default_yes { "[Y/n]" } else { "[y/N]" };
    match ask(&format!("{question} {hint}: ")) {
        Ok(a) if a.is_empty() => default_yes,
        Ok(a) => matches!(a.to_lowercase().as_str(), "y" | "yes"),
        Err(_) => false,
    }
}

// ---------------------------------------------------------------------------
// open / creds / setup
// ---------------------------------------------------------------------------

fn approve_on_terminal(host: &str) -> bool {
    if !is_interactive() {
        return false;
    }
    println!();
    println!("The login portal at {host} is not in your trusted list.");
    println!(
        "Only continue if this is your campus network: your username and password will be sent to it."
    );
    confirm(&format!("Trust {host} and sign in?"), false)
}

fn cmd_open(dir: &Path) -> CliResult {
    match connect::open_now(dir, &|m| println!("{m}"), &approve_on_terminal)? {
        Outcome::AlreadyOnline => println!("Already online. Nothing to do."),
        Outcome::LoggedIn { gateway, .. } => {
            println!("Signed in via {gateway}. The internet is reachable.")
        }
    }
    Ok(())
}

fn prompt_and_save_credentials(dir: &Path) -> CliResult {
    let username = ask("Username: ")?;
    let password = rpassword::prompt_password("Password: ")?;

    if username.is_empty() || password.is_empty() {
        return Err("Username and password cannot be empty.".into());
    }

    config::save_credentials(dir, &username, &password)?;
    // Resume the keeper if it had halted on bad credentials.
    DaemonState::reset_circuit_breaker(&dir.join("state.json"));
    Ok(())
}

fn cmd_creds(dir: &Path) -> CliResult {
    prompt_and_save_credentials(dir)?;
    println!("Credentials saved.");
    if TrustStore::open(dir).hosts().is_empty() {
        println!(
            "Next: connect to your campus Wi-Fi and run `openup now`. It will ask you to approve your login portal."
        );
    }
    Ok(())
}

fn cmd_setup(dir: &Path) -> CliResult {
    if !is_interactive() {
        return Err("`openup setup` needs an interactive terminal.".into());
    }

    println!("{} setup", brand::DISPLAY_NAME);
    println!("{}\n", brand::TAGLINE);

    // 1. Credentials
    println!("Step 1/4: your campus login");
    match config::load_credentials(dir) {
        Ok(existing) => {
            println!("Credentials are already saved for '{}'.", existing.username);
            if confirm("Replace them?", false) {
                prompt_and_save_credentials(dir)?;
                println!("Saved.");
            }
        }
        Err(_) => {
            prompt_and_save_credentials(dir)?;
            println!("Saved.");
        }
    }

    // 2. Portal
    println!("\nStep 2/4: your login portal");
    let client = probe::build_fast_client(false)?;
    match probe::probe_network(&client, connect::cached_gateway(dir).as_deref()) {
        ProbeResult::LockedPortal { gateway_base, .. } => {
            println!("Found a portal at {gateway_base}.");
            if confirm("Sign in now (this also lets you approve the portal)?", true) {
                match connect::open_now(dir, &|m| println!("  {m}"), &approve_on_terminal) {
                    Ok(Outcome::LoggedIn { .. }) => println!("Signed in."),
                    Ok(Outcome::AlreadyOnline) => println!("Already online."),
                    Err(e) => println!("Could not sign in yet: {e}"),
                }
            }
        }
        ProbeResult::OnlineUnlocked => {
            println!(
                "You're online without a portal here. Run `openup now` on campus Wi-Fi to approve your portal."
            );
        }
        _ => {
            println!(
                "No network right now. Run `openup now` on campus Wi-Fi to approve your portal."
            );
        }
    }

    // 3. Wi-Fi
    println!("\nStep 3/4: Wi-Fi (optional)");
    println!("OpenUp can rejoin your campus Wi-Fi by itself, but only networks you list.");
    let answer = ask("Network name(s), comma-separated (blank to skip): ")?;
    if !answer.is_empty() {
        let mut settings = Settings::load(dir);
        for ssid in answer.split(',') {
            settings.add_ssid(ssid);
        }
        settings.save(dir)?;
        println!("Wi-Fi allow-list: {}", settings.wifi_ssids.join(", "));
    }

    // 4. Keeper
    println!("\nStep 4/4: stay signed in");
    if confirm("Start the keeper now and at every login?", true) {
        daemon::set_autostart(dir, true)?;
        println!("{}", daemon::start_background_daemon(dir)?);
    }

    println!(
        "\nAll set. Run `openup` for the dashboard, or `openup doctor` if something seems off."
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// keeper / startup
// ---------------------------------------------------------------------------

fn cmd_keeper(dir: &Path, args: &[String]) -> CliResult {
    match args.first().map(String::as_str) {
        Some("start") => {
            config::load_credentials(dir)?;
            println!("{}", daemon::start_background_daemon(dir)?);
            Ok(())
        }
        Some("stop") => {
            println!("{}", daemon::stop_daemon(dir)?);
            Ok(())
        }
        Some("run") => {
            let creds = config::load_credentials(dir)?;
            let once = args.iter().any(|a| a == "--once");
            let dry_run = args.iter().any(|a| a == "--dry-run");
            if let Err(e) = daemon::run_daemon(dir, &creds.username, &creds.password, once, dry_run)
            {
                // The keeper's stderr is detached, so leave a trace in the log too.
                logger::log(dir, &format!("Keeper error: {e}"));
                return Err(e.into());
            }
            Ok(())
        }
        _ => {
            eprintln!("Usage: openup keeper start | stop | run [--once] [--dry-run]");
            Err("Invalid keeper arguments.".into())
        }
    }
}

fn cmd_startup(dir: &Path, args: &[String]) -> CliResult {
    match args.first().map(String::as_str) {
        Some("on") => {
            daemon::set_autostart(dir, true)?;
            if args.iter().any(|a| a == "--now" || a == "-n") {
                println!(
                    "{}",
                    daemon::start_background_daemon(dir)
                        .map_err(|e| format!("Could not start the keeper: {e}"))?
                );
            } else {
                println!(
                    "The keeper will start at your next login. Start it now with `openup keeper start`."
                );
            }
            Ok(())
        }
        Some("off") => {
            daemon::set_autostart(dir, false)?;
            Ok(())
        }
        _ => {
            eprintln!("Usage: openup startup on [--now] | off");
            Err("Invalid startup arguments.".into())
        }
    }
}

// ---------------------------------------------------------------------------
// portals / wifi / log
// ---------------------------------------------------------------------------

fn print_portals(dir: &Path, store: &TrustStore) {
    let hosts = store.hosts();
    if hosts.is_empty() {
        println!("No trusted portals yet. Run `openup now` on campus Wi-Fi to approve one.");
    } else {
        println!("Trusted portals (these may receive your password):");
        for h in &hosts {
            println!("  {h}");
        }
    }
    let state = DaemonState::load(&dir.join("state.json"));
    if let Some(p) = state
        .pending_untrusted_gateway
        .filter(|h| !store.is_trusted(h))
    {
        println!("\nBlocked by the keeper: {p}   ->  openup portals trust {p}");
    }
}

fn cmd_portals(dir: &Path, args: &[String]) -> CliResult {
    let store = TrustStore::open(dir);

    match args.first().map(String::as_str) {
        None | Some("list") => print_portals(dir, &store),

        Some("trust") => match args.get(1) {
            Some(target) => {
                let host = trust::host_of(target).ok_or("That doesn't look like a host or URL.")?;
                if store.add(&host)? {
                    println!("Trusted {host}. Your credentials may now be sent to it.");
                } else {
                    println!("{host} was already trusted.");
                }
            }
            None => {
                let state = DaemonState::load(&dir.join("state.json"));
                match state
                    .pending_untrusted_gateway
                    .filter(|h| !store.is_trusted(h))
                {
                    Some(host) => {
                        println!("The keeper found a login portal it did not trust: {host}");
                        println!(
                            "Only approve it if it is really your campus network: your password will be sent to it."
                        );
                        if confirm(&format!("Trust {host}?"), false) {
                            store.add(&host)?;
                            println!(
                                "Trusted {host}. The keeper will pick this up within ~30 seconds."
                            );
                        } else {
                            println!("Not trusted.");
                        }
                    }
                    None => println!("Nothing is waiting for approval."),
                }
            }
        },

        Some("forget") => {
            let Some(target) = args.get(1) else {
                return Err("Usage: openup portals forget HOST".into());
            };
            if store.remove(target)? {
                println!("No longer trusting {target}.");
            } else {
                println!("{target} was not in the trusted list.");
            }
        }

        _ => {
            eprintln!("Usage: openup portals [trust [HOST] | forget HOST]");
            return Err("Invalid portals arguments.".into());
        }
    }
    Ok(())
}

fn cmd_wifi(dir: &Path, args: &[String]) -> CliResult {
    let mut settings = Settings::load(dir);
    let name = args.get(1..).unwrap_or(&[]).join(" ");

    match args.first().map(String::as_str) {
        Some("add") if !name.is_empty() => {
            if settings.add_ssid(&name) {
                settings.save(dir)?;
                println!("OpenUp may now rejoin \"{name}\" by itself.");
            } else {
                println!("\"{name}\" is already in the list.");
            }
        }
        Some("remove") if !name.is_empty() => {
            if settings.remove_ssid(&name) {
                settings.save(dir)?;
                println!("Removed \"{name}\".");
            } else {
                println!("\"{name}\" was not in the list.");
            }
        }
        Some("clear") => {
            settings.wifi_ssids.clear();
            settings.save(dir)?;
            println!("Wi-Fi auto-join is now off.");
        }
        Some("list") | None => {
            if settings.wifi_ssids.is_empty() {
                println!("Wi-Fi auto-join is off. OpenUp never connects to Wi-Fi on its own.");
                println!("Enable it for your campus network: openup wifi add \"<SSID>\"");
            } else {
                println!("OpenUp may rejoin (in this order):");
                for s in &settings.wifi_ssids {
                    println!("  {s}");
                }
            }
        }
        _ => {
            eprintln!("Usage: openup wifi add|remove SSID | openup wifi list | openup wifi clear");
            return Err("Invalid wifi arguments.".into());
        }
    }
    Ok(())
}

fn cmd_log(dir: &Path, args: &[String]) -> CliResult {
    let n = args
        .iter()
        .position(|a| a == "-n")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(30);

    let lines = logger::tail(dir, n);
    if lines.is_empty() {
        println!(
            "No log entries yet. (The keeper writes to {})",
            dir.join(logger::LOG_FILE).display()
        );
    } else {
        for line in lines {
            println!("{line}");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// doctor
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Level {
    Ok,
    Info,
    Warn,
    Fail,
}

fn cmd_doctor(dir: &Path) -> CliResult {
    let mut fails = 0;
    let mut warns = 0;
    let mut report = |level: Level, what: &str, detail: String| {
        let tag = match level {
            Level::Ok => "[ ok ]",
            Level::Info => "[info]",
            Level::Warn => {
                warns += 1;
                "[warn]"
            }
            Level::Fail => {
                fails += 1;
                "[FAIL]"
            }
        };
        println!("{tag} {what:<18} {detail}");
    };

    println!(
        "{} doctor ({})\n",
        brand::DISPLAY_NAME,
        env!("CARGO_PKG_VERSION")
    );

    // Config dir
    report(
        if dir.exists() { Level::Ok } else { Level::Info },
        "Config folder",
        dir.display().to_string(),
    );

    // Credentials
    match config::load_credentials(dir) {
        Ok(c) => report(
            Level::Ok,
            "Credentials",
            format!("saved for '{}'", c.username),
        ),
        Err(e) => report(
            Level::Fail,
            "Credentials",
            format!("{e} Run `openup creds`."),
        ),
    }

    // Keeper
    let lock = dir.join("daemon.lock");
    if instance::is_daemon_running(&lock) {
        let pid = instance::read_lock_pid(&lock)
            .map(|p| format!(" (pid {p})"))
            .unwrap_or_default();
        report(Level::Ok, "Keeper", format!("running{pid}"));
    } else {
        report(
            Level::Warn,
            "Keeper",
            "not running. Start it with `openup keeper start`.".to_string(),
        );
    }

    // Circuit breaker
    let state = DaemonState::load(&dir.join("state.json"));
    if state.halted_bad_credentials {
        report(
            Level::Fail,
            "Auto-login",
            format!("paused: {} Run `openup creds`.", state.last_status_message),
        );
    } else {
        report(Level::Ok, "Auto-login", "active".to_string());
    }

    // Network + portal
    let store = TrustStore::open(dir);
    match probe::build_fast_client(false) {
        Err(e) => report(Level::Fail, "Network probe", e.to_string()),
        Ok(client) => {
            let started = std::time::Instant::now();
            match probe::probe_network(&client, connect::cached_gateway(dir).as_deref()) {
                ProbeResult::OnlineUnlocked => report(
                    Level::Ok,
                    "Network",
                    format!("online ({} ms)", started.elapsed().as_millis()),
                ),
                ProbeResult::LockedPortal {
                    gateway_base,
                    login_xml_url,
                } => {
                    report(
                        Level::Warn,
                        "Network",
                        format!("behind a portal at {gateway_base}; run `openup now`"),
                    );
                    match trust::check(dir, &login_xml_url) {
                        trust::Verdict::Trusted => report(
                            Level::Ok,
                            "Portal trust",
                            "this portal is trusted".to_string(),
                        ),
                        trust::Verdict::Untrusted { host } => report(
                            Level::Warn,
                            "Portal trust",
                            format!("{host} is NOT trusted yet: `openup portals trust {host}`"),
                        ),
                    }
                }
                ProbeResult::InterfaceOffline => {
                    report(Level::Warn, "Network", "no network connection".to_string())
                }
                ProbeResult::ProbeTimeoutOrError(e) => {
                    report(Level::Fail, "Network", format!("probe failed: {e}"))
                }
            }
        }
    }

    let hosts = store.hosts();
    report(
        Level::Info,
        "Trusted portals",
        if hosts.is_empty() {
            "none yet".to_string()
        } else {
            hosts.join(", ")
        },
    );
    let ssids = Settings::load(dir).wifi_ssids;
    report(
        Level::Info,
        "Wi-Fi auto-join",
        if ssids.is_empty() {
            "off".to_string()
        } else {
            ssids.join(", ")
        },
    );

    println!();
    match (fails, warns) {
        (0, 0) => println!("Everything looks good."),
        (0, w) => println!("No failures; {w} thing(s) worth a look."),
        (f, _) => {
            return Err(format!("{f} check(s) failed.").into());
        }
    }
    Ok(())
}
