//! Runs the real compiled `openup` binary. These catch wiring mistakes that unit tests
//! cannot: renamed subcommands, argument parsing, exit codes, and the keeper's own
//! process being launched with the right arguments.

mod common;

use common::*;
use openup::config;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_openup");
/// Nothing listens here, so probes fail fast instead of touching the real network.
const DEAD_PROBE: &str = "http://127.0.0.1:9/probe";

fn openup(dir: &Path, probe: &str, args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .env("OPENUP_CONFIG_DIR", dir)
        .env("OPENUP_PROBE_URL", probe)
        .env_remove("OPENUP_TRUST_ANY")
        .stdin(std::process::Stdio::null())
        .output()
        .expect("run openup")
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).to_string()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).to_string()
}

fn dir(name: &str) -> PathBuf {
    let d = tmp_dir(name);
    config::save_credentials(&d, "alice", "hunter2").unwrap();
    d
}

/// Always stops a keeper this test started, even when an assertion fails.
struct KeeperGuard(PathBuf);
impl Drop for KeeperGuard {
    fn drop(&mut self) {
        let _ = openup(&self.0, DEAD_PROBE, &["keeper", "stop"]);
    }
}

#[test]
fn version_and_help_work_and_use_the_new_name() {
    let d = dir("cli_help");
    let v = openup(&d, DEAD_PROBE, &["--version"]);
    assert!(v.status.success());
    assert!(out(&v).starts_with("openup "), "{}", out(&v));

    for flag in ["--help", "-h", "help"] {
        let h = openup(&d, DEAD_PROBE, &[flag]);
        assert!(h.status.success());
        let text = out(&h);
        for cmd in [
            "openup now",
            "openup setup",
            "openup doctor",
            "openup keeper",
            "openup portals",
            "openup wifi",
            "openup log",
        ] {
            assert!(text.contains(cmd), "{flag}: help is missing {cmd:?}");
        }
        assert!(
            !text.contains("ccp") && !text.contains("wicket"),
            "an old name leaked into help"
        );
    }
}

#[test]
fn without_a_terminal_bare_openup_prints_help_instead_of_the_dashboard() {
    let d = dir("cli_bare");
    let o = openup(&d, DEAD_PROBE, &[]);
    assert!(o.status.success());
    assert!(out(&o).contains("Usage:"));
}

#[test]
fn unknown_and_removed_commands_fail_clearly() {
    let d = dir("cli_unknown");
    for bad in ["frobnicate", "daemon", "set", "autostart", "open"] {
        let o = openup(&d, DEAD_PROBE, &[bad]);
        assert!(!o.status.success(), "{bad} should fail");
        assert!(
            err(&o).contains(&format!("Unknown command '{bad}'")),
            "{}",
            err(&o)
        );
    }
}

#[test]
fn open_without_credentials_points_at_the_next_step() {
    let d = tmp_dir("cli_nocreds");
    let o = openup(&d, DEAD_PROBE, &["now"]);
    assert!(!o.status.success());
    assert!(err(&o).contains("openup creds"), "{}", err(&o));
}

#[test]
fn keeper_start_status_stop_lifecycle_uses_the_real_process() {
    let d = dir("cli_keeper");
    let _guard = KeeperGuard(d.clone());

    let start = openup(&d, DEAD_PROBE, &["keeper", "start"]);
    assert!(
        start.status.success(),
        "start failed: {} {}",
        out(&start),
        err(&start)
    );
    assert!(out(&start).contains("Keeper started"), "{}", out(&start));

    let again = openup(&d, DEAD_PROBE, &["keeper", "start"]);
    assert!(out(&again).contains("already running"), "{}", out(&again));

    let status = openup(&d, DEAD_PROBE, &["status"]);
    assert!(out(&status).contains("Running"), "{}", out(&status));

    let stop = openup(&d, DEAD_PROBE, &["keeper", "stop"]);
    assert!(stop.status.success(), "{}", err(&stop));
    assert!(out(&stop).contains("stopped"), "{}", out(&stop));

    let status = openup(&d, DEAD_PROBE, &["status"]);
    assert!(out(&status).contains("Stopped"), "{}", out(&status));

    let log = openup(&d, DEAD_PROBE, &["log"]);
    assert!(
        out(&log).contains("Keeper started"),
        "the keeper should have logged: {}",
        out(&log)
    );
}

#[test]
fn keeper_start_without_credentials_refuses() {
    let d = tmp_dir("cli_keeper_nocreds");
    let o = openup(&d, DEAD_PROBE, &["keeper", "start"]);
    assert!(!o.status.success());
    assert!(err(&o).contains("openup creds"));
}

#[test]
fn portals_trust_list_forget_round_trip() {
    let d = dir("cli_portals");
    assert!(out(&openup(&d, DEAD_PROBE, &["portals"])).contains("No trusted portals"));

    assert!(
        out(&openup(
            &d,
            DEAD_PROBE,
            &["portals", "trust", "http://172.16.68.6:8090/login.xml"]
        ))
        .contains("Trusted 172.16.68.6")
    );
    assert!(
        out(&openup(
            &d,
            DEAD_PROBE,
            &["portals", "trust", "172.16.68.6"]
        ))
        .contains("already trusted")
    );
    assert!(out(&openup(&d, DEAD_PROBE, &["portals"])).contains("172.16.68.6"));

    assert!(
        out(&openup(
            &d,
            DEAD_PROBE,
            &["portals", "forget", "172.16.68.6"]
        ))
        .contains("No longer trusting")
    );
    assert!(out(&openup(&d, DEAD_PROBE, &["portals"])).contains("No trusted portals"));

    let bad = openup(&d, DEAD_PROBE, &["portals", "bogus"]);
    assert!(!bad.status.success());
}

