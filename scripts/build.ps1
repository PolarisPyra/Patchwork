# Builds Patchwork's Windows installer and installs it for the current user under
# %LOCALAPPDATA%\Patchwork, with a Start menu shortcut. The app updates itself from the releases at
# updates.patchworkeditor.com with the same installer. Release builds come from scripts/release.sh.
#
# Usage: powershell -ExecutionPolicy Bypass -File scripts\build.ps1 install
param([Parameter(Position = 0)][string]$Action)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $repoRoot

function Die([string]$Message) {
  [Console]::Error.WriteLine($Message)
  exit 1
}

function Show-Usage {
  @'
Usage: scripts\build.ps1 install

  install   Build the installer and install Patchwork for this user: %LOCALAPPDATA%\Patchwork
            and a Start menu shortcut
'@
}

# Runs a native command and stops the script when it fails, which PowerShell does not do itself.
# Its output goes to the console, not into the calling function's return value.
function Invoke-Checked([string]$Program, [string[]]$Arguments) {
  & $Program @Arguments | Out-Host
  if ($LASTEXITCODE -ne 0) { Die "$Program $($Arguments -join ' ') failed with exit code $LASTEXITCODE." }
}

function Assert-Tools {
  foreach ($tool in 'cargo', 'node') {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) { Die "$tool is required." }
  }
  if (-not (Get-Command pnpm -ErrorAction SilentlyContinue)) {
    if (-not (Get-Command npm -ErrorAction SilentlyContinue)) { Die 'pnpm is required.' }
    Write-Host 'pnpm: not found, installing it with npm'
    Invoke-Checked npm @('install', '--global', 'pnpm@10')
    # npm's global folder may have been added to PATH just now, which this session doesn't see.
    $env:Path = "$env:APPDATA\npm;$env:Path"
    if (-not (Get-Command pnpm -ErrorAction SilentlyContinue)) { Die 'pnpm is required.' }
  }
}

function Build-Installer([string]$Version) {
  Assert-Tools
  Invoke-Checked pnpm @('install', '--frozen-lockfile')
  Invoke-Checked pnpm @('--filter', 'patchwork-desktop', 'exec', 'tauri', 'build', '--bundles', 'nsis')
  $built = Join-Path $repoRoot "target\release\bundle\nsis\Patchwork_${Version}_x64-setup.exe"
  if (-not (Test-Path $built)) { Die "Expected $built after the build." }
  $built
}

function Install-App {
  $version = (Get-Content -Raw apps\desktop\src-tauri\tauri.conf.json | ConvertFrom-Json).version
  $installer = Build-Installer $version
  # The installer replaces the app's files, which Windows keeps locked while it runs.
  if (Get-Process -Name Patchwork -ErrorAction SilentlyContinue) {
    Die "Patchwork is running. Quit it, then run the install again; the installer is at $installer."
  }
  $process = Start-Process -FilePath $installer -ArgumentList '/S' -Wait -PassThru
  if ($process.ExitCode -ne 0) { Die "The installer failed with exit code $($process.ExitCode)." }
  $exe = Join-Path $env:LOCALAPPDATA 'Patchwork\Patchwork.exe'
  if (-not (Test-Path $exe)) { Die "The installer finished, but $exe is missing." }
  Write-Host "App: $exe"
  Write-Host "Installed Patchwork $version. Open it from the Start menu."
}

switch ($Action) {
  'install' { Install-App }
  { $_ -in '-h', '--help' } { Show-Usage }
  default {
    [Console]::Error.WriteLine((Show-Usage))
    exit 1
  }
}
