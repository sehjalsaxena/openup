//! `connect::open_now` (the flow behind `openup now` and the dashboard's "sign in")
//! against a mock captive portal.

mod common;

use common::*;
use openup::config;
use openup::connect::{OpenError, Outcome, open_now};
use openup::state::DaemonState;
use openup::trust::TrustStore;
use std::cell::RefCell;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn with_probe_url<T>(url: &str, f: impl FnOnce() -> T) -> T {
    unsafe { std::env::set_var("OPENUP_PROBE_URL", url) };
    let out = f();
    unsafe { std::env::remove_var("OPENUP_PROBE_URL") };
    out
}

fn dir_with_credentials(name: &str) -> std::path::PathBuf {
    let dir = tmp_dir(name);
    config::save_credentials(&dir, "alice", "hunter2").unwrap();
    dir
}

/// Locked until a POST arrives, then online.
fn portal_that_unlocks_after_login() -> MockPortal {
    let logged_in = Arc::new(AtomicBool::new(false));
    MockPortal::start(move |req, port| {
        if req.starts_with("POST") {
            logged_in.store(true, Ordering::SeqCst);
            http("200 OK", "Content-Type: text/xml\r\n", SOPHOS_LIVE)
        } else if logged_in.load(Ordering::SeqCst) {
            internet_ok()
        } else {
            redirect_to_portal(port)
        }
    })
}

fn open(
    dir: &std::path::Path,
    probe: &str,
    approve: bool,
) -> (Result<Outcome, OpenError>, Vec<String>) {
    let seen = RefCell::new(Vec::new());
    let _guard = ENV_LOCK.lock().unwrap();
    let result = with_probe_url(probe, || {
        open_now(dir, &|m| seen.borrow_mut().push(m.to_string()), &|_| {
            approve
        })
    });
    (result, seen.into_inner())
}

#[test]
fn already_online_does_nothing() {
    let portal = MockPortal::start(|_, _| internet_ok());
    let dir = dir_with_credentials("conn_online");
    let (result, _) = open(&dir, &portal.probe_url(), false);
    assert_eq!(result, Ok(Outcome::AlreadyOnline));
    assert_eq!(portal.count("POST"), 0);
}

#[test]
fn signs_in_through_a_trusted_portal_and_verifies() {
    let portal = portal_that_unlocks_after_login();
    let dir = dir_with_credentials("conn_trusted");
    TrustStore::open(&dir).add("127.0.0.1").unwrap();

    let (result, progress) = open(&dir, &portal.probe_url(), false);
    match result {
        Ok(Outcome::LoggedIn { gateway, .. }) => assert!(gateway.contains("127.0.0.1")),
        other => panic!("{other:?}"),
    }
    assert_eq!(portal.count("POST"), 1);
    assert!(
        progress.iter().any(|m| m.starts_with("Portal detected")),
        "{progress:?}"
    );
    assert!(
        progress.iter().any(|m| m.contains("Signing in")),
        "{progress:?}"
    );
}

#[test]
fn an_unapproved_portal_receives_nothing() {
    let portal = portal_that_unlocks_after_login();
    let dir = dir_with_credentials("conn_declined");

    let (result, _) = open(&dir, &portal.probe_url(), false);
    assert_eq!(result, Err(OpenError::Untrusted("127.0.0.1".into())));
    assert_eq!(portal.count("POST"), 0, "credentials must not be sent");
    assert!(
        TrustStore::open(&dir).hosts().is_empty(),
        "declining must not trust it"
    );
}

#[test]
fn approving_trusts_the_portal_for_next_time_and_signs_in() {
    let portal = portal_that_unlocks_after_login();
    let dir = dir_with_credentials("conn_approved");

    let (result, progress) = open(&dir, &portal.probe_url(), true);
    assert!(matches!(result, Ok(Outcome::LoggedIn { .. })), "{result:?}");
    assert_eq!(TrustStore::open(&dir).hosts(), vec!["127.0.0.1"]);
    assert!(
        progress.iter().any(|m| m.starts_with("Trusted")),
        "{progress:?}"
    );
}

#[test]
fn bad_credentials_pause_everything_and_stop_further_attempts() {
    let portal = MockPortal::start(|req, port| {
        if req.starts_with("POST") {
            http("200 OK", "Content-Type: text/xml\r\n", SOPHOS_BAD_PASSWORD)
        } else {
            redirect_to_portal(port)
        }
    });
    let dir = dir_with_credentials("conn_badcreds");
    TrustStore::open(&dir).add("127.0.0.1").unwrap();

    let (first, _) = open(&dir, &portal.probe_url(), false);
    assert!(
        matches!(first, Err(OpenError::BadCredentials(_))),
        "{first:?}"
    );
    assert!(DaemonState::load(&dir.join("state.json")).halted_bad_credentials);
    assert_eq!(portal.count("POST"), 1);

    let (second, _) = open(&dir, &portal.probe_url(), false);
    assert_eq!(second, Err(OpenError::Paused));
    assert_eq!(portal.count("POST"), 1);

    DaemonState::reset_circuit_breaker(&dir.join("state.json"));
    assert!(!DaemonState::load(&dir.join("state.json")).halted_bad_credentials);
}

#[test]
fn missing_credentials_tell_you_the_next_command() {
    let portal = MockPortal::start(|_, _| internet_ok());
    let dir = tmp_dir("conn_nocreds");
    let (result, _) = open(&dir, &portal.probe_url(), false);
    match result {
        Err(OpenError::NoCredentials(msg)) => assert!(msg.contains("openup creds"), "{msg}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn error_messages_name_the_new_commands() {
    assert!(OpenError::Paused.to_string().contains("openup creds"));
    let msg = OpenError::Untrusted("10.1.1.9".into()).to_string();
    assert!(msg.contains("openup portals trust 10.1.1.9"), "{msg}");
}
