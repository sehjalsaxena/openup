//! The terminal dashboard (`openup` with no arguments).
//!
//! * `app`: state and key handling (pure, unit-tested)
//! * `ui`: rendering
//! * this file: the event loop and the side effects. Anything slow (probing the
//!   network, signing in, starting or stopping the keeper) runs on a background thread
//!   and reports back through a channel, so the screen never freezes.

pub mod app;
pub mod ui;

use crate::config::{self, Settings};
use crate::connect;
use crate::daemon;
use crate::probe::{self, ProbeResult};
use crate::state::DaemonState;
use crate::trust::{self, TrustStore, Verdict};
use app::{App, Command, Msg, NetState, Snapshot, ToastKind};
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, Instant};

const TICK: Duration = Duration::from_millis(200);
const SNAPSHOT_EVERY: Duration = Duration::from_secs(1);
const PROBE_EVERY: Duration = Duration::from_secs(5);

/// Runs the dashboard until the user quits. The terminal is always restored, including
/// after a panic.
pub fn run(dir: &Path) -> io::Result<()> {
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, dir);
    ratatui::restore();
    result
}

fn event_loop(terminal: &mut DefaultTerminal, dir: &Path) -> io::Result<()> {
    let (tx, rx) = mpsc::channel::<Msg>();
    let mut app = App::new(Snapshot::load(dir));

    let mut probing = true;
    spawn_probe(dir, &tx);
    let mut last_snapshot = Instant::now();
    let mut last_probe = Instant::now();

    loop {
        terminal.draw(|f| ui::render(f, &app))?;

        if event::poll(TICK)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            let cmd = app.on_key(key);
            if matches!(cmd, Command::Quit) {
                return Ok(());
            }
            execute(cmd, &mut app, dir, &tx, &mut probing, &mut last_snapshot);
        }

        while let Ok(msg) = rx.try_recv() {
            if matches!(msg, Msg::Net(..)) {
                probing = false;
            }
            let cmd = app.on_msg(msg);
            execute(cmd, &mut app, dir, &tx, &mut probing, &mut last_snapshot);
        }

        app.tick();

        if last_snapshot.elapsed() >= SNAPSHOT_EVERY {
            app.set_snapshot(Snapshot::load(dir));
            last_snapshot = Instant::now();
        }
        if !probing && last_probe.elapsed() >= PROBE_EVERY {
            probing = true;
            last_probe = Instant::now();
            spawn_probe(dir, &tx);
        }
    }
}

fn execute(
    cmd: Command,
    app: &mut App,
    dir: &Path,
    tx: &Sender<Msg>,
    probing: &mut bool,
    last_snapshot: &mut Instant,
) {
    match cmd {
        Command::None | Command::Quit => {}

        Command::Refresh => {
            app.set_snapshot(Snapshot::load(dir));
            *last_snapshot = Instant::now();
            if !*probing {
                *probing = true;
                spawn_probe(dir, tx);
            }
        }

        Command::OpenNow => start_sign_in(app, dir, tx),

        Command::TrustAndOpen(host) => match TrustStore::open(dir).add(&host) {
            Ok(_) => start_sign_in(app, dir, tx),
            Err(e) => app.toast(
                ToastKind::Bad,
                format!("Could not save the trust list: {e}"),
            ),
        },

        Command::ForgetHost(host) => {
            match TrustStore::open(dir).remove(&host) {
                Ok(true) => app.toast(ToastKind::Good, format!("No longer trusting {host}.")),
                Ok(false) => app.toast(ToastKind::Info, format!("{host} was not trusted.")),
                Err(e) => app.toast(ToastKind::Bad, format!("Could not save: {e}")),
            }
            app.set_snapshot(Snapshot::load(dir));
        }

        Command::AddSsid(ssid) => {
            let mut settings = Settings::load(dir);
            if settings.add_ssid(&ssid) {
                match settings.save(dir) {
                    Ok(()) => app.toast(
                        ToastKind::Good,
                        format!("OpenUp may now rejoin \"{ssid}\"."),
                    ),
                    Err(e) => app.toast(ToastKind::Bad, format!("Could not save: {e}")),
                }
            } else {
                app.toast(
                    ToastKind::Info,
                    format!("\"{ssid}\" is already in the list."),
                );
            }
            app.set_snapshot(Snapshot::load(dir));
        }

        Command::RemoveSsid(ssid) => {
            let mut settings = Settings::load(dir);
            if settings.remove_ssid(&ssid) {
                match settings.save(dir) {
                    Ok(()) => app.toast(ToastKind::Good, format!("Removed \"{ssid}\".")),
                    Err(e) => app.toast(ToastKind::Bad, format!("Could not save: {e}")),
                }
            }
            app.set_snapshot(Snapshot::load(dir));
        }

        Command::SaveCreds { username, password } => {
            match config::save_credentials(dir, &username, &password) {
                Ok(()) => {
                    DaemonState::reset_circuit_breaker(&dir.join("state.json"));
                    app.toast(ToastKind::Good, "Login saved.");
                }
                Err(e) => app.toast(ToastKind::Bad, format!("Could not save: {e}")),
            }
            app.set_snapshot(Snapshot::load(dir));
        }

        Command::ToggleKeeper => {
            app.busy = Some(if app.snap.keeper_running {
                "Stopping the keeper...".to_string()
            } else {
                "Starting the keeper...".to_string()
            });
            let running = app.snap.keeper_running;
            let (dir, tx) = (dir.to_path_buf(), tx.clone());
            thread::spawn(move || {
                let result = if running {
                    daemon::stop_daemon(&dir)
                } else {
                    config::load_credentials(&dir)
                        .map_err(|e| e.to_string())
                        .and_then(|_| daemon::start_background_daemon(&dir))
                };
                let _ = tx.send(Msg::KeeperDone(result));
            });
        }
    }
}

fn start_sign_in(app: &mut App, dir: &Path, tx: &Sender<Msg>) {
    app.busy = Some("Signing in...".to_string());
    let (dir, tx): (PathBuf, Sender<Msg>) = (dir.to_path_buf(), tx.clone());
    thread::spawn(move || {
        let progress_tx = tx.clone();
        let result = connect::open_now(
            &dir,
            &|m| {
                let _ = progress_tx.send(Msg::Progress(m.to_string()));
            },
            &|_| false,
        );
        let _ = tx.send(Msg::OpenDone(result));
    });
}

fn spawn_probe(dir: &Path, tx: &Sender<Msg>) {
    let (dir, tx) = (dir.to_path_buf(), tx.clone());
    thread::spawn(move || {
        let (net, ping) = probe_once(&dir);
        let _ = tx.send(Msg::Net(net, ping));
    });
}

fn probe_once(dir: &Path) -> (NetState, Option<u64>) {
    let client = match probe::build_fast_client(false) {
        Ok(c) => c,
        Err(e) => return (NetState::Error(e.to_string()), None),
    };
    let start = std::time::Instant::now();
    match probe::probe_network(&client, connect::cached_gateway(dir).as_deref()) {
        ProbeResult::OnlineUnlocked => (
            NetState::Online,
            Some(start.elapsed().as_millis() as u64),
        ),
        ProbeResult::LockedPortal { login_xml_url, .. } => {
            let trusted = matches!(trust::check(dir, &login_xml_url), Verdict::Trusted);
            let host = trust::host_of(&login_xml_url).unwrap_or(login_xml_url);
            (NetState::Portal { host, trusted }, None)
        }
        ProbeResult::InterfaceOffline => (NetState::Offline, None),
        ProbeResult::ProbeTimeoutOrError(e) => (NetState::Error(e), None),
    }
}
