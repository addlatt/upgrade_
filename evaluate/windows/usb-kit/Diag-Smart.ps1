<#
.SYNOPSIS
    upgrade_ - read-only SMART / drive diagnostic. Writes one text file to
    the stick. Changes nothing on the computer.

.DESCRIPTION
    For a drive Windows has logged bad blocks on (RISKS R18, the Aspire,
    2026-09-13). Reads, for every physical disk: model, bus, firmware,
    Windows' failure-prediction flag and the raw SMART attribute table
    (root\wmi MSStorageDriver_FailurePredictStatus / FailurePredictData -
    ATA pass-through, so SATA drives answer; NVMe usually does not), the
    storage reliability counters, and the 'disk' events (7 bad block, 51
    paging error, 153 reset, 157 surprise removal) of the last 60 days.
    The attributes that decide the diagnosis are named in the output:
      5   Reallocated_Sector_Ct   } flash/media wearing out -> replace
      197 Current_Pending_Sector  }
      198 Offline_Uncorrectable   }
      187 Reported_Uncorrectable  }
      199 UDMA_CRC_Error_Count    -> link/connector errors -> reseat/cable
      177/231/233 wear / media wearout / total writes (vendor-specific)
#>
param([string]$OutFile)
$ErrorActionPreference = 'Continue'
$o = New-Object System.Collections.Generic.List[string]
function L([string]$s) { $o.Add($s) }
function Short([string]$m, [int]$n) { $t = ($m -replace '\s+', ' '); if ($t.Length -gt $n) { $t.Substring(0, $n) } else { $t } }
L ("== upgrade_ SMART diagnostic " + (Get-Date).ToUniversalTime().ToString('o') + " " + $env:COMPUTERNAME)
$names = @{ 1='Raw_Read_Error_Rate'; 5='Reallocated_Sector_Ct'; 9='Power_On_Hours'; 12='Power_Cycle_Count'; 170='Available_Reserved_Space'; 171='Program_Fail_Count'; 172='Erase_Fail_Count'
            173='Wear_Leveling_Count'; 174='Unexpected_Power_Loss'; 177='Wear_Leveling_Count'; 179='Used_Rsvd_Blk_Cnt_Tot'; 181='Program_Fail_Cnt_Total'; 182='Erase_Fail_Count_Total'
            183='Runtime_Bad_Block'; 184='End-to-End_Error'; 187='Reported_Uncorrectable'; 188='Command_Timeout'; 190='Airflow_Temperature'; 194='Temperature'; 195='Hardware_ECC_Recovered'
            196='Reallocated_Event_Count'; 197='Current_Pending_Sector'; 198='Offline_Uncorrectable'; 199='UDMA_CRC_Error_Count'; 231='SSD_Life_Left'; 232='Available_Reservd_Space'
            233='Media_Wearout_Indicator'; 234='Thermal_Throttle'; 241='Total_LBAs_Written'; 242='Total_LBAs_Read'; 249='NAND_Writes' }

L "-- physical disks"
foreach ($d in Get-PhysicalDisk) {
    L ("  disk {0}: {1}  bus={2} media={3} health={4} op={5} size={6} GB firmware={7} serial={8}" -f $d.DeviceId, $d.FriendlyName, $d.BusType, $d.MediaType, $d.HealthStatus, ($d.OperationalStatus -join ','), [math]::Round($d.Size/1e9,1), $d.FirmwareVersion, $d.SerialNumber)
    try {
        $c = $d | Get-StorageReliabilityCounter -ErrorAction Stop
        L ("      reliability: wear={0} temp={1}C maxtemp={2}C poweron={3}h readErr(uncorr)={4} writeErr(uncorr)={5} readErrTotal={6} writeErrTotal={7} readLatencyMax={8}ms writeLatencyMax={9}ms startStop={10} loadUnload={11}" -f $c.Wear, $c.Temperature, $c.TemperatureMax, $c.PowerOnHours, $c.ReadErrorsUncorrected, $c.WriteErrorsUncorrected, $c.ReadErrorsTotal, $c.WriteErrorsTotal, $c.ReadLatencyMax, $c.WriteLatencyMax, $c.StartStopCycleCount, $c.LoadUnloadCycleCount)
    } catch { L "      reliability counters: $($_.Exception.Message)" }
}

L "-- Win32_DiskDrive (index N is the N in \Device\HarddiskN of the disk events below)"
foreach ($d in Get-CimInstance Win32_DiskDrive) { L ("  index {0}: {1}  iface={2} status={3} pnp={4}" -f $d.Index, $d.Model, $d.InterfaceType, $d.Status, $d.PNPDeviceID) }

