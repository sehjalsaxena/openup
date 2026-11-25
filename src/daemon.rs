use crate::config::{self, Settings};
use crate::instance::{self, SingleInstanceLock};
use crate::logger;
use crate::notify;
use crate::probe::{self, LoginError, ProbeResult};
use crate::state::DaemonState;
use crate::trust::{self, TrustStore, Verdict};
use crate::wifi;
use std::path::Path;
use std::time::Duration;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(target_os = "windows")]
const DETACHED_PROCESS: u32 = 0x0000_0008;

/// Loop period. Roam detection is a local, zero-packet check, so this is cheap.
const TICK: Duration = Duration::from_secs(2);
/// If more wall-clock time than this passed between two ticks (>= 2.5 * TICK), the machine slept.
const WAKE_GAP_SECS: u64 = 5;
/// Consecutive "portal said OK, internet still locked" results before giving up.
const MAX_STILL_LOCKED: u32 = 3;
/// Ticks between Wi-Fi reconnect attempts: starts at ~10 s, backs off to ~2 min.
const RECONNECT_MIN_TICKS: u32 = 5;
const RECONNECT_MAX_TICKS: u32 = 60;

/// Seconds to wait before the next automatic probe after `failure_streak` failures:
/// 5, 10, 20, 40, 80, 160, then 300 s.
pub fn backoff_secs(failure_streak: u32) -> u64 {
    300u64.min(5 * (1u64 << failure_streak.saturating_sub(1).min(6)))
}

fn load_latest_credentials(
    config_dir: &Path,
    default_user: &str,
    default_pass: &str,
) -> (String, String) {
    match config::load_credentials(config_dir) {
        Ok(c) => (c.username, c.password),
        Err(_) => (default_user.to_string(), default_pass.to_string()),
    }
}

