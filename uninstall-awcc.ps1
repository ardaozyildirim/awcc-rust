# uninstall-awcc.ps1
# Launches official Alienware Command Center uninstaller with elevation.
$ErrorActionPreference = 'Stop'

$installer = "C:\Program Files (x86)\InstallShield Installation Information\{D2DA930B-CB5D-4DD6-BF62-BE6C310A353D}\AWCCInstallationManager.exe"

if (-not (Test-Path $installer)) {
    Write-Host "[!] AWCC Installation Manager not found at expected path." -ForegroundColor Red
    Write-Host "[*] Opening Windows Installed Apps Settings instead..." -ForegroundColor Yellow
    Start-Process "ms-settings:appsfeatures"
    exit 1
}

Write-Host "=== Launching Official Alienware Command Center Uninstaller ===" -ForegroundColor Cyan
Write-Host "This will open the Dell AWCC Removal Wizard." -ForegroundColor Yellow

Start-Process -FilePath $installer -ArgumentList "-remove -runfromtemp" -Verb RunAs
Write-Host "[OK] Uninstaller launched. Please follow the on-screen prompts to complete uninstallation." -ForegroundColor Green
