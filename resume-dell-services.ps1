# resume-dell-services.ps1 - Restore Dell TechHub service to Automatic and start it
# Requires Administrator privileges.

$ErrorActionPreference = 'Continue'

$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Host "[!] Administrator privilege required. Requesting elevation (UAC)..." -ForegroundColor Yellow
    Start-Process powershell.exe -Verb RunAs -ArgumentList "-ExecutionPolicy Bypass -NoExit -File `"$PSCommandPath`""
    exit 0
}

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host " Restoring Dell TechHub & Official Services" -ForegroundColor Cyan
Write-Host "==========================================================" -ForegroundColor Cyan

try {
    Write-Host "[+] Setting DellTechHub startup type to Automatic..." -ForegroundColor Yellow
    Set-Service -Name DellTechHub -StartupType Automatic -ErrorAction Stop
    Write-Host "[+] Starting DellTechHub service..." -ForegroundColor Yellow
    Start-Service -Name DellTechHub -ErrorAction Stop
    Write-Host "[OK] DellTechHub is now running and set to Automatic." -ForegroundColor Green
} catch {
    Write-Host "[-] Error: $($_.Exception.Message)" -ForegroundColor Red
}

Write-Host "`n>>> Done! Dell TechHub and official subagents have been resumed. <<<" -ForegroundColor Green
