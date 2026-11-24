//! System tray icon integration for OpenUp.

use std::path::Path;

/// Launches the system tray runner.
pub fn run_tray(config_dir: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        run_windows_tray(config_dir)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = config_dir;
        println!("OpenUp system tray is currently integrated natively for Windows.");
        println!(
            "On macOS/Linux, run 'openup' in your terminal or use 'openup keeper start' in the background."
        );
        Ok(())
    }
}

#[cfg(target_os = "windows")]
fn run_windows_tray(config_dir: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let state_file = config_dir.join("state.json");
    let state_path_str = state_file.to_string_lossy().to_string();

    let ps_script = r#"
        Add-Type -AssemblyName System.Windows.Forms
        Add-Type -AssemblyName System.Drawing

        $statePath = $env:OPENUP_STATE_PATH

        $notify = New-Object System.Windows.Forms.NotifyIcon
        $notify.Icon = [System.Drawing.SystemIcons]::Information
        $notify.Text = "OpenUp: Starting..."
        $notify.Visible = $true

        $contextMenu = New-Object System.Windows.Forms.ContextMenuStrip
        $statusItem = $contextMenu.Items.Add("OpenUp: Checking...")
        $statusItem.Enabled = $false
        $null = $contextMenu.Items.Add("-")

        $openItem = $contextMenu.Items.Add("Open Dashboard", $null, {
            Start-Process "wt.exe" -ArgumentList "openup" -ErrorAction SilentlyContinue
            if (-not $?) {
                Start-Process "powershell" -ArgumentList "-NoExit -Command openup"
            }
        })

        $nowItem = $contextMenu.Items.Add("Sign In Now", $null, {
            Start-Process "openup" -ArgumentList "now" -WindowStyle Hidden
        })

        $null = $contextMenu.Items.Add("-")
        $exitItem = $contextMenu.Items.Add("Exit Tray", $null, {
            $notify.Visible = $false
            $notify.Dispose()
            [System.Windows.Forms.Application]::Exit()
        })

        $notify.ContextMenuStrip = $contextMenu

        $notify.add_DoubleClick({
            Start-Process "wt.exe" -ArgumentList "openup" -ErrorAction SilentlyContinue
            if (-not $?) {
                Start-Process "powershell" -ArgumentList "-NoExit -Command openup"
            }
        })

        $timer = New-Object System.Windows.Forms.Timer
        $timer.Interval = 3000
        $timer.add_Tick({
            try {
                if (Test-Path $statePath) {
                    $json = Get-Content -Raw $statePath | ConvertFrom-Json
                    $msg = $json.last_status_message
                    if ($msg -eq "Online and unlocked") {
                        $notify.Icon = [System.Drawing.SystemIcons]::Information
                        $notify.Text = "OpenUp: Online"
                        $statusItem.Text = "Status: Online"
                    } elseif ($json.halted_bad_credentials) {
                        $notify.Icon = [System.Drawing.SystemIcons]::Error
                        $notify.Text = "OpenUp: Halted (Bad Password)"
                        $statusItem.Text = "Status: Bad Password"
                    } else {
                        $notify.Icon = [System.Drawing.SystemIcons]::Warning
                        $notify.Text = "OpenUp: $msg"
                        $statusItem.Text = "Status: $msg"
                    }
                } else {
                    $notify.Text = "OpenUp: Active"
                    $statusItem.Text = "Status: Active"
                }
            } catch {}
        })
        $timer.Start()

        [System.Windows.Forms.Application]::Run()
    "#;

    println!("Starting OpenUp system tray in the taskbar notification area...");
    println!("Right-click the OpenUp tray icon to view status, sign in, or open the dashboard.");

    let status = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", ps_script])
        .env("OPENUP_STATE_PATH", state_path_str)
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| format!("Failed to start system tray runner: {e}"))?;

    if !status.success() {
        return Err(format!("System tray exited with code: {:?}", status.code()));
    }
    Ok(())
}