L "-- SMART failure prediction (root\wmi)"
try {
    foreach ($s in Get-CimInstance -Namespace root\wmi -ClassName MSStorageDriver_FailurePredictStatus -ErrorAction Stop) {
        L ("  {0}: PredictFailure={1} Reason={2}" -f $s.InstanceName, $s.PredictFailure, $s.Reason)
    }
} catch { L "  FailurePredictStatus: $($_.Exception.Message)" }
try {
    foreach ($s in Get-CimInstance -Namespace root\wmi -ClassName MSStorageDriver_FailurePredictData -ErrorAction Stop) {
        L ("  attributes for {0}:" -f $s.InstanceName)
        $b = [byte[]]$s.VendorSpecific
        L ("    {0,-4} {1,-26} {2,5} {3,5} {4,8} {5}" -f 'id', 'name', 'cur', 'worst', 'flags', 'raw')
        for ($i = 2; $i + 12 -le $b.Length; $i += 12) {
            $id = $b[$i]; if ($id -eq 0) { continue }
            $flags = $b[$i+1] + 256 * $b[$i+2]; $cur = $b[$i+3]; $worst = $b[$i+4]
            $raw = [long]0; for ($k = 5; $k -ge 0; $k--) { $raw = $raw * 256 + $b[$i+5+$k] }
            $raw16 = [long]($b[$i+5] + 256 * $b[$i+6])
            $n = if ($names.ContainsKey([int]$id)) { $names[[int]$id] } else { 'vendor_' + $id }
            L ("    {0,-4} {1,-26} {2,5} {3,5} {4,8} {5}  (low16={6})" -f $id, $n, $cur, $worst, ('0x{0:x4}' -f $flags), $raw, $raw16)
        }
    }
} catch { L "  FailurePredictData: $($_.Exception.Message)" }
try {
    foreach ($t in Get-CimInstance -Namespace root\wmi -ClassName MSStorageDriver_FailurePredictThresholds -ErrorAction Stop) {
        $b = [byte[]]$t.VendorSpecific; $th = @()
        for ($i = 2; $i + 12 -le $b.Length; $i += 12) { if ($b[$i] -ne 0) { $th += ('{0}:{1}' -f $b[$i], $b[$i+1]) } }
        L ("  thresholds for {0}: {1}" -f $t.InstanceName, ($th -join ' '))
    }
} catch { L "  FailurePredictThresholds: $($_.Exception.Message)" }

L "-- 'disk' events (7 bad block, 51 paging, 153 reset, 157 removal), System log, last 60 days"
$ev = Get-WinEvent -FilterHashtable @{ LogName = 'System'; ProviderName = 'disk'; StartTime = (Get-Date).AddDays(-60) } -ErrorAction SilentlyContinue
if ($ev) {
    $ev | Group-Object Id | ForEach-Object { L ("  id {0}: {1} events, first {2}, last {3}" -f $_.Name, $_.Count, ($_.Group | Sort-Object TimeCreated | Select-Object -First 1).TimeCreated, ($_.Group | Sort-Object TimeCreated | Select-Object -Last 1).TimeCreated) }
    $ev | Sort-Object TimeCreated | Select-Object -Last 40 | ForEach-Object { L ("  [{0}] id={1} {2}" -f $_.TimeCreated, $_.Id, (Short $_.Message 160)) }
} else { L "  none" }
L "-- storahci / stornvme / Ntfs events, System log, last 60 days"
$ev2 = Get-WinEvent -FilterHashtable @{ LogName = 'System'; StartTime = (Get-Date).AddDays(-60) } -ErrorAction SilentlyContinue | Where-Object { $_.ProviderName -in 'storahci', 'stornvme', 'Microsoft-Windows-Ntfs', 'Ntfs', 'volmgr', 'partmgr' }
if ($ev2) { $ev2 | Group-Object ProviderName, Id | ForEach-Object { L ("  {0}: {1} events, last {2}" -f $_.Name, $_.Count, ($_.Group | Sort-Object TimeCreated | Select-Object -Last 1).TimeCreated) }; $ev2 | Sort-Object TimeCreated | Select-Object -Last 20 | ForEach-Object { L ("  [{0}] {1} id={2} {3}" -f $_.TimeCreated, $_.ProviderName, $_.Id, (Short $_.Message 200)) } } else { L "  none" }

L "-- volumes"
Get-Volume | Where-Object DriveLetter | ForEach-Object { L ("  {0}: {1} {2} health={3} op={4} size={5} GB free={6} GB" -f $_.DriveLetter, $_.FileSystemLabel, $_.FileSystem, $_.HealthStatus, $_.OperationalStatus, [math]::Round($_.Size/1e9,1), [math]::Round($_.SizeRemaining/1e9,1)) }

if (-not $OutFile) { $OutFile = Join-Path $PSScriptRoot ('upgrade_\reports\smart-diag-' + $env:COMPUTERNAME + '.txt') }
New-Item -ItemType Directory -Path (Split-Path $OutFile -Parent) -Force | Out-Null
[IO.File]::WriteAllLines($OutFile, $o.ToArray(), (New-Object Text.UTF8Encoding($false)))
Write-Host "  written: $OutFile  ($($o.Count) lines). Nothing on this computer was changed."
