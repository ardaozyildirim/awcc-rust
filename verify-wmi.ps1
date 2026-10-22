# verify-wmi.ps1 - Validates Alienware Thermal WMI provider (firmware / ACPI driver)
# Run as Administrator to test WMI read and method invocations.

$ErrorActionPreference = 'Stop'

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host " Alienware WMI Provider Verification Script" -ForegroundColor Cyan
Write-Host "==========================================================" -ForegroundColor Cyan

$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Warning "Not running as Administrator. Method calls may fail or return Access Denied."
}

Write-Host "`n[1/5] Checking WMI Class 'AWCCWmiMethodFunction' in 'root\wmi'..." -ForegroundColor Yellow
try {
    $instances = Get-CimInstance -Namespace root\wmi -ClassName AWCCWmiMethodFunction -ErrorAction Stop
    $instance = $instances | Select-Object -First 1
    if (-not $instance) {
        Write-Host "[-] FAILED: AWCCWmiMethodFunction class exists but returned 0 instances!" -ForegroundColor Red
        exit 1
    }
    Write-Host "[+] SUCCESS: Found WMI instance." -ForegroundColor Green
    Write-Host "    InstanceName : $($instance.InstanceName)" -ForegroundColor Gray
    Write-Host "    Active       : $($instance.Active)" -ForegroundColor Gray
} catch {
    Write-Host "[-] FAILED: Cannot open root\wmi:AWCCWmiMethodFunction: $($_.Exception.Message)" -ForegroundColor Red
    Write-Host "    AWCC WMI driver provider is NOT accessible." -ForegroundColor Red
    exit 1
}

function Invoke-ThermalInfo([uint32]$arg) {
    try {
        $res = Invoke-CimMethod -InputObject $instance -MethodName 'Thermal_Information' -Arguments @{ arg2 = $arg } -ErrorAction Stop
        return [uint32]$res.argr
    } catch {
        Write-Warning "Thermal_Information (0x$('{0:X4}' -f $arg)) failed: $($_.Exception.Message)"
        return $null
    }
}

Write-Host "`n[2/5] Probing Thermal Description (0x02)..." -ForegroundColor Yellow
$desc = Invoke-ThermalInfo 0x02
if ($null -eq $desc) {
    Write-Host "[-] FAILED: Could not invoke Thermal_Information 0x02." -ForegroundColor Red
    exit 1
}
$b = [BitConverter]::GetBytes([uint32]$desc)
$fanCount  = [int]$b[0]
$tempCount = [int]$b[1]
$miscCount = [int]$b[2]
$profCount = [int]$b[3]
Write-Host "[+] SUCCESS: Description raw=0x$('{0:X8}' -f $desc)" -ForegroundColor Green
Write-Host "    Fans: $fanCount | Temp Sensors: $tempCount | Misc: $miscCount | Profiles: $profCount" -ForegroundColor Gray

Write-Host "`n[3/5] Querying Current Active Profile (0x0B)..." -ForegroundColor Yellow
$cur = Invoke-ThermalInfo 0x0B
if ($null -ne $cur) {
    $curId = $cur -band 0xFF
    $name = switch ($curId) {
        0xA0 { "Balanced (0xA0)" }
        0xA1 { "Performance (0xA1)" }
        0xA2 { "Battery (0xA2)" }
        0xA3 { "Quiet (0xA3)" }
        0xA4 { "Overdrive (0xA4)" }
        0x00 { "Custom (0x00)" }
        Default { "Unknown (0x$('{0:X2}' -f $curId))" }
    }
    Write-Host "[+] Current Thermal Profile: $name" -ForegroundColor Green
} else {
    Write-Host "[-] Warning: Could not read current profile (0x0B)." -ForegroundColor Yellow
}

Write-Host "`n[4/5] Reading Fan Sensors..." -ForegroundColor Yellow
for ($i = 0; $i -lt $fanCount; $i++) {
    $idRes = Invoke-ThermalInfo ([uint32](0x03 -bor ($i -shl 8)))
    if ($null -ne $idRes) {
        $fanId = $idRes -band 0xFF
        $rpmRes = Invoke-ThermalInfo ([uint32](0x05 -bor ($fanId -shl 8)))
        $rpm = if ($null -ne $rpmRes) { $rpmRes -band 0xFFFF } else { 'N/A' }
        Write-Host "    Fan #$i (ID 0x$('{0:X2}' -f $fanId)): $rpm RPM" -ForegroundColor Cyan
    }
}

Write-Host "`n[5/5] Testing TCC (CPU Thermal Control)..." -ForegroundColor Yellow
try {
    $tccMaxTemp = Invoke-CimMethod -InputObject $instance -MethodName 'TccControl' -Arguments @{ arg2 = [uint32]1 } -ErrorAction Stop
    $tccMaxOff  = Invoke-CimMethod -InputObject $instance -MethodName 'TccControl' -Arguments @{ arg2 = [uint32]2 } -ErrorAction Stop
    $tccCurOff  = Invoke-CimMethod -InputObject $instance -MethodName 'TccControl' -Arguments @{ arg2 = [uint32]3 } -ErrorAction Stop
    Write-Host "[+] TCC Supported:" -ForegroundColor Green
    Write-Host "    Max CPU Temp   : $($tccMaxTemp.argr) C" -ForegroundColor Gray
    Write-Host "    Max Offset     : $($tccMaxOff.argr) C" -ForegroundColor Gray
    Write-Host "    Current Offset : $($tccCurOff.argr) C" -ForegroundColor Gray
} catch {
    Write-Host "[-] TCC Control not available: $($_.Exception.Message)" -ForegroundColor Yellow
}

Write-Host "`n==========================================================" -ForegroundColor Cyan
Write-Host " VERIFICATION RESULT: WMI Thermal Provider is ALIVE & OK! " -ForegroundColor Green
Write-Host "==========================================================" -ForegroundColor Cyan
