# Install Cove from an unpacked release zip, for this Windows user (no admin).
#
#   powershell -ExecutionPolicy Bypass -File .\install.ps1
#   powershell -ExecutionPolicy Bypass -File .\install.ps1 -Uninstall
#
# Layout: %LOCALAPPDATA%\Programs\cove\bin\cove.exe, plus a Start
# Menu shortcut. A folder the user owns, so the app's own updater
# (src/update.rs) can swap the .exe in place; R, Python and the shell run in
# the person's ndexr cell once they sign in (the app's toolbar).
param([switch]$Uninstall)
$ErrorActionPreference = "Stop"

$root = Join-Path $env:LOCALAPPDATA "Programs\cove"
$exe = Join-Path $root "bin\cove.exe"
$lnk = Join-Path ([Environment]::GetFolderPath("Programs")) "Cove.lnk"

if ($Uninstall) {
    Remove-Item -Force -ErrorAction SilentlyContinue $lnk
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $root
    Write-Host "removed Cove ($root)"
    exit 0
}

$here = Split-Path -Parent $MyInvocation.MyCommand.Path
New-Item -ItemType Directory -Force -Path (Join-Path $root "bin") | Out-Null
Copy-Item -Force (Join-Path $here "bin\cove.exe") $exe

$shell = New-Object -ComObject WScript.Shell
$s = $shell.CreateShortcut($lnk)
$s.TargetPath = $exe
$s.WorkingDirectory = $env:USERPROFILE
$s.Description = "Cove - R with Python through reticulate, in your ndexr cell"
$s.Save()

Write-Host "installed Cove -> $exe"
Write-Host "Start menu: 'Cove'. Sign in from the toolbar to run in your cell."