fn epoch_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Runs the autonomous background daemon roaming & re-auth loop.
pub fn run_daemon(
    config_dir: &Path,
    username: &str,
    password: &str,
    once: bool,
    dry_run: bool,
) -> Result<(), String> {
    let lock_path = config_dir.join("daemon.lock");
    let state_path = config_dir.join("state.json");

    let _lock = if once && dry_run {
        None
    } else {
        Some(SingleInstanceLock::acquire(&lock_path)?)
    };

    let can_save_state = _lock.is_some() || !instance::is_daemon_running(&lock_path);
    let mut state = DaemonState::load(&state_path);
    if can_save_state {
        state.pid = std::process::id();
        let _ = state.save_atomic(&state_path);
    }

    let save_state = |st: &DaemonState| {
        if can_save_state {
            let _ = st.save_atomic(&state_path);
        }
    };
    let log = |msg: &str| {
        if can_save_state {
            logger::log(config_dir, msg);
        }
    };

    let cached_gw_path = config_dir.join("cached_gateway.txt");
    if state.active_gateway.is_none()
        && cached_gw_path.exists()
        && let Ok(gw) = std::fs::read_to_string(&cached_gw_path)
    {
        let gw_trimmed = gw.trim().to_string();
        if !gw_trimmed.is_empty() {
            state.active_gateway = Some(gw_trimmed);
        }
    }

    let probe_client = probe::build_fast_client(false).map_err(|e| e.to_string())?;
    let login_client = probe::build_login_client().map_err(|e| e.to_string())?;

    let mut first_tick = true;
    let mut next_allowed_probe_epoch: u64 = 0;
    let mut reconnect_countdown: u32 = RECONNECT_MIN_TICKS;
    let mut reconnect_interval: u32 = RECONNECT_MIN_TICKS;
    let mut last_tick_epoch: u64 = 0;

    log(&format!(
        "Keeper started (pid {}{}{})",
        std::process::id(),
        if once { ", once" } else { "" },
        if dry_run { ", dry-run" } else { "" }
    ));

    loop {
        let now = epoch_now();

        let mut force_probe = once || first_tick;
        first_tick = false;

        if last_tick_epoch != 0 && now.saturating_sub(last_tick_epoch) >= WAKE_GAP_SECS {
            let gap = now.saturating_sub(last_tick_epoch);
            log(&format!(
                "Wake from sleep detected ({gap}s gap); triggering immediate re-auth"
            ));
            force_probe = true;
            next_allowed_probe_epoch = 0;
            state.last_probe_epoch = 0;
            reconnect_countdown = 0;
        }

        if state.halted_bad_credentials {
            let disk_state = DaemonState::load(&state_path);
            if !disk_state.halted_bad_credentials {
                state.halted_bad_credentials = false;
                state.failure_streak = 0;
                state.still_locked_streak = 0;
                force_probe = true;
                next_allowed_probe_epoch = 0;
                state.last_probe_epoch = 0;
                log("Credentials updated; automatic login resumed");
            }
        }

        let local_ip = probe::current_local_ipv4();

        match local_ip {
            None => {
                state.last_local_ip = None;
                reconnect_countdown = reconnect_countdown.saturating_sub(1);
                if reconnect_countdown == 0 {
                    reconnect_countdown = reconnect_interval;
                    let ssids = Settings::load(config_dir).wifi_ssids;
                    if !dry_run && !ssids.is_empty() {
                        log(&format!(
                            "No network; trying allowed Wi-Fi: {}",
                            ssids.join(", ")
                        ));
                        let _ = wifi::connect_allowed(&ssids);
                        if let Some(ip) =
                            wifi::wait_for_ipv4(Duration::from_secs(5), Duration::from_millis(200))
                        {
                            state.last_local_ip = Some(ip.to_string());
                            force_probe = true;
                            next_allowed_probe_epoch = 0;
                            state.last_probe_epoch = 0;
                            reconnect_interval = RECONNECT_MIN_TICKS;
                            notify::notify_wifi_connected(config_dir, &ssids.join(", "));
                        } else {
                            reconnect_interval = (reconnect_interval * 2).min(RECONNECT_MAX_TICKS);
                            reconnect_countdown = reconnect_interval;
                        }
                    }
                }
            }
            Some(ip) => {
                reconnect_countdown = RECONNECT_MIN_TICKS;
                reconnect_interval = RECONNECT_MIN_TICKS;
                let ip_str = ip.to_string();
                if state.last_local_ip.as_deref() != Some(&ip_str) {
                    log(&format!(
                        "Network change: {} -> {ip_str}",
                        state.last_local_ip.as_deref().unwrap_or("none")
                    ));
                    state.last_local_ip = Some(ip_str);
                    force_probe = true;
                    next_allowed_probe_epoch = 0;
                    state.last_probe_epoch = 0;
                }
            }
        }

        let is_battery = crate::power::is_on_battery();
        let probe_interval: u64 = if is_battery { 60 } else { 30 };
        let probe_due = now >= next_allowed_probe_epoch
            && now.saturating_sub(state.last_probe_epoch) >= probe_interval;
        if force_probe || probe_due {
            state.last_probe_epoch = now;

            let fallback_gw = if !force_probe && state.last_status_message == "Online and unlocked"
            {
                None
            } else {
                state.active_gateway.as_deref()
            };

            let probe_res = probe::probe_network(&probe_client, fallback_gw);
            match probe_res {
                ProbeResult::OnlineUnlocked => {
                    if state.last_status_message != "Online and unlocked" {
                        log("Online");
                    }
                    if state.session_start_epoch.is_none() {
                        state.session_start_epoch = Some(now);
                    }
                    state.failure_streak = 0;
                    state.still_locked_streak = 0;
                    state.last_status_message = "Online and unlocked".to_string();
                    save_state(&state);
                }

                ProbeResult::LockedPortal {
                    gateway_base,
                    login_xml_url,
                } => {
                    if dry_run {
                        state.last_status_message =
                            format!("Dry-run: portal locked at {gateway_base}");
                        save_state(&state);
                        if once {
                            return Ok(());
                        }
                    } else if state.halted_bad_credentials {
                        state.last_status_message =
                            "Halted due to bad credentials. Run 'openup creds'.".to_string();
                        save_state(&state);
                    } else if let Verdict::Untrusted { host } =
                        trust::check(config_dir, &login_xml_url)
                    {
                        if state.pending_untrusted_gateway.as_deref() != Some(&host) {
                            log(&format!(
                                "Blocked login: portal host {host} is not trusted (run 'openup portals trust {host}')"
                            ));
                            notify::notify_portal_blocked(config_dir, &host);
                        }
                        state.last_status_message = format!(
                            "Blocked: portal host {host} is not trusted. Run 'openup portals trust {host}'."
                        );
                        state.pending_untrusted_gateway = Some(host);
                        save_state(&state);
                    } else {
                        let (cur_user, cur_pass) =
                            load_latest_credentials(config_dir, username, password);
                        log(&format!("Portal at {gateway_base}; logging in"));
                        match probe::login_and_verify(
                            &login_client,
                            &probe_client,
                            &login_xml_url,
                            &cur_user,
                            &cur_pass,
                        ) {
                            Ok(_) => {
                                state.failure_streak = 0;
                                state.still_locked_streak = 0;
                                state.record_login(now);
                                state.session_start_epoch = Some(now);
                                state.last_login_epoch = now;
                                state.active_gateway = Some(login_xml_url.clone());
                                state.pending_untrusted_gateway = None;
                                state.last_status_message =
                                    format!("Authenticated at {gateway_base}");
                                next_allowed_probe_epoch = 0;
                                log(&format!("Login verified at {gateway_base}"));
                                notify::notify_login_success(config_dir, &cur_user, &gateway_base);
                                save_state(&state);
                                if can_save_state {
                                    let _ = std::fs::write(&cached_gw_path, &login_xml_url);
                                }
                            }
                            Err(LoginError::BadCredentials(msg)) => {
                                state.halted_bad_credentials = true;
                                state.failure_streak += 1;
                                state.last_status_message =
                                    format!("Halted (bad credentials): {msg}");
                                log(&format!("Halted: {msg}"));
                                notify::notify_login_failed(config_dir, &msg);
                                save_state(&state);
                            }
                            Err(LoginError::StillLocked) => {
                                state.still_locked_streak += 1;
                                state.failure_streak += 1;
                                if state.still_locked_streak >= MAX_STILL_LOCKED {
                                    state.halted_bad_credentials = true;
                                    state.last_status_message = format!(
                                        "Halted: the portal accepted the login {} times but the internet stayed locked. Check your credentials with 'openup creds'.",
                                        state.still_locked_streak
                                    );
                                    notify::notify_circuit_breaker(
                                        config_dir,
                                        state.still_locked_streak,
                                    );
                                } else {
                                    state.last_status_message = format!(
                                        "Login accepted but internet still locked (attempt {}/{MAX_STILL_LOCKED})",
                                        state.still_locked_streak
                                    );
                                    next_allowed_probe_epoch =
                                        now + backoff_secs(state.failure_streak);
                                }
                                log(&state.last_status_message);
                                save_state(&state);
                            }
                            Err(e) => {
                                state.failure_streak += 1;
                                state.last_status_message =
                                    format!("Login failed (attempt {}): {e}", state.failure_streak);
                                log(&state.last_status_message);
                                save_state(&state);
                                next_allowed_probe_epoch = now + backoff_secs(state.failure_streak);
                            }
                        }
                    }
                }

                ProbeResult::InterfaceOffline => {
                    state.last_status_message = "Wi-Fi interface offline".to_string();
                    save_state(&state);
                }

                ProbeResult::ProbeTimeoutOrError(e) => {
                    state.last_status_message = format!("Probe error: {e}");
                    save_state(&state);
                }
            }
        }

        if once {
            return Ok(());
        }

        last_tick_epoch = epoch_now();
        std::thread::sleep(TICK);
    }
}

