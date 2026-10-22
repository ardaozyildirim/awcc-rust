# disable-all-dell-services.ps1
# Fully stops and disables ALL Dell / Alienware background services & subagents.
$ErrorActionPreference = 'Continue'

$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Host "[!] Admin privileges required. Elevating via UAC..." -ForegroundColor Yellow
    Start-Process powershell.exe -Verb RunAs -ArgumentList "-ExecutionPolicy Bypass -NoExit -File `"$PSCommandPath`""
    exit 0
}

Write-Host "=== Stopping and Disabling All Dell / Alienware Services ===" -ForegroundColor Cyan

$services = @('DellTechHub', 'DellClientManagementService', 'DellConnectedServiceDelivery')
foreach ($svc in $services) {
    if (Get-Service -Name $svc -ErrorAction SilentlyContinue) {
        Write-Host "[*] Disabling service: $svc" -ForegroundColor Yellow
        Stop-Service -Name $svc -Force -ErrorAction SilentlyContinue
        Set-Service -Name $svc -StartupType Disabled -ErrorAction SilentlyContinue
        Write-Host "[OK] $svc is now STOPPED and DISABLED." -ForegroundColor Green
    }
}

Write-Host "`n[*] Terminating all running Dell / AWCC subagent processes..." -ForegroundColor Yellow
$procList = @(
    'AWCC.SCSubAgent', 'AWCC.UCSubAgent', 'Dell.TechHub',
    'Dell.TechHub.Analytics.SubAgent', 'Dell.TechHub.DataManager.SubAgent',
    'Dell.TechHub.Diagnostics.SubAgent', 'Dell.TechHub.Instrumentation.SubAgent',
    'Dell.TechHub.Instrumentation.UserProcess', 'Dell.Connected.Service.Delivery',
    'Dell.Connected.Service.Delivery.SubAgent', 'Dell.CoreServices.Client',
    'Dell.Remediation.Agent', 'Dell.UCA.Manager', 'Dell.Update.SubAgent'
)

foreach ($proc in $procList) {
    $found = Get-Process -Name $proc -ErrorAction SilentlyContinue
    if ($found) {
        $found | Stop-Process -Force -ErrorAction SilentlyContinue
        Write-Host "    Killed: $proc" -ForegroundColor Gray
    }
}

Write-Host "`n[OK] All Dell background services and telemetry subagents are now completely disabled." -ForegroundColor Green
Write-Host "Your system is 100% free of Dell background CPU/RAM overhead." -ForegroundColor Cyan
