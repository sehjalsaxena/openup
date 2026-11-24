//! Rendering. Takes `&App`, draws a frame, owns no state.

use super::app::{App, CredField, Focus, MIN_HEIGHT, MIN_WIDTH, Modal, NetState, ToastKind};
use crate::brand;
use crate::logger;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, List, ListItem, ListState, Paragraph, Wrap};

const DIM: Color = Color::DarkGray;
const ACCENT: Color = Color::Cyan;

pub fn render(f: &mut Frame, app: &App) {
    let area = f.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        let msg = format!(
            "Terminal too small ({}x{}). {} needs at least {}x{}. Resize the window, or press q to quit.",
            area.width,
            area.height,
            brand::DISPLAY_NAME,
            MIN_WIDTH,
            MIN_HEIGHT
        );
        f.render_widget(Paragraph::new(msg).wrap(Wrap { trim: true }), area);
        return;
    }

    let [header, body, toast_row, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(8),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);
    let [top, log] = Layout::vertical([Constraint::Length(11), Constraint::Min(4)]).areas(body);
    let [status, right] =
        Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(top);
    let [portals, wifi] =
        Layout::vertical([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(right);

    render_header(f, header, app);
    render_status(f, status, app);
    render_portals(f, portals, app);
    render_wifi(f, wifi, app);
    render_log(f, log, app);
    render_toast(f, toast_row, app);
    render_footer(f, footer);

    match &app.modal {
        Modal::None => {}
        Modal::Help => render_help(f, area),
        Modal::Confirm { text, .. } => render_confirm(f, area, text),
        Modal::Input { title, value, .. } => render_input(f, area, title, value),
        Modal::Creds { user, pass, field } => render_creds(f, area, user, pass, *field),
    }
}

fn panel<'a>(title: &'a str, focused: bool) -> Block<'a> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(if focused { ACCENT } else { DIM }))
        .title(Line::from(format!(" {title} ")).style(Style::new().add_modifier(Modifier::BOLD)))
}

fn dim(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), Style::new().fg(DIM))
}

fn colored(text: impl Into<String>, color: Color) -> Span<'static> {
    Span::styled(text.into(), Style::new().fg(color))
}

/// Clips `text` to `max` characters, ending in an ellipsis when it was cut.
fn fit(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else if max == 0 {
        String::new()
    } else {
        let mut out: String = text.chars().take(max - 1).collect();
        out.push('…');
        out
    }
}

fn kv(label: &str, mut value: Vec<Span<'static>>) -> Line<'static> {
    let mut spans = vec![dim(format!("{label:<12}"))];
    spans.append(&mut value);
    Line::from(spans)
}

fn net_badge(net: &NetState) -> (&'static str, Color) {
    match net {
        NetState::Checking => ("o CHECKING", Color::Gray),
        NetState::Online => ("* ONLINE", Color::Green),
        NetState::Portal { trusted: true, .. } => ("* BEHIND A PORTAL", Color::Yellow),
        NetState::Portal { trusted: false, .. } => ("! PORTAL NOT TRUSTED", Color::Yellow),
        NetState::Offline => ("x OFFLINE", Color::Red),
        NetState::Error(_) => ("x PROBE ERROR", Color::Red),
    }
}

fn render_header(f: &mut Frame, area: Rect, app: &App) {
    let (badge, color) = net_badge(&app.net);
    let keeper = if app.snap.keeper_running {
        colored("* Keeper running", Color::Green)
    } else {
        dim("o Keeper stopped")
    };
    let line = Line::from(vec![
        Span::styled(badge, Style::new().fg(color).add_modifier(Modifier::BOLD)),
        Span::raw("    "),
        keeper,
    ]);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(ACCENT))
        .title(Line::from(Span::styled(
            format!(" {} ", brand::DISPLAY_NAME.to_uppercase()),
            Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
        )))
        .title(Line::from(dim(format!(" v{} ", env!("CARGO_PKG_VERSION")))).right_aligned());
    f.render_widget(Paragraph::new(line).block(block), area);
}

