//! Battery and power source detection for laptop battery-saving optimization.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerState {
    Ac,
    Battery,
    Unknown,
}

pub fn current_power_state() -> PowerState {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        let output = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "(Get-CimInstance -ClassName Win32_Battery -ErrorAction SilentlyContinue).BatteryStatus",
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output();

        if let Ok(out) = output {
            let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if matches!(text.as_str(), "1" | "4" | "5") {
                PowerState::Battery
            } else {
                PowerState::Ac
            }
        } else {
            PowerState::Unknown
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(entries) = std::fs::read_dir("/sys/class/power_supply") {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Ok(supply_type) = std::fs::read_to_string(path.join("type"))
                    && supply_type.trim().eq_ignore_ascii_case("Battery")
                    && let Ok(status) = std::fs::read_to_string(path.join("status"))
                {
                    if status.trim().eq_ignore_ascii_case("Discharging") {
                        return PowerState::Battery;
                    }
                    return PowerState::Ac;
                }
            }
        }
        PowerState::Unknown
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("pmset")
            .args(["-g", "batt"])
            .output()
        {
            let text = String::from_utf8_lossy(&output.stdout);
            if text.contains("Battery Power") {
                return PowerState::Battery;
            }
            if text.contains("AC Power") {
                return PowerState::Ac;
            }
        }
        PowerState::Unknown
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        PowerState::Unknown
    }
}

pub fn is_on_battery() -> bool {
    current_power_state() == PowerState::Battery
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_state_does_not_panic() {
        let state = current_power_state();
        let _ = is_on_battery();
        assert!(matches!(
            state,
            PowerState::Ac | PowerState::Battery | PowerState::Unknown
        ));
    }
}
