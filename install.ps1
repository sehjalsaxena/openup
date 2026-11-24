[CmdletBinding()]
param(
    [string]$Version = $env:OPENUP_VERSION,
    [string]$Repo = $(if ($env:OPENUP_REPO) { $env:OPENUP_REPO } else { "sehjalsaxena/openup" }),
    [string]$InstallDir = $(if ($env:INSTALL_DIR) { $env:INSTALL_DIR } else { "$env:USERPROFILE\.openup\bin" })
)

$ErrorActionPreference = 'Stop'

$BinaryName = "openup.exe"
$AssetName = "openup-windows-amd64.zip"

if ($env:OPENUP_DOWNLOAD_BASE) {
    $DownloadUrl = $env:OPENUP_DOWNLOAD_BASE.TrimEnd('/') + "/$AssetName"
} elseif ($Version) {
    $DownloadUrl = "https://github.com/$Repo/releases/download/$Version/$AssetName"
} else {
    $DownloadUrl = "https://github.com/$Repo/releases/latest/download/$AssetName"
}

Write-Host "==> Installing openup (windows/amd64)..." -ForegroundColor Cyan
Write-Host "==> Downloading from: $DownloadUrl" -ForegroundColor Cyan

$TempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("openup-install-" + [System.Guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $TempDir -Force | Out-Null
$ArchiveFile = Join-Path $TempDir $AssetName

try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12 -bor [Net.SecurityProtocolType]::Tls13

    Invoke-WebRequest -Uri $DownloadUrl -OutFile $ArchiveFile -UseBasicParsing
    
    if (-not (Test-Path $ArchiveFile)) {
        Write-Error "[ERROR] Download failed: archive file was not found."
        exit 1
    }

    if ($env:OPENUP_SKIP_VERIFY -eq '1') {
        Write-Host "[WARN] Skipping checksum verification (OPENUP_SKIP_VERIFY=1)." -ForegroundColor Yellow
    } else {
        $ChecksumFile = "$ArchiveFile.sha256"
        try {
            Invoke-WebRequest -Uri "$DownloadUrl.sha256" -OutFile $ChecksumFile -UseBasicParsing
        } catch {
            throw "[ERROR] Could not download the checksum file ($DownloadUrl.sha256). Refusing to install an unverified binary. Set OPENUP_SKIP_VERIFY=1 to override."
        }
        $Expected = ((Get-Content -Path $ChecksumFile -Raw).Trim() -split '\s+')[0].ToLower()
        $Actual = (Get-FileHash -Path $ArchiveFile -Algorithm SHA256).Hash.ToLower()
        if ($Expected -ne $Actual) {
            throw "[ERROR] Checksum mismatch for $AssetName. Not installing. Expected $Expected but got $Actual."
        }
        Write-Host "[OK] Checksum verified." -ForegroundColor Green
    }

    Expand-Archive -Path $ArchiveFile -DestinationPath $TempDir -Force

    $ExtractedExe = Get-ChildItem -Path $TempDir -Filter $BinaryName -Recurse -File | Select-Object -First 1

    if (-not $ExtractedExe) {
        Write-Error "[ERROR] Could not find $BinaryName in downloaded archive."
        exit 1
    }

    if (-not (Test-Path $InstallDir)) {
        New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    }

    $TargetExe = Join-Path $InstallDir $BinaryName
    try {
        Copy-Item -Path $ExtractedExe.FullName -Destination $TargetExe -Force -ErrorAction Stop
    } catch {
        # If openup is currently running, Windows allows renaming but blocks overwriting
        $OldExe = "$TargetExe.old"
        if (Test-Path $OldExe) {
            Remove-Item $OldExe -Force -ErrorAction SilentlyContinue
        }
        Move-Item -Path $TargetExe -Destination $OldExe -Force -ErrorAction SilentlyContinue
        Copy-Item -Path $ExtractedExe.FullName -Destination $TargetExe -Force
    }

    Write-Host "[OK] openup installed to $TargetExe" -ForegroundColor Green

    if (($env:PATH -split ';') -notcontains $InstallDir) {
        $env:PATH = "$InstallDir;$env:PATH"
    }

    $UserPath = [Environment]::GetEnvironmentVariable("Path", [EnvironmentVariableTarget]::User)
    $PathParts = ($UserPath -split ';') | Where-Object { $_ -ne '' }
    $NormalizedInstallDir = $InstallDir.TrimEnd('\')
    $AlreadyInPath = $PathParts | Where-Object { $_.TrimEnd('\') -ieq $NormalizedInstallDir }

    if (-not $AlreadyInPath) {
        Write-Host ""
        Write-Host "Adding $InstallDir to your persistent User PATH..." -ForegroundColor Cyan

        $NewPath = if ([string]::IsNullOrWhiteSpace($UserPath)) { $InstallDir } else { "$UserPath;$InstallDir" }
        [Environment]::SetEnvironmentVariable("Path", $NewPath, [EnvironmentVariableTarget]::User)

        Write-Host "[OK] Added $InstallDir to User PATH." -ForegroundColor Green
    }

    Write-Host ""
    Write-Host "Next steps:" -ForegroundColor Cyan
    Write-Host "  1. openup setup     (guided: login, portal, Wi-Fi, background keeper)" -ForegroundColor Cyan
    Write-Host "  2. openup           (open the dashboard)" -ForegroundColor Cyan
    Write-Host "  3. openup doctor    (if anything seems off)" -ForegroundColor Cyan
}
finally {
    if (Test-Path $TempDir) {
        Remove-Item -Path $TempDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}