/// Spawns the keeper in the background, completely detached, with no window.
/// Returns a human-readable message; this function never prints.
pub fn start_background_daemon(config_dir: &Path) -> Result<String, String> {
    let lock_path = config_dir.join("daemon.lock");
    if instance::is_daemon_running(&lock_path) {
        return Ok("The keeper is already running.".to_string());
    }

    let exe = std::env::current_exe().map_err(|e| format!("Failed to get exe path: {e}"))?;
    let mut cmd = std::process::Command::new(exe);
    cmd.args(["keeper", "run"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    #[cfg(target_os = "windows")]
    cmd.creation_flags(DETACHED_PROCESS);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn the keeper: {e}"))?;

    let mut started = false;
    for _ in 0..40 {
        if instance::is_daemon_running(&lock_path) {
            started = true;
            break;
        }
        if let Ok(Some(_)) = child.try_wait() {
            break; // exited already
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    if !started {
        return Err("The keeper exited before it could start. See 'openup log'.".to_string());
    }

    Ok("Keeper started.".to_string())
}

/// Stops the running keeper by terminating its process. Never prints.
pub fn stop_daemon(config_dir: &Path) -> Result<String, String> {
    let lock_path = config_dir.join("daemon.lock");
    let state_path = config_dir.join("state.json");

    if !instance::is_daemon_running(&lock_path) {
        return Ok("The keeper is not running.".to_string());
    }

    let pid = instance::read_lock_pid(&lock_path)
        .or_else(|| Some(DaemonState::load(&state_path).pid).filter(|p| *p > 0));

    let Some(pid) = pid else {
        return Err("Could not determine the keeper's PID.".to_string());
    };

    #[cfg(target_os = "windows")]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn OpenProcess(
                dwDesiredAccess: u32,
                bInheritHandle: i32,
                dwProcessId: u32,
            ) -> *mut std::ffi::c_void;
            fn TerminateProcess(hProcess: *mut std::ffi::c_void, uExitCode: u32) -> i32;
            fn CloseHandle(hObject: *mut std::ffi::c_void) -> i32;
        }
        const PROCESS_TERMINATE: u32 = 0x0001;
        unsafe {
            let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if !handle.is_null() {
                let _ = TerminateProcess(handle, 1);
                let _ = CloseHandle(handle);
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = std::process::Command::new("kill")
            .arg(pid.to_string())
            .output();
    }

    for _ in 0..40 {
        if !instance::is_daemon_running(&lock_path) {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    if instance::is_daemon_running(&lock_path) {
        return Err(format!("Keeper (PID {pid}) did not stop."));
    }

    #[cfg(target_os = "windows")]
    let _ = std::fs::remove_file(&lock_path);

    logger::log(config_dir, &format!("Keeper (pid {pid}) stopped"));
    Ok(format!("Keeper (PID {pid}) stopped."))
}

/// Prints the detailed operational status of the daemon.
pub fn print_status(config_dir: &Path) {
    let lock_path = config_dir.join("daemon.lock");
    let state_path = config_dir.join("state.json");

    let is_running = instance::is_daemon_running(&lock_path);
    let state = DaemonState::load(&state_path);

    let status_str = if is_running { "Running" } else { "Stopped" };
    let breaker_str = if state.halted_bad_credentials {
        "Halted (bad credentials - run 'openup creds')"
    } else {
        "Normal (Active)"
    };

    let store = TrustStore::open(config_dir);
    let trusted = store.hosts();
    let ssids = Settings::load(config_dir).wifi_ssids;

    println!("OpenUp status");
    println!("-------------");
    println!("State:            {status_str}");
    if is_running && state.pid > 0 {
        println!("PID:              {}", state.pid);
    }
    println!(
        "Local IP:         {}",
        state.last_local_ip.as_deref().unwrap_or("None (Offline)")
    );
    println!(
        "Active Gateway:   {}",
        state
            .active_gateway
            .as_deref()
            .unwrap_or("None (Undiscovered)")
    );
    println!("Total Logins:     {}", state.total_logins);
    if state.last_login_epoch > 0 {
        println!(
            "Last Login:       {}",
            logger::format_utc(state.last_login_epoch)
        );
    }
    println!("Failure Streak:   {}", state.failure_streak);
    println!("Circuit Breaker:  {breaker_str}");
    println!(
        "Trusted Portals:  {}",
        if trusted.is_empty() {
            "none yet (run 'openup now' once on campus Wi-Fi to approve one)".to_string()
        } else {
            trusted.join(", ")
        }
    );
    if let Some(pending) = state
        .pending_untrusted_gateway
        .as_deref()
        .filter(|h| !store.is_trusted(h))
    {
        println!(
            "Blocked Portal:   {pending}  ->  run 'openup portals trust {pending}' to allow it"
        );
    }
    println!(
        "Wi-Fi Auto-join:  {}",
        if ssids.is_empty() {
            "off (enable with 'openup wifi add <SSID>')".to_string()
        } else {
            ssids.join(", ")
        }
    );
    println!(
        "Last Status:      {}",
        if state.last_status_message.is_empty() {
            "No activity recorded"
        } else {
            &state.last_status_message
        }
    );
}

/// Configures operating system autostart on system boot/login.
pub fn set_autostart(config_dir: &Path, enable: bool) -> Result<(), String> {
    let _ = config_dir;
    let exe = std::env::current_exe().map_err(|e| format!("Failed to get exe path: {e}"))?;
    let exe_str = exe.to_string_lossy();

    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        if enable {
            let value = format!("\"{exe_str}\" keeper start");
            let output = Command::new("reg")
                .args([
                    "add",
                    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                    "/v",
                    crate::brand::WINDOWS_RUN_VALUE,
                    "/t",
                    "REG_SZ",
                    "/d",
                    &value,
                    "/f",
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .map_err(|e| format!("Failed to execute reg.exe: {e}"))?;

            if output.status.success() {
                println!("[OK] Auto-start enabled (Windows Startup registry key set).");
            } else {
                return Err(format!(
                    "Failed to set registry key: {}",
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
        } else {
            let output = Command::new("reg")
                .args([
                    "delete",
                    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                    "/v",
                    crate::brand::WINDOWS_RUN_VALUE,
                    "/f",
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .map_err(|e| format!("Failed to execute reg.exe: {e}"))?;

            if output.status.success() {
                println!("[OK] Auto-start disabled (Windows Startup registry key removed).");
            } else {
                return Err(format!(
                    "Failed to delete registry key: {}",
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").map_err(|_| "HOME not set".to_string())?;
        let agents_dir = Path::new(&home).join("Library/LaunchAgents");
        let plist_path = agents_dir.join(format!("{}.plist", crate::brand::LAUNCH_LABEL));

        if enable {
            std::fs::create_dir_all(&agents_dir)
                .map_err(|e| format!("Failed to create LaunchAgents dir: {e}"))?;
            let exe_escaped = exe_str
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;");
            let label = crate::brand::LAUNCH_LABEL;
            let plist_content = format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe_escaped}</string>
        <string>keeper</string>
        <string>start</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <false/>
</dict>
</plist>"#
            );
            std::fs::write(&plist_path, plist_content)
                .map_err(|e| format!("Failed to write plist: {e}"))?;
            let _ = std::process::Command::new("launchctl")
                .args(["load", "-w", &plist_path.to_string_lossy()])
                .output();
            println!("[OK] Auto-start enabled (macOS LaunchAgent created).");
        } else if plist_path.exists() {
            let _ = std::process::Command::new("launchctl")
                .args(["unload", &plist_path.to_string_lossy()])
                .output();
            let _ = std::fs::remove_file(&plist_path);
            println!("[OK] Auto-start disabled (macOS LaunchAgent removed).");
        }
    }

    #[cfg(target_os = "linux")]
    {
        let home = std::env::var("HOME").map_err(|_| "HOME not set".to_string())?;
        let systemd_dir = Path::new(&home).join(".config/systemd/user");
        let service_path = systemd_dir.join(crate::brand::SERVICE_NAME);

        if enable {
            let lock_path = config_dir.join("daemon.lock");
            if instance::is_daemon_running(&lock_path) {
                let _ = stop_daemon(config_dir);
            }

            std::fs::create_dir_all(&systemd_dir)
                .map_err(|e| format!("Failed to create systemd user dir: {e}"))?;
            let service_content = format!(
                "[Unit]\nDescription=OpenUp captive portal keeper\nAfter=network.target\n\n[Service]\nExecStart=\"{exe_str}\" keeper run\nRestart=on-failure\nRestartSec=5\n\n[Install]\nWantedBy=default.target\n"
            );
            std::fs::write(&service_path, service_content)
                .map_err(|e| format!("Failed to write service file: {e}"))?;
            let output = std::process::Command::new("systemctl")
                .args(["--user", "enable", "--now", crate::brand::SERVICE_NAME])
                .output()
                .map_err(|e| format!("Failed to execute systemctl: {e}"))?;
            if !output.status.success() {
                return Err(format!(
                    "Failed to enable systemd service: {}",
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
            println!("[OK] Auto-start enabled (Linux user systemd service enabled).");
        } else {
            let _ = std::process::Command::new("systemctl")
                .args(["--user", "disable", "--now", crate::brand::SERVICE_NAME])
                .output();
            let _ = std::fs::remove_file(&service_path);
            println!("[OK] Auto-start disabled (Linux user systemd service removed).");
        }
    }

    Ok(())
}
