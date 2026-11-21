use crate::config::Settings;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub title: String,
    pub body: String,
}

pub fn is_enabled(config_dir: &Path) -> bool {
    Settings::load(config_dir).notifications
}

pub fn login_success_notification(account: &str, gateway: &str) -> Notification {
    Notification {
        title: "OpenUp: Signed In".to_string(),
        body: format!("Connected as {account} via {gateway}. Internet is active."),
    }
}

pub fn login_failed_notification(reason: &str) -> Notification {
    Notification {
        title: "OpenUp: Login Failed".to_string(),
        body: format!("{reason}. Run 'openup creds' to update."),
    }
}

pub fn portal_blocked_notification(host: &str) -> Notification {
    Notification {
        title: "OpenUp: Portal Blocked".to_string(),
        body: format!("Host {host} is not trusted. Run 'openup portals trust' to approve."),
    }
}

pub fn wifi_connected_notification(ssid: &str) -> Notification {
    Notification {
        title: "OpenUp: Wi-Fi Connected".to_string(),
        body: format!("Reconnected to allowed network: {ssid}."),
    }
}

pub fn circuit_breaker_notification(attempts: u32) -> Notification {
    Notification {
        title: "OpenUp: Autopilot Paused".to_string(),
        body: format!(
            "Portal accepted login {attempts} times but internet stayed locked. Run 'openup creds'."
        ),
    }
}

pub fn notify_login_success(config_dir: &Path, account: &str, gateway: &str) {
    if !is_enabled(config_dir) {
        return;
    }
    let notif = login_success_notification(account, gateway);
    send_desktop_notification(&notif.title, &notif.body);
}

pub fn notify_login_failed(config_dir: &Path, reason: &str) {
    if !is_enabled(config_dir) {
        return;
    }
    let notif = login_failed_notification(reason);
    send_desktop_notification(&notif.title, &notif.body);
}

pub fn notify_portal_blocked(config_dir: &Path, host: &str) {
    if !is_enabled(config_dir) {
        return;
    }
    let notif = portal_blocked_notification(host);
    send_desktop_notification(&notif.title, &notif.body);
}

pub fn notify_wifi_connected(config_dir: &Path, ssid: &str) {
    if !is_enabled(config_dir) {
        return;
    }
    let notif = wifi_connected_notification(ssid);
    send_desktop_notification(&notif.title, &notif.body);
}

pub fn notify_circuit_breaker(config_dir: &Path, attempts: u32) {
    if !is_enabled(config_dir) {
        return;
    }
    let notif = circuit_breaker_notification(attempts);
    send_desktop_notification(&notif.title, &notif.body);
}

pub fn all_sample_notifications() -> Vec<Notification> {
    vec![
        login_success_notification("alice", "172.16.68.6"),
        login_failed_notification("Password was rejected by portal"),
        portal_blocked_notification("10.9.9.9"),
        wifi_connected_notification("College_WiFi"),
        circuit_breaker_notification(3),
    ]
}

pub fn send_desktop_notification(title: &str, body: &str) {
    if std::env::var("CI").is_ok()
        || std::env::var("GITHUB_ACTIONS").is_ok()
        || std::env::var("OPENUP_NO_NOTIFY").is_ok()
    {
        return;
    }
    let title = title.to_string();
    let body = body.to_string();
    std::thread::spawn(move || {
        send_platform(&title, &body);
    });
}

#[cfg(target_os = "windows")]
fn send_platform(title: &str, body: &str) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let ps_script = r#"
        try {
            [Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null
            $template = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02)
            $texts = $template.GetElementsByTagName('text')
            $texts.Item(0).AppendChild($template.CreateTextNode($env:OPENUP_TOAST_TITLE)) > $null
            $texts.Item(1).AppendChild($template.CreateTextNode($env:OPENUP_TOAST_BODY)) > $null
            $notifier = [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('OpenUp')
            $notifier.Show([Windows.UI.Notifications.ToastNotification]::new($template))
        } catch {}
    "#;

    let _ = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", ps_script])
        .env("OPENUP_TOAST_TITLE", title)
        .env("OPENUP_TOAST_BODY", body)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}

#[cfg(target_os = "macos")]
fn send_platform(title: &str, body: &str) {
    let safe_title = title.replace('"', "\\\"");
    let safe_body = body.replace('"', "\\\"");
    let script = format!(r#"display notification "{safe_body}" with title "{safe_title}""#);
    let _ = std::process::Command::new("osascript")
        .args(["-e", &script])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

#[cfg(target_os = "linux")]
fn send_platform(title: &str, body: &str) {
    let _ = std::process::Command::new("notify-send")
        .args(["-a", "OpenUp", title, body])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn send_platform(_title: &str, _body: &str) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notifications_never_contain_em_dashes() {
        let samples = all_sample_notifications();
        for notif in &samples {
            assert!(
                !notif.title.contains('—'),
                "Title '{}' contains an em-dash",
                notif.title
            );
            assert!(
                !notif.body.contains('—'),
                "Body '{}' contains an em-dash",
                notif.body
            );
        }
    }

    #[test]
    fn notification_content_is_clean_and_informative() {
        let n = login_success_notification("testuser", "10.0.0.1");
        assert_eq!(n.title, "OpenUp: Signed In");
        assert!(n.body.contains("testuser"));
        assert!(n.body.contains("10.0.0.1"));

        let fail = login_failed_notification("Invalid password");
        assert_eq!(fail.title, "OpenUp: Login Failed");
        assert!(fail.body.contains("Invalid password"));

        let blocked = portal_blocked_notification("192.168.1.1");
        assert_eq!(blocked.title, "OpenUp: Portal Blocked");
        assert!(blocked.body.contains("192.168.1.1"));

        let wifi = wifi_connected_notification("Campus_Net");
        assert_eq!(wifi.title, "OpenUp: Wi-Fi Connected");
        assert!(wifi.body.contains("Campus_Net"));

        let cb = circuit_breaker_notification(3);
        assert_eq!(cb.title, "OpenUp: Autopilot Paused");
        assert!(cb.body.contains("3 times"));
    }
}