fn render_status(f: &mut Frame, area: Rect, app: &App) {
    let s = &app.snap;
    let st = &s.state;
    let avail = area.width.saturating_sub(2 + 12) as usize;

    let internet = match &app.net {
        NetState::Checking => vec![dim("checking...")],
        NetState::Online => {
            let mut v = vec![colored("online", Color::Green)];
            let mut details = Vec::new();
            if let Some(ip) = &st.last_local_ip {
                details.push(ip.clone());
            }
            if let Some(ms) = app.ping_ms {
                details.push(format!("{ms} ms"));
            }
            if !details.is_empty() {
                v.push(dim(format!("  ({})", details.join(", "))));
            }
            v
        }
        NetState::Portal { host, trusted } => vec![
            colored("locked behind ", Color::Yellow),
            Span::raw(host.clone()),
            if *trusted {
                dim("  (trusted)")
            } else {
                colored("  (not trusted)", Color::Yellow)
            },
        ],
        NetState::Offline => vec![colored("no connection", Color::Red)],
        NetState::Error(e) => vec![colored(format!("probe failed: {e}"), Color::Red)],
    };

    let keeper = if s.keeper_running {
        let pid = s
            .keeper_pid
            .map(|p| format!("  (pid {p})"))
            .unwrap_or_default();
        vec![colored("running", Color::Green), dim(pid)]
    } else {
        vec![dim("stopped")]
    };

    let breaker = if st.halted_bad_credentials {
        vec![colored("PAUSED (bad credentials)", Color::Red)]
    } else {
        vec![colored("active", Color::Green)]
    };

    let last_login = if st.last_login_epoch > 0 {
        format!("  (last {})", logger::format_utc(st.last_login_epoch))
    } else {
        String::new()
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let session_display = if matches!(app.net, NetState::Online) {
        if let Some(dur_str) = st.session_duration_human(now) {
            vec![colored(dur_str, Color::Green), dim(" (online)")]
        } else {
            vec![colored("just started", Color::Green)]
        }
    } else {
        vec![dim("-")]
    };

    let power_display = match crate::power::current_power_state() {
        crate::power::PowerState::Battery => {
            vec![colored("battery", Color::Yellow), dim(" (saving power)")]
        }
        crate::power::PowerState::Ac => vec![colored("AC", Color::Green)],
        crate::power::PowerState::Unknown => vec![dim("unknown")],
    };

    let account = match &s.username {
        Some(u) => vec![Span::raw(u.clone())],
        None => vec![colored("not saved: press c", Color::Yellow)],
    };

    let mut lines = vec![kv("Internet", internet)];

    if !app.ping_history.is_empty() {
        let spark = app.ping_sparkline();
        let ping_text = app.ping_ms.map(|m| format!("{m} ms ")).unwrap_or_default();
        lines.push(kv(
            "Latency",
            vec![Span::raw(ping_text), colored(spark, Color::Cyan)],
        ));
    }

    lines.extend([
        kv("Session", session_display),
        kv("Keeper", keeper),
        kv("Power", power_display),
        kv("Account", account),
        kv(
            "Gateway",
            vec![Span::raw(fit(
                st.active_gateway.as_deref().unwrap_or("-"),
                avail,
            ))],
        ),
        kv(
            "Logins",
            vec![
                Span::raw(format!("{} total", st.total_logins)),
                if st.daily_logins > 0 {
                    dim(format!(" ({} today)", st.daily_logins))
                } else {
                    dim(last_login)
                },
            ],
        ),
        kv("Autopilot", breaker),
        kv(
            "Last event",
            vec![dim(fit(
                if st.last_status_message.is_empty() {
                    "-"
                } else {
                    &st.last_status_message
                },
                avail,
            ))],
        ),
    ]);
    f.render_widget(Paragraph::new(lines).block(panel("Status", false)), area);
}

fn list_style(focused: bool) -> Style {
    if focused {
        Style::new().add_modifier(Modifier::REVERSED)
    } else {
        Style::new().add_modifier(Modifier::BOLD)
    }
}

fn render_portals(f: &mut Frame, area: Rect, app: &App) {
    let focused = app.focus == Focus::Portals;
    let mut items: Vec<ListItem> = app
        .snap
        .trusted
        .iter()
        .map(|h| ListItem::new(h.clone()))
        .collect();

    if items.is_empty() {
        items.push(ListItem::new(Line::from(dim(
            "none yet - press o on campus Wi-Fi",
        ))));
    }
    if let Some(host) = app.blocked_host() {
        items.push(ListItem::new(Line::from(colored(
            format!("! {host}  blocked - press t"),
            Color::Yellow,
        ))));
    }

    let mut state = ListState::default();
    if !app.snap.trusted.is_empty() {
        state.select(Some(app.portal_sel));
    }
    let list = List::new(items)
        .block(panel("Trusted portals", focused))
        .highlight_style(list_style(focused))
        .highlight_symbol("> ");
    f.render_stateful_widget(list, area, &mut state);
}

fn render_wifi(f: &mut Frame, area: Rect, app: &App) {
    let focused = app.focus == Focus::Wifi;
    let mut items: Vec<ListItem> = app
        .snap
        .ssids
        .iter()
        .map(|s| ListItem::new(s.clone()))
        .collect();
    if items.is_empty() {
        items.push(ListItem::new(Line::from(dim("off - press w to add one"))));
    }

    let mut state = ListState::default();
    if !app.snap.ssids.is_empty() {
        state.select(Some(app.wifi_sel));
    }
    let list = List::new(items)
        .block(panel("Wi-Fi it may rejoin", focused))
        .highlight_style(list_style(focused))
        .highlight_symbol("> ");
    f.render_stateful_widget(list, area, &mut state);
}

/// `[2026-11-18 11:03:00Z] message` -> (`11:03:00`, `message`)
fn split_log_line(line: &str) -> (String, String) {
    if let Some(rest) = line.strip_prefix('[')
        && let Some((ts, msg)) = rest.split_once("] ")
    {
        let time = ts.get(11..19).unwrap_or(ts);
        return (time.to_string(), msg.to_string());
    }
    (String::new(), line.to_string())
}

fn log_color(msg: &str) -> Color {
    let m = msg.to_lowercase();
    if m.contains("halted") || m.contains("error") || m.contains("failed") {
        Color::Red
    } else if m.contains("blocked") || m.contains("still locked") || m.contains("wake") {
        Color::Yellow
    } else if m.contains("verified") || m.contains("online") || m.contains("started") {
        Color::Green
    } else {
        Color::Reset
    }
}

fn render_log(f: &mut Frame, area: Rect, app: &App) {
    let height = area.height.saturating_sub(2) as usize;
    let log = &app.snap.log;
    let start = log.len().saturating_sub(height);

    let lines: Vec<Line> = if log.is_empty() {
        vec![Line::from(dim(
            "Nothing yet. The keeper writes here when it signs in, roams or gets blocked.",
        ))]
    } else {
        log[start..]
            .iter()
            .map(|l| {
                let (ts, msg) = split_log_line(l);
                Line::from(vec![
                    dim(format!("{ts} ")),
                    Span::styled(msg.clone(), Style::new().fg(log_color(&msg))),
                ])
            })
            .collect()
    };
    f.render_widget(
        Paragraph::new(lines)
            .block(panel("Activity", false))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_toast(f: &mut Frame, area: Rect, app: &App) {
    let line = if let Some(b) = &app.busy {
        Line::from(colored(format!(" ... {b}"), Color::Yellow))
    } else if let Some(t) = &app.toast {
        let color = match t.kind {
            ToastKind::Info => Color::Gray,
            ToastKind::Good => Color::Green,
            ToastKind::Bad => Color::Red,
        };
        Line::from(colored(format!(" {}", t.text), color))
    } else {
        Line::default()
    };
    f.render_widget(Paragraph::new(line), area);
}

fn render_footer(f: &mut Frame, area: Rect) {
    let optional = [
        ("o", "sign in"),
        ("k", "keeper"),
        ("t", "trust"),
        ("c", "login"),
        ("w", "+wifi"),
        ("x", "remove"),
        ("Tab", "focus"),
    ];
    let always = [("?", "help"), ("q", "quit")];
    let cost = |(k, l): &(&str, &str)| k.chars().count() + 1 + l.chars().count() + 2;

    let mut budget =
        (area.width as usize).saturating_sub(1 + always.iter().map(cost).sum::<usize>());
    let mut shown = Vec::new();
    for hint in optional {
        if cost(&hint) > budget {
            break;
        }
        budget -= cost(&hint);
        shown.push(hint);
    }
    shown.extend(always);

    let mut spans = vec![Span::raw(" ")];
    for (key, label) in shown {
        spans.push(Span::styled(
            key,
            Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
        ));
        spans.push(dim(format!(" {label}  ")));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    }
}

fn dialog(f: &mut Frame, area: Rect, title: &str, width: u16, lines: Vec<Line<'static>>) {
    let rect = centered(area, width, lines.len() as u16 + 2);
    f.render_widget(Clear, rect);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(ACCENT))
        .title(Line::from(format!(" {title} ")).style(Style::new().add_modifier(Modifier::BOLD)));
    f.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        rect,
    );
}

fn render_help(f: &mut Frame, area: Rect) {
    let row = |k: &str, d: &str| {
        Line::from(vec![
            Span::styled(
                format!(" {k:<8}"),
                Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::raw(d.to_string()),
        ])
    };
    let lines = vec![
        row("o", "Sign in now (asks first if the portal is new)"),
        row("k", "Start or stop the background keeper"),
        row("t", "Trust the portal that was blocked"),
        row("c", "Save or replace your username and password"),
        row("w", "Add a Wi-Fi network OpenUp may rejoin"),
        row("x", "Remove the selected item (Del works too)"),
        row("Tab", "Switch between the two lists"),
        row("Up/Down", "Move the selection"),
        row("r", "Refresh now"),
        row("q", "Quit (the keeper keeps running)"),
        Line::default(),
        Line::from(dim(" Press any key to close")),
    ];
    dialog(f, area, "Keys", 62, lines);
}

fn render_confirm(f: &mut Frame, area: Rect, text: &str) {
    let mut lines: Vec<Line> = text.lines().map(|l| Line::from(format!(" {l}"))).collect();
    lines.push(Line::default());
    lines.push(Line::from(vec![
        Span::raw(" "),
        Span::styled(
            "y",
            Style::new().fg(Color::Green).add_modifier(Modifier::BOLD),
        ),
        dim(" yes     any other key: cancel"),
    ]));
    dialog(f, area, "Please confirm", 64, lines);
}

fn render_input(f: &mut Frame, area: Rect, title: &str, value: &str) {
    let lines = vec![
        Line::from(vec![
            Span::raw(" > "),
            Span::raw(value.to_string()),
            colored("_", ACCENT),
        ]),
        Line::default(),
        Line::from(dim(" Enter: add     Esc: cancel")),
    ];
    dialog(f, area, title, 62, lines);
}

fn render_creds(f: &mut Frame, area: Rect, user: &str, pass: &str, field: CredField) {
    let cursor = colored("_", ACCENT);
    let marker = |active: bool| if active { " > " } else { "   " };

    let mut user_line = vec![
        dim(marker(field == CredField::User)),
        Span::raw("Username  "),
        Span::raw(user.to_string()),
    ];
    let mut pass_line = vec![
        dim(marker(field == CredField::Pass)),
        Span::raw("Password  "),
        Span::raw("*".repeat(pass.chars().count())),
    ];
    match field {
        CredField::User => user_line.push(cursor),
        CredField::Pass => pass_line.push(cursor),
    }

    let lines = vec![
        Line::from(user_line),
        Line::from(pass_line),
        Line::default(),
        Line::from(dim(" Enter: next / save     Tab: switch     Esc: cancel")),
        Line::from(dim(" Saved privately on this computer; never shown again.")),
    ];
    dialog(f, area, "Your campus login", 64, lines);
}

#[cfg(test)]
pub(crate) fn buffer_text(buf: &ratatui::buffer::Buffer) -> String {
    let mut out = String::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            out.push_str(buf[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::DaemonState;
    use crate::tui::app::{Snapshot, Toast};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn draw(app: &App, w: u16, h: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| render(f, app)).unwrap();
        buffer_text(terminal.backend().buffer())
    }

    fn sample_app() -> App {
        let state = DaemonState {
            total_logins: 12,
            last_login_epoch: 1_700_000_000,
            last_local_ip: Some("172.16.4.21".into()),
            active_gateway: Some("http://172.16.68.6:8090/login.xml".into()),
            last_status_message: "Online and unlocked".into(),
            ..DaemonState::default()
        };
        let snap = Snapshot {
            username: Some("alice".into()),
            keeper_running: true,
            keeper_pid: Some(4242),
            state,
            trusted: vec!["172.16.68.6".into(), "10.20.0.1".into()],
            ssids: vec!["College_WiFi".into()],
            log: vec![
                "[2026-11-18 09:01:02Z] Keeper started (pid 4242)".into(),
                "[2026-11-18 09:01:05Z] Login verified at http://172.16.68.6:8090".into(),
                "[2026-11-18 10:30:00Z] Blocked login: portal host 10.9.9.9 is not trusted".into(),
            ],
        };
        let mut app = App::new(snap);
        app.net = NetState::Online;
        app
    }

    /// Prints the dashboard for the README. Regenerate with:
    /// `cargo test render_readme_screenshot -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn render_readme_screenshot() {
        let text = draw(&sample_app(), 100, 24);
        for line in text.lines() {
            println!("{}", line.trim_end());
        }
    }

    #[test]
    fn main_screen_shows_the_essentials() {
        let text = draw(&sample_app(), 100, 30);
        for needle in [
            "OPENUP",
            "ONLINE",
            "Keeper running",
            "pid 4242",
            "alice",
            "172.16.4.21",
            "12",
            "Trusted portals",
            "172.16.68.6",
            "10.20.0.1",
            "Wi-Fi it may rejoin",
            "College_WiFi",
            "Activity",
            "Login verified",
            "sign in",
            "quit",
        ] {
            assert!(text.contains(needle), "missing {needle:?} in:\n{text}");
        }
    }

    #[test]
    fn a_portal_that_is_not_trusted_is_called_out() {
        let mut app = sample_app();
        app.net = NetState::Portal {
            host: "10.9.9.9".into(),
            trusted: false,
        };
        let text = draw(&app, 100, 30);
        assert!(text.contains("PORTAL NOT TRUSTED"), "{text}");
        assert!(text.contains("blocked - press t"), "{text}");
    }

    #[test]
    fn paused_autologin_and_missing_credentials_are_visible() {
        let mut app = sample_app();
        app.snap.state.halted_bad_credentials = true;
        app.snap.username = None;
        let text = draw(&app, 100, 30);
        assert!(text.contains("PAUSED"), "{text}");
        assert!(text.contains("press c"), "{text}");
    }

    #[test]
    fn empty_lists_explain_what_to_do() {
        let mut app = App::new(Snapshot::default());
        app.net = NetState::Offline;
        let text = draw(&app, 100, 30);
        assert!(text.contains("OFFLINE"));
        assert!(text.contains("none yet"));
        assert!(text.contains("press w to add one"));
        assert!(text.contains("Keeper stopped"));
        assert!(text.contains("Nothing yet"));
    }

    #[test]
    fn nothing_important_is_cut_off_at_the_minimum_size() {
        let mut app = sample_app();
        app.snap.state.last_status_message = "Authenticated".into();
        let ok = draw(&app, MIN_WIDTH, MIN_HEIGHT);
        for needle in [
            "Internet",
            "Keeper",
            "Account",
            "Gateway",
            "Logins",
            "Autopilot",
            "Last event",
            "Activity",
            "quit",
        ] {
            assert!(
                ok.contains(needle),
                "{needle:?} is cut off at {MIN_WIDTH}x{MIN_HEIGHT}:\n{ok}"
            );
        }
    }

    #[test]
    fn works_at_the_minimum_size_and_tells_you_when_too_small() {
        let ok = draw(&sample_app(), MIN_WIDTH, MIN_HEIGHT);
        assert!(ok.contains("OPENUP") && ok.contains("Status"), "{ok}");
        let small = draw(&sample_app(), MIN_WIDTH - 1, MIN_HEIGHT);
        assert!(small.contains("too small"), "{small}");
    }

    #[test]
    fn never_panics_at_odd_sizes() {
        let app = sample_app();
        for (w, h) in [
            (1, 1),
            (10, 3),
            (59, 17),
            (60, 18),
            (61, 19),
            (200, 60),
            (300, 5),
        ] {
            let _ = draw(&app, w, h);
        }
    }

    #[test]
    fn dialogs_render_over_the_main_screen() {
        let mut app = sample_app();

        app.modal = Modal::Help;
        assert!(draw(&app, 100, 30).contains("Switch between the two lists"));

        app.modal = Modal::Confirm {
            text: "Stop trusting 10.20.0.1?".into(),
            yes: Box::new(crate::tui::app::Command::None),
        };
        let t = draw(&app, 100, 30);
        assert!(
            t.contains("Please confirm") && t.contains("Stop trusting 10.20.0.1?"),
            "{t}"
        );

        app.modal = Modal::Input {
            kind: crate::tui::app::InputKind::AddSsid,
            title: "Wi-Fi network OpenUp may rejoin by itself".into(),
            value: "Hostel_Net".into(),
        };
        let t = draw(&app, 100, 30);
        assert!(t.contains("Hostel_Net") && t.contains("Esc: cancel"), "{t}");
    }

    #[test]
    fn the_password_is_never_drawn() {
        let mut app = sample_app();
        app.modal = Modal::Creds {
            user: "alice".into(),
            pass: "hunter2".into(),
            field: CredField::Pass,
        };
        let t = draw(&app, 100, 30);
        assert!(t.contains("Username") && t.contains("alice"));
        assert!(
            !t.contains("hunter2"),
            "password leaked onto the screen:\n{t}"
        );
        assert!(t.contains("*******"));
    }

    #[test]
    fn busy_and_toast_lines_show_up() {
        let mut app = sample_app();
        app.busy = Some("Signing in...".into());
        assert!(draw(&app, 100, 30).contains("Signing in..."));
        app.busy = None;
        app.toast = Some(Toast::for_test("Keeper started.", ToastKind::Good));
        assert!(draw(&app, 100, 30).contains("Keeper started."));
    }

    #[test]
    fn log_lines_are_shortened_to_the_time_of_day() {
        assert_eq!(
            split_log_line("[2026-11-18 09:01:02Z] Keeper started"),
            ("09:01:02".to_string(), "Keeper started".to_string())
        );
        assert_eq!(
            split_log_line("plain"),
            (String::new(), "plain".to_string())
        );
        let text = draw(&sample_app(), 100, 30);
        assert!(text.contains("09:01:05"));
        assert!(
            !text.contains("2026-11-18"),
            "the date is dropped to save width"
        );
    }
}
