# Claude HUD uninstaller for Windows
$ErrorActionPreference = 'Stop'

$InstallDir = Join-Path $env:LOCALAPPDATA 'claude-hud\bin'
$RootDir = Split-Path $InstallDir -Parent
$Bin = Join-Path $InstallDir 'claude-hud.exe'
if (-not (Test-Path $Bin)) { $Bin = Join-Path $InstallDir 'claude-hud.cmd' }

# 1. 先摘掉 statusLine 并删除配置目录，避免 Claude Code 继续调用已删除命令
if (Test-Path $Bin) {
    & $Bin uninstall
    if ($LASTEXITCODE -ne 0) {
        Write-Host "warning: claude-hud uninstall failed (exit code $LASTEXITCODE) - the statusLine may still be active" -ForegroundColor Yellow
    }
}

# 2. 移除 PATH 条目（逐段精确匹配）
$UserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($UserPath -match [regex]::Escape($InstallDir)) {
    $NewPath = ($UserPath -split ';' | Where-Object { $_ -and $_.TrimEnd('\') -ne $InstallDir }) -join ';'
    [Environment]::SetEnvironmentVariable('Path', $NewPath, 'User')
    Write-Host "Removed $InstallDir from user PATH."
}

# 3. 删除安装目录（二进制可能被占用，失败则提示手动删除）
if (-not (Test-Path $RootDir)) {
    Write-Host 'Claude HUD was not installed - nothing to remove.'
    return
}
$removed = $false
for ($i = 0; $i -lt 3; $i++) {
    Remove-Item $RootDir -Recurse -Force -ErrorAction SilentlyContinue
    if (-not (Test-Path $RootDir)) { $removed = $true; break }
    Start-Sleep -Milliseconds 300
}
if ($removed) {
    Write-Host "Removed $RootDir"
} else {
    Write-Host "warning: $RootDir could not be fully removed - delete it manually" -ForegroundColor Yellow
    exit 1
}

Write-Host 'Claude HUD uninstalled.'
