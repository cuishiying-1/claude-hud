# Claude HUD installer for Windows (no admin required)
$ErrorActionPreference = 'Stop'

$Repo = $env:HUD_REPO
if (-not $Repo) { $Repo = 'user/claude-hud' }   # 发布前替换为真实仓库

$InstallDir = Join-Path $env:LOCALAPPDATA 'claude-hud\bin'
New-Item -ItemType Directory -Force $InstallDir | Out-Null
Write-Host "Installing Claude HUD to $InstallDir ..."

# PATH（HKCU 用户级，免管理员）
$UserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($UserPath -notmatch [regex]::Escape($InstallDir)) {
    $NewPath = if ($UserPath) { "$UserPath;$InstallDir" } else { $InstallDir }
    [Environment]::SetEnvironmentVariable('Path', $NewPath, 'User')
    Write-Host "Added $InstallDir to user PATH (new terminal windows pick it up)."
}

$LocalStub = $env:HUD_LOCAL_STUB
if ($LocalStub) {
    # 本地安装模式（开发/CI 冒烟）：不访问网络
    Copy-Item $LocalStub (Join-Path $InstallDir 'claude-hud.cmd') -Force
} else {
    # 兼容旧版 .NET：PS 5.1 需显式启用 TLS 1.2 才能访问 GitHub API
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    $Release = Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest"
    $Tag = $Release.tag_name
    $VersionFile = Join-Path $InstallDir 'version.txt'
    if ((Test-Path $VersionFile) -and ((Get-Content $VersionFile -Raw).Trim() -eq $Tag)) {
        Write-Host "claude-hud $($Tag.Replace('v','')) already installed - nothing to do."
        return
    }
    $Zip = Join-Path $env:TEMP 'claude-hud-windows.zip'
    try {
        Invoke-WebRequest "https://github.com/$Repo/releases/download/$Tag/claude-hud-windows-x64.zip" -OutFile $Zip
        Expand-Archive $Zip -DestinationPath $InstallDir -Force
    } catch {
        throw "Failed to download/extract claude-hud - close any running claude-hud process and retry. $_"
    }
    Set-Content -Path $VersionFile -Value $Tag -Encoding ascii
}

$Bin = Join-Path $InstallDir 'claude-hud.exe'
if (-not (Test-Path $Bin)) { $Bin = Join-Path $InstallDir 'claude-hud.cmd' }
& $Bin setup
if ($LASTEXITCODE -ne 0) {
    throw "claude-hud setup failed (exit code $LASTEXITCODE) - check the output above"
}

Write-Host ''
Write-Host 'Done! Verify:'
Write-Host '  echo ''{"model":{"id":"test","display_name":"Test"},"context_window":{"used_percentage":50,"total_input_tokens":1000,"context_window_size":200000},"cost":{"total_cost_usd":0.1,"total_duration_ms":60000}}'' | claude-hud render'
Write-Host '  Restart Claude Code or run /reload-plugins to see the HUD status bar.'
