//! The one-shot "get me online" flow, shared by `openup now` and the dashboard.
//!
//! It never prints and never prompts by itself: progress goes to a callback and the
//! "do you trust this portal?" question goes to another, so the command line can ask on
//! stdin, the dashboard can ask in a dialog, and tests can answer instantly.

use crate::config::{self, Settings};
use crate::logger;
use crate::probe::{self, LoginError, ProbeResult};
use crate::state::DaemonState;
use crate::trust::{self, TrustStore, Verdict};
use crate::wifi;
use std::fmt;
use std::fs;
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing to do: the internet already works.
    AlreadyOnline,
    /// Signed in, and the internet was confirmed reachable afterwards.
    LoggedIn { gateway: String, message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenError {
    /// No (valid) credentials saved yet.
    NoCredentials(String),
    /// The circuit breaker is engaged; `openup creds` resets it.
    Paused,
    /// A portal was found but is not trusted and the caller declined to trust it.
    Untrusted(String),
    /// The portal rejected the credentials (and the breaker has now been engaged).
    BadCredentials(String),
    /// No network, and nothing OpenUp is allowed to join.
    Offline(String),
    /// Anything else (timeout, portal refusal, unreachable probe, ...).
    Failed(String),
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenError::NoCredentials(m) | OpenError::Offline(m) | OpenError::Failed(m) => {
                write!(f, "{m}")
            }
            OpenError::Paused => write!(
                f,
                "Automatic login is paused because the portal rejected your credentials. \
                 Run `openup creds` to update them (this also resumes the keeper)."
            ),
            OpenError::Untrusted(host) => write!(
                f,
                "The portal at {host} is not trusted yet. Run `openup portals trust {host}` \
                 to allow sending your credentials to it."
            ),
            OpenError::BadCredentials(m) => write!(
                f,
                "{m}\nAutomatic login is now paused so your account isn't locked out. \
                 Run `openup creds` with the correct password."
            ),
        }
    }
}

impl std::error::Error for OpenError {}

pub fn cached_gateway(dir: &Path) -> Option<String> {
    let content = fs::read_to_string(dir.join("cached_gateway.txt")).ok()?;
    let trimmed = content.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

pub fn set_cached_gateway(dir: &Path, gateway: &str) {
    let _ = fs::create_dir_all(dir);
    let _ = fs::write(dir.join("cached_gateway.txt"), gateway);
}

/// Signs in now.
///
/// * `progress` receives short status lines ("Portal detected at ...").
/// * `approve(host)` is asked when the portal isn't trusted yet; returning `true` trusts
///   the host and continues, `false` fails with [`OpenError::Untrusted`].
pub fn open_now(
    dir: &Path,
    progress: &dyn Fn(&str),
    approve: &dyn Fn(&str) -> bool,
) -> Result<Outcome, OpenError> {
    let creds = config::load_credentials(dir).map_err(OpenError::NoCredentials)?;

    let state_path = dir.join("state.json");
    if DaemonState::load(&state_path).halted_bad_credentials {
        return Err(OpenError::Paused);
    }

    let cached = cached_gateway(dir);
    let probe_client =
        probe::build_fast_client(false).map_err(|e| OpenError::Failed(e.to_string()))?;
    let login_client = probe::build_login_client().map_err(|e| OpenError::Failed(e.to_string()))?;
    let mut tried_wifi = false;

    loop {
        match probe::probe_network(&probe_client, cached.as_deref()) {
            ProbeResult::OnlineUnlocked => return Ok(Outcome::AlreadyOnline),

            ProbeResult::LockedPortal {
                gateway_base,
                login_xml_url,
            } => {
                progress(&format!("Portal detected at {gateway_base}"));

                if let Verdict::Untrusted { host } = trust::check(dir, &login_xml_url) {
                    if !approve(&host) {
                        logger::log(
                            dir,
                            &format!("Sign-in blocked: portal host {host} is not trusted"),
                        );
                        return Err(OpenError::Untrusted(host));
                    }
                    TrustStore::open(dir).add(&host).map_err(|e| {
                        OpenError::Failed(format!("Could not save trust list: {e}"))
                    })?;
                    progress(&format!("Trusted {host}"));
                }

                progress("Signing in...");
                return match probe::login_and_verify(
                    &login_client,
                    &probe_client,
                    &login_xml_url,
                    &creds.username,
                    &creds.password,
                ) {
                    Ok(message) => {
                        set_cached_gateway(dir, &login_xml_url);
                        logger::log(dir, &format!("Signed in via {gateway_base} (manual)"));
                        Ok(Outcome::LoggedIn {
                            gateway: gateway_base,
                            message,
                        })
                    }
                    Err(LoginError::BadCredentials(msg)) => {
                        DaemonState::mark_bad_credentials(
                            &state_path,
                            &format!("Halted (bad credentials): {msg}"),
                        );
                        logger::log(dir, &format!("Halted: portal rejected the login: {msg}"));
                        Err(OpenError::BadCredentials(msg))
                    }
                    Err(e) => Err(OpenError::Failed(e.to_string())),
                };
            }

            ProbeResult::InterfaceOffline => {
                let ssids = Settings::load(dir).wifi_ssids;
                if ssids.is_empty() {
                    return Err(OpenError::Offline(
                        "Not connected to any network. Connect to your campus Wi-Fi, or run \
                         `openup wifi add \"<SSID>\"` so OpenUp can do it for you."
                            .to_string(),
                    ));
                }
                if tried_wifi {
                    return Err(OpenError::Offline(
                        "Still offline after trying your allowed Wi-Fi networks.".to_string(),
                    ));
                }
                tried_wifi = true;

                progress("Wi-Fi is down; joining an allowed network...");
                let _ = wifi::connect_allowed(&ssids);
                match wifi::wait_for_ipv4(Duration::from_millis(6000), Duration::from_millis(200)) {
                    Some(ip) => progress(&format!("Connected ({ip})")),
                    None => {
                        return Err(OpenError::Offline(
                            "Timed out waiting for an IPv4 address.".to_string(),
                        ));
                    }
                }
            }

            ProbeResult::ProbeTimeoutOrError(e) => {
                return Err(OpenError::Failed(format!("Network probe error: {e}")));
            }
        }
    }
}
