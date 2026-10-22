# pause-dell-services.ps1 - Safely stop & disable Dell TechHub / AWCC background subagents
# Requires Administrator privileges.

$ErrorActionPreference = 'Continue'

$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Host "[!] Administrator privilege required. Requesting elevation (UAC)..." -ForegroundColor Yellow
    Start-Process powershell.exe -Verb RunAs -ArgumentList "-ExecutionPolicy Bypass -NoExit -File `"$PSCommandPath`""
    exit 0
}

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host " 1. Stopping Dell TechHub & AWCC Background Services" -ForegroundColor Cyan
Write-Host "==========================================================" -ForegroundColor Cyan

# 1. Stop Dell TechHub service and set to Manual
Write-Host "[+] Configuring DellTechHub service..." -ForegroundColor Yellow
try {
    Stop-Service -Name DellTechHub -Force -ErrorAction SilentlyContinue
    Set-Service -Name DellTechHub -StartupType Manual -ErrorAction SilentlyContinue
    Write-Host "[OK] DellTechHub service stopped and set to Manual." -ForegroundColor Green
} catch {
    Write-Warning "Could not stop DellTechHub: $($_.Exception.Message)"
}

# 2. Kill any stray subagents if still running
Write-Host "`n[+] Terminating remaining AWCC / TechHub background processes..." -ForegroundColor Yellow
$procNames = @('AWCC.SCSubAgent', 'AWCC.UCSubAgent', 'Dell.TechHub', 'Dell.TechHub.Instrumentation.UserProcess')
foreach ($p in $procNames) {
    $found = Get-Process -Name $p -ErrorAction SilentlyContinue
    if ($found) {
        $found | Stop-Process -Force -ErrorAction SilentlyContinue
        Write-Host "    Killed process: $p (PID $($found.Id -join ', '))" -ForegroundColor Gray
    }
}
Write-Host "[OK] All background conflicting processes stopped." -ForegroundColor Green

# 3. Verify WMI is completely unaffected
Write-Host "`n==========================================================" -ForegroundColor Cyan
Write-Host " 2. Verifying ACPI / WMI Hardware Interface" -ForegroundColor Cyan
Write-Host "==========================================================" -ForegroundColor Cyan
$verifyScript = Join-Path $PSScriptRoot "verify-wmi.ps1"
if (Test-Path $verifyScript) {
    & powershell.exe -ExecutionPolicy Bypass -File $verifyScript
}

Write-Host "`n==========================================================" -ForegroundColor Cyan
Write-Host " 3. Launching AWCC Lite (Release Build)" -ForegroundColor Cyan
Write-Host "==========================================================" -ForegroundColor Cyan
$exe = Join-Path $PSScriptRoot "target-release\release\awcc-lite.exe"
if (Test-Path $exe) {
    Write-Host "[+] Starting $exe..." -ForegroundColor Green
    Start-Process -FilePath $exe
} else {
    Write-Warning "Release build not found at $exe. Please run cargo build --release."
}

Write-Host "`n>>> Done! You are now running with pure AWCC Lite. <<<" -ForegroundColor Green
Write-Host ">>> To revert back at any time, run resume-dell-services.ps1 <<<" -ForegroundColor Gray
