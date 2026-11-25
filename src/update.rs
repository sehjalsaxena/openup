//! Self-updater for OpenUp releases from GitHub.

use serde::Deserialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const RELEASES_API_URL: &str = "https://api.github.com/repos/sehjalsaxena/openup/releases/latest";

#[derive(Debug, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Debug, Deserialize)]
pub struct ReleaseInfo {
    pub tag_name: String,
    pub html_url: String,
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum VersionStatus {
    UpToDate(String),
    NewerAvailable { current: String, latest: String },
}

pub fn parse_version(tag: &str) -> (u64, u64, u64) {
    let clean = tag.trim().trim_start_matches('v');
    let mut parts = clean.split('.');
    let major = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let patch = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (major, minor, patch)
}

pub fn compare_versions(current: &str, latest: &str) -> VersionStatus {
    let c = parse_version(current);
    let l = parse_version(latest);
    if l > c {
        VersionStatus::NewerAvailable {
            current: current.to_string(),
            latest: latest.to_string(),
        }
    } else {
        VersionStatus::UpToDate(current.to_string())
    }
}

pub fn target_asset_name() -> Result<&'static str, String> {
    if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Ok("openup-windows-amd64.zip")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Ok("openup-linux-amd64.tar.gz")
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        Ok("openup-linux-arm64.tar.gz")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Ok("openup-darwin-amd64.tar.gz")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Ok("openup-darwin-arm64.tar.gz")
    } else {
        Err("Unsupported operating system or CPU architecture for self-updating.".into())
    }
}

pub fn fetch_latest_release() -> Result<ReleaseInfo, String> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(format!("openup/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    let resp = client
        .get(RELEASES_API_URL)
        .send()
        .map_err(|e| format!("Could not reach GitHub Releases API: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("GitHub API returned HTTP {}", resp.status()));
    }

    let text = resp
        .text()
        .map_err(|e| format!("Could not read release response: {e}"))?;

    serde_json::from_str::<ReleaseInfo>(&text)
        .map_err(|e| format!("Could not parse release metadata: {e}"))
}

pub fn run_update(check_only: bool, force: bool) -> Result<(), Box<dyn std::error::Error>> {
    println!("Checking for updates...");
    let release = fetch_latest_release()?;
    let current_ver = env!("CARGO_PKG_VERSION");
    let status = compare_versions(current_ver, &release.tag_name);

    match status {
        VersionStatus::UpToDate(_) if !force => {
            println!("openup is already up to date ({}).", release.tag_name);
            return Ok(());
        }
        VersionStatus::NewerAvailable { current, latest } => {
            println!("Update available: v{current} -> {latest}");
            if check_only {
                println!("Run `openup update` to install the update.");
                return Ok(());
            }
        }
        VersionStatus::UpToDate(_) => {
            println!(
                "Reinstalling current version {} (--force)",
                release.tag_name
            );
        }
    }

    let asset_name = target_asset_name()?;
    let asset = release
        .assets
        .iter()
        .find(|a| a.name == asset_name)
        .ok_or_else(|| format!("No release asset found matching current platform: {asset_name}"))?;

    println!("Downloading {} from GitHub...", asset.name);
    let client = reqwest::blocking::Client::builder()
        .user_agent(format!("openup/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(60))
        .build()?;

    let mut resp = client.get(&asset.browser_download_url).send()?;
    if !resp.status().is_success() {
        return Err(format!("Download failed with HTTP {}", resp.status()).into());
    }

    let temp_dir = env::temp_dir().join(format!("openup_update_{}", std::process::id()));
    if temp_dir.exists() {
        let _ = fs::remove_dir_all(&temp_dir);
    }
    fs::create_dir_all(&temp_dir)?;

    let archive_path = temp_dir.join(&asset.name);
    let mut file = fs::File::create(&archive_path)?;
    std::io::copy(&mut resp, &mut file)?;

    println!("Extracting archive...");
    extract_archive(&archive_path, &temp_dir)?;

    let bin_name = if cfg!(windows) {
        "openup.exe"
    } else {
        "openup"
    };
    let extracted_bin = find_file_recursive(&temp_dir, bin_name)
        .ok_or_else(|| format!("Could not find {bin_name} inside the downloaded archive."))?;

    let current_exe = env::current_exe()?;
    install_binary(&extracted_bin, &current_exe)?;

    let _ = fs::remove_dir_all(&temp_dir);

    println!(
        "\x1b[32m[OK] Successfully updated openup to {}!\x1b[0m",
        release.tag_name
    );
    println!("Updated binary: {}", current_exe.display());
    Ok(())
}

fn extract_archive(archive: &Path, dest: &Path) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        let script = format!(
            "Expand-Archive -Path '{}' -DestinationPath '{}' -Force",
            archive.display(),
            dest.display()
        );
        let status = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .creation_flags(CREATE_NO_WINDOW)
            .status()?;
        if !status.success() {
            return Err("PowerShell Expand-Archive failed.".into());
        }
    }

    #[cfg(not(windows))]
    {
        let status = std::process::Command::new("tar")
            .args([
                "-xzf",
                archive.to_str().unwrap_or_default(),
                "-C",
                dest.to_str().unwrap_or_default(),
            ])
            .status()?;
        if !status.success() {
            return Err("tar extraction failed.".into());
        }
    }

    Ok(())
}

fn find_file_recursive(dir: &Path, filename: &str) -> Option<PathBuf> {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.file_name().and_then(|n| n.to_str()) == Some(filename) {
                return Some(path);
            }
            if path.is_dir() {
                if let Some(found) = find_file_recursive(&path, filename) {
                    return Some(found);
                }
            }
        }
    }
    None
}

fn install_binary(extracted: &Path, target: &Path) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(windows)]
    {
        let old_exe = target.with_extension("exe.old");
        if old_exe.exists() {
            let _ = fs::remove_file(&old_exe);
        }
        // Windows allows renaming a running executable
        let _ = fs::rename(target, &old_exe);
        fs::copy(extracted, target)?;
    }

    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        let old_bin = target.with_extension("old");
        let _ = fs::remove_file(&old_bin);
        let _ = fs::rename(target, &old_bin);
        fs::copy(extracted, target)?;
        let _ = fs::set_permissions(target, fs::Permissions::from_mode(0o755));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_parsing_works() {
        assert_eq!(parse_version("0.1.0"), (0, 1, 0));
        assert_eq!(parse_version("v0.1.0"), (0, 1, 0));
        assert_eq!(parse_version("v1.2.3"), (1, 2, 3));
    }

    #[test]
    fn version_comparison_works() {
        assert_eq!(
            compare_versions("0.1.0", "v0.1.1"),
            VersionStatus::NewerAvailable {
                current: "0.1.0".into(),
                latest: "v0.1.1".into()
            }
        );
        assert_eq!(
            compare_versions("0.2.0", "v0.1.9"),
            VersionStatus::UpToDate("0.2.0".into())
        );
        assert_eq!(
            compare_versions("0.1.0", "v0.1.0"),
            VersionStatus::UpToDate("0.1.0".into())
        );
    }

    #[test]
    fn target_asset_name_returns_string() {
        let name = target_asset_name();
        assert!(name.is_ok());
        let s = name.unwrap();
        assert!(s.starts_with("openup-"));
    }
}
