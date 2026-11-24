//! Battery and power source detection for laptop battery-saving optimization.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerState {
    Ac,
    Battery,
    Unknown,
}

#[cfg(target_os = "windows")]
#[repr(C)]
struct SystemPowerStatus {
    ac_line_status: u8,
    battery_flag: u8,
    battery_life_percent: u8,
    system_status_flag: u8,
    battery_life_time: u32,
    battery_full_life_time: u32,
}

#[cfg(target_os = "windows")]
#[link(name = "kernel32")]
extern "system" {
    fn GetSystemPowerStatus(lpSystemPowerStatus: *mut SystemPowerStatus) -> i32;
}

pub fn current_power_state() -> PowerState {
    #[cfg(target_os = "windows")]
    {
        let mut status = SystemPowerStatus {
            ac_line_status: 255,
            battery_flag: 255,
            battery_life_percent: 255,
            system_status_flag: 0,
            battery_life_time: 0,
            battery_full_life_time: 0,
        };
        let res = unsafe { GetSystemPowerStatus(&mut status) };
        if res != 0 {
            match status.ac_line_status {
                0 => PowerState::Battery,
                1 => PowerState::Ac,
                _ => PowerState::Unknown,
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
                if let Ok(supply_type) = std::fs::read_to_string(path.join("type")) {
                    if supply_type.trim().eq_ignore_ascii_case("Battery") {
                        if let Ok(status) = std::fs::read_to_string(path.join("status")) {
                            if status.trim().eq_ignore_ascii_case("Discharging") {
                                return PowerState::Battery;
                            } else {
                                return PowerState::Ac;
                            }
                        }
                    }
                }
            }
        }
        PowerState::Unknown
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("pmset")
            .arg("-g")
            .arg("batt")
            .output()
        {
            let text = String::from_utf8_lossy(&output.stdout);
            if text.contains("Battery Power") {
                return PowerState::Battery;
            } else if text.contains("AC Power") {
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