#[test]
fn wifi_allow_list_round_trip_including_names_with_spaces() {
    let d = dir("cli_wifi");
    assert!(out(&openup(&d, DEAD_PROBE, &["wifi", "list"])).contains("never connects"));

    assert!(
        out(&openup(&d, DEAD_PROBE, &["wifi", "add", "College", "WiFi"]))
            .contains("\"College WiFi\"")
    );
    assert!(out(&openup(&d, DEAD_PROBE, &["wifi", "add", "college wifi"])).contains("already"));
    assert!(out(&openup(&d, DEAD_PROBE, &["wifi", "list"])).contains("College WiFi"));
    assert!(out(&openup(&d, DEAD_PROBE, &["wifi", "remove", "COLLEGE WIFI"])).contains("Removed"));
    assert!(out(&openup(&d, DEAD_PROBE, &["wifi", "list"])).contains("never connects"));
    assert!(
        !openup(&d, DEAD_PROBE, &["wifi", "add"]).status.success(),
        "add needs a name"
    );
}

#[test]
fn log_with_no_entries_says_so() {
    let d = dir("cli_log_empty");
    assert!(out(&openup(&d, DEAD_PROBE, &["log"])).contains("No log entries yet"));
}

#[test]
fn doctor_reports_failures_with_a_nonzero_exit() {
    let d = tmp_dir("cli_doctor_bad"); // no credentials
    let o = openup(&d, DEAD_PROBE, &["doctor"]);
    assert!(!o.status.success());
    let text = out(&o);
    assert!(text.contains("[FAIL] Credentials"), "{text}");
    assert!(text.contains("openup creds"), "{text}");
}

#[test]
fn doctor_is_happy_when_online_with_credentials() {
    let portal = MockPortal::start(|_, _| internet_ok());
    let d = dir("cli_doctor_ok");
    let o = openup(&d, &portal.probe_url(), &["doctor"]);
    let text = out(&o);
    assert!(text.contains("[ ok ] Credentials"), "{text}");
    assert!(text.contains("[ ok ] Network"), "{text}");
    assert!(!text.contains("[FAIL]"), "{text}");
}

#[test]
fn the_whole_trust_then_sign_in_story_through_the_cli() {
    let logged_in = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = logged_in.clone();
    let portal = MockPortal::start(move |req, port| {
        use std::sync::atomic::Ordering::SeqCst;
        if req.starts_with("POST") {
            flag.store(true, SeqCst);
            http("200 OK", "Content-Type: text/xml\r\n", SOPHOS_LIVE)
        } else if flag.load(SeqCst) {
            internet_ok()
        } else {
            redirect_to_portal(port)
        }
    });
    let d = dir("cli_story");

    let o = openup(&d, &portal.probe_url(), &["now"]);
    assert!(!o.status.success());
    assert!(
        err(&o).contains("openup portals trust 127.0.0.1"),
        "{}",
        err(&o)
    );
    assert_eq!(portal.count("POST"), 0);

    assert!(
        openup(&d, &portal.probe_url(), &["portals", "trust", "127.0.0.1"])
            .status
            .success()
    );

    let o = openup(&d, &portal.probe_url(), &["now"]);
    assert!(o.status.success(), "{} {}", out(&o), err(&o));
    assert!(out(&o).contains("Signed in"), "{}", out(&o));
    assert_eq!(portal.count("POST"), 1);

    let o = openup(&d, &portal.probe_url(), &["now"]);
    assert!(out(&o).contains("Already online"), "{}", out(&o));
    assert_eq!(portal.count("POST"), 1);

    assert!(out(&openup(&d, &portal.probe_url(), &["log"])).contains("Signed in via"));
}

#[test]
fn notify_command_reports_and_toggles_status() {
    let d = dir("cli_notify");
    let o = openup(&d, DEAD_PROBE, &["notify"]);
    assert!(o.status.success());
    assert!(out(&o).contains("Desktop notifications are enabled"));

    let o = openup(&d, DEAD_PROBE, &["notify", "off"]);
    assert!(o.status.success());
    assert!(out(&o).contains("disabled"));

    let o = openup(&d, DEAD_PROBE, &["notify"]);
    assert!(out(&o).contains("Desktop notifications are disabled"));

    let o = openup(&d, DEAD_PROBE, &["notify", "on"]);
    assert!(o.status.success());
    assert!(out(&o).contains("enabled"));

    let o = openup(&d, DEAD_PROBE, &["notify", "test"]);
    assert!(o.status.success());
    let text = out(&o);
    assert!(text.contains("OpenUp: Signed In"));
    assert!(!text.contains('—'));
}

#[test]
fn completions_command_generates_scripts() {
    let d = dir("cli_completions");
    for shell in ["powershell", "bash", "zsh", "fish"] {
        let o = openup(&d, DEAD_PROBE, &["completions", shell]);
        assert!(o.status.success(), "completions for {shell} failed");
        let script = out(&o);
        assert!(script.contains("openup"));
        assert!(script.contains("keeper"));
        assert!(script.contains("update"));
    }

    let bad = openup(&d, DEAD_PROBE, &["completions", "unknownshell"]);
    assert!(!bad.status.success());
}

#[test]
fn update_command_help_and_parsing() {
    let d = dir("cli_update");
    let h = openup(&d, DEAD_PROBE, &["--help"]);
    assert!(out(&h).contains("openup update"));
    assert!(out(&h).contains("openup completions"));
}
