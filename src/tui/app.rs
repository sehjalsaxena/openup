//! Dashboard state and key handling. Pure logic: no terminal, no I/O, no threads, so
//! everything here is unit-tested directly.

use crate::config;
use crate::connect::{OpenError, Outcome};
use crate::instance;
use crate::logger;
use crate::state::DaemonState;
use crate::trust::TrustStore;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::Path;

pub const MIN_WIDTH: u16 = 60;
pub const MIN_HEIGHT: u16 = 18;
/// Toasts last this many 200 ms ticks (about 5 s).
const TOAST_TICKS: u16 = 25;

/// What the network looks like right now (from the dashboard's own background probe).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetState {
    Checking,
    Online,
    Portal { host: String, trusted: bool },
    Offline,
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Portals,
    Wifi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredField {
    User,
    Pass,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    AddSsid,
}

/// Side effects the event loop must perform on the app's behalf.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    None,
    Quit,
    Refresh,
    OpenNow,
    ToggleKeeper,
    TrustAndOpen(String),
    ForgetHost(String),
    AddSsid(String),
    RemoveSsid(String),
    SaveCreds { username: String, password: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Modal {
    None,
    Help,
    Confirm {
        text: String,
        yes: Box<Command>,
    },
    Input {
        kind: InputKind,
        title: String,
        value: String,
    },
    Creds {
        user: String,
        pass: String,
        field: CredField,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Good,
    Bad,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub text: String,
    pub kind: ToastKind,
    ttl: u16,
}

impl Toast {
    #[cfg(test)]
    pub(crate) fn for_test(text: &str, kind: ToastKind) -> Self {
        Self {
            text: text.to_string(),
            kind,
            ttl: TOAST_TICKS,
        }
    }
}

/// Results coming back from background threads.
#[derive(Debug)]
pub enum Msg {
    Net(NetState, Option<u64>),
    Progress(String),
    OpenDone(Result<Outcome, OpenError>),
    KeeperDone(Result<String, String>),
}

/// Everything read from disk, refreshed about once a second.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub username: Option<String>,
    pub keeper_running: bool,
    pub keeper_pid: Option<u32>,
    pub state: DaemonState,
    pub trusted: Vec<String>,
    pub ssids: Vec<String>,
    pub log: Vec<String>,
}

impl Snapshot {
    pub fn load(dir: &Path) -> Self {
        let lock = dir.join("daemon.lock");
        let keeper_running = instance::is_daemon_running(&lock);
        Self {
            username: config::load_credentials(dir).ok().map(|c| c.username),
            keeper_running,
            keeper_pid: if keeper_running {
                instance::read_lock_pid(&lock)
            } else {
                None
            },
            state: DaemonState::load(&dir.join("state.json")),
            trusted: TrustStore::open(dir).hosts(),
            ssids: config::Settings::load(dir).wifi_ssids,
            log: logger::tail(dir, 200),
        }
    }
}

pub struct App {
    pub snap: Snapshot,
    pub net: NetState,
    pub focus: Focus,
    pub portal_sel: usize,
    pub wifi_sel: usize,
    pub modal: Modal,
    pub toast: Option<Toast>,
    /// Set while a background job (sign-in, keeper start/stop) is running.
    pub busy: Option<String>,
    pub ping_ms: Option<u64>,
}

impl App {
    pub fn new(snap: Snapshot) -> Self {
        let mut app = Self {
            snap,
            net: NetState::Checking,
            focus: Focus::Portals,
            portal_sel: 0,
            wifi_sel: 0,
            modal: Modal::None,
            toast: None,
            busy: None,
            ping_ms: None,
        };
        app.clamp_selection();
        app
    }

    pub fn set_snapshot(&mut self, snap: Snapshot) {
        self.snap = snap;
        self.clamp_selection();
    }

    fn clamp_selection(&mut self) {
        self.portal_sel = self
            .portal_sel
            .min(self.snap.trusted.len().saturating_sub(1));
        self.wifi_sel = self.wifi_sel.min(self.snap.ssids.len().saturating_sub(1));
    }

    pub fn toast(&mut self, kind: ToastKind, text: impl Into<String>) {
        self.toast = Some(Toast {
            text: text.into(),
            kind,
            ttl: TOAST_TICKS,
        });
    }

    /// Called every 200 ms.
    pub fn tick(&mut self) {
        if let Some(t) = &mut self.toast {
            t.ttl = t.ttl.saturating_sub(1);
            if t.ttl == 0 {
                self.toast = None;
            }
        }
    }

    /// A portal that was found but is not trusted yet, if any.
    pub fn blocked_host(&self) -> Option<String> {
        if let NetState::Portal {
            host,
            trusted: false,
        } = &self.net
        {
            return Some(host.clone());
        }
        self.snap
            .state
            .pending_untrusted_gateway
            .clone()
            .filter(|h| !self.snap.trusted.contains(h))
    }

    pub fn on_msg(&mut self, msg: Msg) -> Command {
        match msg {
            Msg::Net(n, ping) => {
                self.net = n;
                self.ping_ms = ping;
                Command::None
            }
            Msg::Progress(text) => {
                self.busy = Some(text);
                Command::None
            }
            Msg::OpenDone(result) => {
                self.busy = None;
                match result {
                    Ok(Outcome::AlreadyOnline) => self.toast(ToastKind::Good, "Already online."),
                    Ok(Outcome::LoggedIn { gateway, .. }) => {
                        self.toast(ToastKind::Good, format!("Signed in via {gateway}."))
                    }
                    Err(OpenError::Untrusted(host)) => {
                        self.modal = Modal::Confirm {
                            text: format!(
                                "The portal at {host} isn't trusted yet.\nSend your password to it and sign in?"
                            ),
                            yes: Box::new(Command::TrustAndOpen(host)),
                        };
                    }
                    Err(e) => self.toast(ToastKind::Bad, e.to_string().replace('\n', " ")),
                }
                Command::Refresh
            }
            Msg::KeeperDone(result) => {
                self.busy = None;
                match result {
                    Ok(m) => self.toast(ToastKind::Good, m),
                    Err(e) => self.toast(ToastKind::Bad, e),
                }
                Command::Refresh
            }
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) -> Command {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Command::Quit;
        }
        match self.modal.clone() {
            Modal::None => self.on_main_key(key),
            Modal::Help => {
                self.modal = Modal::None;
                Command::None
            }
            Modal::Confirm { yes, .. } => {
                self.modal = Modal::None;
                if matches!(key.code, KeyCode::Char('y' | 'Y')) {
                    *yes
                } else {
                    Command::None
                }
            }
            Modal::Input { kind, title, value } => self.on_input_key(key, kind, title, value),
            Modal::Creds { user, pass, field } => self.on_creds_key(key, user, pass, field),
        }
    }

    fn on_main_key(&mut self, key: KeyEvent) -> Command {
        match key.code {
            KeyCode::Char('q') => Command::Quit,
            KeyCode::Char('r') => Command::Refresh,
            KeyCode::Char('?') | KeyCode::F(1) => {
                self.modal = Modal::Help;
                Command::None
            }
            KeyCode::Char('o') => {
                if let Some(b) = &self.busy {
                    let msg = format!("Busy: {b}");
                    self.toast(ToastKind::Info, msg);
                    Command::None
                } else {
                    Command::OpenNow
                }
            }
            KeyCode::Char('k') => {
                if let Some(b) = &self.busy {
                    let msg = format!("Busy: {b}");
                    self.toast(ToastKind::Info, msg);
                    Command::None
                } else {
                    Command::ToggleKeeper
                }
            }
            KeyCode::Char('t') => {
                match self.blocked_host() {
                    Some(host) => {
                        self.modal = Modal::Confirm {
                            text: format!(
                                "Trust the portal at {host}?\nYour password will be sent to it when you sign in."
                            ),
                            yes: Box::new(Command::TrustAndOpen(host)),
                        };
                    }
                    None => self.toast(ToastKind::Info, "Nothing is waiting for approval."),
                }
                Command::None
            }
            KeyCode::Char('c') => {
                self.modal = Modal::Creds {
                    user: self.snap.username.clone().unwrap_or_default(),
                    pass: String::new(),
                    field: if self.snap.username.is_some() {
                        CredField::Pass
                    } else {
                        CredField::User
                    },
                };
                Command::None
            }
            KeyCode::Char('w') => {
                self.modal = Modal::Input {
                    kind: InputKind::AddSsid,
                    title: "Wi-Fi network OpenUp may rejoin by itself".to_string(),
                    value: String::new(),
                };
                Command::None
            }
            KeyCode::Char('x') | KeyCode::Delete => {
                self.ask_remove_selected();
                Command::None
            }
            KeyCode::Tab | KeyCode::BackTab => {
                self.focus = match self.focus {
                    Focus::Portals => Focus::Wifi,
                    Focus::Wifi => Focus::Portals,
                };
                Command::None
            }
            KeyCode::Up => {
                self.move_selection(-1);
                Command::None
            }
            KeyCode::Down => {
                self.move_selection(1);
                Command::None
            }
            _ => Command::None,
        }
    }

    fn move_selection(&mut self, delta: i32) {
        let (sel, len) = match self.focus {
            Focus::Portals => (&mut self.portal_sel, self.snap.trusted.len()),
            Focus::Wifi => (&mut self.wifi_sel, self.snap.ssids.len()),
        };
        if len == 0 {
            return;
        }
        let next = (*sel as i32 + delta).clamp(0, len as i32 - 1);
        *sel = next as usize;
    }

    fn ask_remove_selected(&mut self) {
        match self.focus {
            Focus::Portals => match self.snap.trusted.get(self.portal_sel) {
                Some(host) => {
                    self.modal = Modal::Confirm {
                        text: format!(
                            "Stop trusting {host}?\nOpenUp will no longer send your password to it."
                        ),
                        yes: Box::new(Command::ForgetHost(host.clone())),
                    }
                }
                None => self.toast(ToastKind::Info, "No trusted portal selected."),
            },
            Focus::Wifi => match self.snap.ssids.get(self.wifi_sel) {
                Some(ssid) => {
                    self.modal = Modal::Confirm {
                        text: format!("Remove \"{ssid}\" from the Wi-Fi list?"),
                        yes: Box::new(Command::RemoveSsid(ssid.clone())),
                    }
                }
                None => self.toast(ToastKind::Info, "No Wi-Fi network selected."),
            },
        }
    }

    fn on_input_key(
        &mut self,
        key: KeyEvent,
        kind: InputKind,
        title: String,
        mut value: String,
    ) -> Command {
        match key.code {
            KeyCode::Esc => {
                self.modal = Modal::None;
                Command::None
            }
            KeyCode::Enter => {
                self.modal = Modal::None;
                let value = value.trim().to_string();
                if value.is_empty() {
                    return Command::None;
                }
                match kind {
                    InputKind::AddSsid => Command::AddSsid(value),
                }
            }
            KeyCode::Backspace => {
                value.pop();
                self.modal = Modal::Input { kind, title, value };
                Command::None
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                value.push(c);
                self.modal = Modal::Input { kind, title, value };
                Command::None
            }
            _ => Command::None,
        }
    }

    fn on_creds_key(
        &mut self,
        key: KeyEvent,
        mut user: String,
        mut pass: String,
        field: CredField,
    ) -> Command {
        let switch = |f: CredField| match f {
            CredField::User => CredField::Pass,
            CredField::Pass => CredField::User,
        };
        match key.code {
            KeyCode::Esc => {
                self.modal = Modal::None;
                Command::None
            }
            KeyCode::Tab | KeyCode::BackTab | KeyCode::Up | KeyCode::Down => {
                self.modal = Modal::Creds {
                    user,
                    pass,
                    field: switch(field),
                };
                Command::None
            }
            KeyCode::Enter => {
                if field == CredField::User {
                    self.modal = Modal::Creds {
                        user,
                        pass,
                        field: CredField::Pass,
                    };
                    return Command::None;
                }
                if user.trim().is_empty() || pass.is_empty() {
                    self.toast(ToastKind::Bad, "Username and password can't be empty.");
                    self.modal = Modal::Creds { user, pass, field };
                    return Command::None;
                }
                self.modal = Modal::None;
                Command::SaveCreds {
                    username: user.trim().to_string(),
                    password: pass,
                }
            }
            KeyCode::Backspace => {
                match field {
                    CredField::User => user.pop(),
                    CredField::Pass => pass.pop(),
                };
                self.modal = Modal::Creds { user, pass, field };
                Command::None
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                match field {
                    CredField::User => user.push(c),
                    CredField::Pass => pass.push(c),
                }
                self.modal = Modal::Creds { user, pass, field };
                Command::None
            }
            _ => Command::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyEventKind, KeyEventState};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn ch(c: char) -> KeyEvent {
        key(KeyCode::Char(c))
    }

    fn app_with(trusted: &[&str], ssids: &[&str]) -> App {
        let snap = Snapshot {
            trusted: trusted.iter().map(|s| s.to_string()).collect(),
            ssids: ssids.iter().map(|s| s.to_string()).collect(),
            ..Snapshot::default()
        };
        App::new(snap)
    }

    #[test]
    fn quit_keys() {
        let mut app = app_with(&[], &[]);
        assert_eq!(app.on_key(ch('q')), Command::Quit);
        let ctrl_c = KeyEvent {
            modifiers: KeyModifiers::CONTROL,
            ..ch('c')
        };
        assert_eq!(app.on_key(ctrl_c), Command::Quit);
    }

    #[test]
    fn ctrl_c_quits_even_inside_a_dialog() {
        let mut app = app_with(&[], &[]);
        app.on_key(ch('w'));
        assert!(matches!(app.modal, Modal::Input { .. }));
        let ctrl_c = KeyEvent {
            modifiers: KeyModifiers::CONTROL,
            ..ch('c')
        };
        assert_eq!(app.on_key(ctrl_c), Command::Quit);
    }

    #[test]
    fn sign_in_and_keeper_keys_are_blocked_while_busy() {
        let mut app = app_with(&[], &[]);
        assert_eq!(app.on_key(ch('o')), Command::OpenNow);
        assert_eq!(app.on_key(ch('k')), Command::ToggleKeeper);

        app.busy = Some("Signing in...".into());
        assert_eq!(app.on_key(ch('o')), Command::None);
        assert_eq!(app.on_key(ch('k')), Command::None);
        assert!(app.toast.as_ref().unwrap().text.contains("Busy"));
    }

    #[test]
    fn tab_switches_focus_and_arrows_move_within_the_focused_list() {
        let mut app = app_with(&["a", "b", "c"], &["x", "y"]);
        assert_eq!(app.focus, Focus::Portals);
        app.on_key(key(KeyCode::Down));
        app.on_key(key(KeyCode::Down));
        app.on_key(key(KeyCode::Down)); // clamped
        assert_eq!(app.portal_sel, 2);
        app.on_key(key(KeyCode::Up));
        assert_eq!(app.portal_sel, 1);

        app.on_key(key(KeyCode::Tab));
        assert_eq!(app.focus, Focus::Wifi);
        app.on_key(key(KeyCode::Down));
        assert_eq!(app.wifi_sel, 1);
        assert_eq!(app.portal_sel, 1, "the other list's selection is untouched");
        app.on_key(key(KeyCode::BackTab));
        assert_eq!(app.focus, Focus::Portals);
    }

    #[test]
    fn selection_is_clamped_when_the_lists_shrink() {
        let mut app = app_with(&["a", "b", "c"], &[]);
        app.portal_sel = 2;
        app.set_snapshot(Snapshot {
            trusted: vec!["a".into()],
            ..Snapshot::default()
        });
        assert_eq!(app.portal_sel, 0);
    }

    #[test]
    fn removing_requires_confirmation_and_only_y_confirms() {
        let mut app = app_with(&["10.0.0.1", "10.0.0.2"], &[]);
        app.portal_sel = 1;
        assert_eq!(app.on_key(ch('x')), Command::None);
        assert!(matches!(app.modal, Modal::Confirm { .. }));

        assert_eq!(app.on_key(key(KeyCode::Enter)), Command::None);
        assert_eq!(app.modal, Modal::None);

        app.on_key(ch('x'));
        assert_eq!(app.on_key(ch('y')), Command::ForgetHost("10.0.0.2".into()));
        assert_eq!(app.modal, Modal::None);
    }

    #[test]
    fn removing_a_wifi_network_targets_the_wifi_list() {
        let mut app = app_with(&["p"], &["Campus", "Hostel"]);
        app.on_key(key(KeyCode::Tab));
        app.on_key(key(KeyCode::Down));
        app.on_key(ch('x'));
        assert_eq!(app.on_key(ch('y')), Command::RemoveSsid("Hostel".into()));
    }

    #[test]
    fn removing_from_an_empty_list_just_explains() {
        let mut app = app_with(&[], &[]);
        assert_eq!(app.on_key(ch('x')), Command::None);
        assert_eq!(app.modal, Modal::None);
        assert!(app.toast.is_some());
    }

    #[test]
    fn trust_key_needs_a_blocked_portal() {
        let mut app = app_with(&[], &[]);
        app.on_key(ch('t'));
        assert_eq!(app.modal, Modal::None);
        assert!(app.toast.as_ref().unwrap().text.contains("Nothing"));

        app.net = NetState::Portal {
            host: "10.9.9.9".into(),
            trusted: false,
        };
        app.on_key(ch('t'));
        assert!(matches!(app.modal, Modal::Confirm { .. }));
        assert_eq!(
            app.on_key(ch('y')),
            Command::TrustAndOpen("10.9.9.9".into())
        );
    }

    #[test]
    fn a_trusted_portal_is_not_offered_for_approval() {
        let mut app = app_with(&["10.0.0.1"], &[]);
        app.net = NetState::Portal {
            host: "10.0.0.1".into(),
            trusted: true,
        };
        assert_eq!(app.blocked_host(), None);

        app.net = NetState::Online;
        app.snap.state.pending_untrusted_gateway = Some("10.0.0.1".into());
        assert_eq!(app.blocked_host(), None);
        app.snap.state.pending_untrusted_gateway = Some("10.7.7.7".into());
        assert_eq!(app.blocked_host().as_deref(), Some("10.7.7.7"));
    }

    #[test]
    fn adding_wifi_types_into_a_dialog_then_emits_a_command() {
        let mut app = app_with(&[], &[]);
        app.on_key(ch('w'));
        for c in "Campus WiFi".chars() {
            app.on_key(ch(c));
        }
        app.on_key(key(KeyCode::Backspace));
        for c in "i".chars() {
            app.on_key(ch(c));
        }
        assert_eq!(
            app.on_key(key(KeyCode::Enter)),
            Command::AddSsid("Campus WiFi".into())
        );
        assert_eq!(app.modal, Modal::None);
    }

    #[test]
    fn empty_input_and_escape_do_nothing() {
        let mut app = app_with(&[], &[]);
        app.on_key(ch('w'));
        assert_eq!(app.on_key(key(KeyCode::Enter)), Command::None);
        app.on_key(ch('w'));
        app.on_key(ch('z'));
        assert_eq!(app.on_key(key(KeyCode::Esc)), Command::None);
        assert_eq!(app.modal, Modal::None);
    }

    #[test]
    fn credentials_dialog_flow() {
        let mut app = app_with(&[], &[]);
        app.on_key(ch('c')); // no saved username -> starts on the username field
        for c in "alice".chars() {
            app.on_key(ch(c));
        }
        assert_eq!(app.on_key(key(KeyCode::Enter)), Command::None); // moves to password
        for c in "hunter2".chars() {
            app.on_key(ch(c));
        }
        assert_eq!(
            app.on_key(key(KeyCode::Enter)),
            Command::SaveCreds {
                username: "alice".into(),
                password: "hunter2".into()
            }
        );
        assert_eq!(app.modal, Modal::None);
    }

    #[test]
    fn credentials_dialog_rejects_an_empty_password_and_stays_open() {
        let mut app = app_with(&[], &[]);
        app.snap.username = Some("alice".into());
        app.on_key(ch('c')); // prefilled, jumps straight to the password field
        match &app.modal {
            Modal::Creds { user, field, .. } => {
                assert_eq!(user, "alice");
                assert_eq!(*field, CredField::Pass);
            }
            other => panic!("unexpected modal {other:?}"),
        }
        assert_eq!(app.on_key(key(KeyCode::Enter)), Command::None);
        assert!(matches!(app.modal, Modal::Creds { .. }));
        assert_eq!(app.toast.as_ref().unwrap().kind, ToastKind::Bad);
    }

    #[test]
    fn untrusted_sign_in_result_opens_the_trust_dialog() {
        let mut app = app_with(&[], &[]);
        app.busy = Some("Signing in...".into());
        let cmd = app.on_msg(Msg::OpenDone(Err(OpenError::Untrusted("10.1.1.9".into()))));
        assert_eq!(cmd, Command::Refresh);
        assert!(app.busy.is_none());
        assert_eq!(
            app.on_key(ch('y')),
            Command::TrustAndOpen("10.1.1.9".into())
        );
    }

    #[test]
    fn sign_in_results_become_toasts() {
        let mut app = app_with(&[], &[]);
        app.on_msg(Msg::OpenDone(Ok(Outcome::LoggedIn {
            gateway: "http://10.1.1.9:8090".into(),
            message: "ok".into(),
        })));
        let t = app.toast.clone().unwrap();
        assert_eq!(t.kind, ToastKind::Good);
        assert!(t.text.contains("10.1.1.9"));

        app.on_msg(Msg::OpenDone(Err(OpenError::BadCredentials(
            "Invalid password".into(),
        ))));
        let t = app.toast.clone().unwrap();
        assert_eq!(t.kind, ToastKind::Bad);
        assert!(!t.text.contains('\n'), "toasts are single-line");
    }

    #[test]
    fn toasts_expire() {
        let mut app = app_with(&[], &[]);
        app.toast(ToastKind::Info, "hi");
        for _ in 0..TOAST_TICKS {
            app.tick();
        }
        assert!(app.toast.is_none());
    }

    #[test]
    fn help_dialog_closes_on_any_key() {
        let mut app = app_with(&[], &[]);
        app.on_key(ch('?'));
        assert_eq!(app.modal, Modal::Help);
        assert_eq!(app.on_key(ch('z')), Command::None);
        assert_eq!(app.modal, Modal::None);
    }
}
