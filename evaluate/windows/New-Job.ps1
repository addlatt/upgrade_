<#
.SYNOPSIS
    upgrade_ - the job writer: this machine's facts -> job.json (schema job/1).

.DESCRIPTION
    The evaluate module's output (architecture.md, "Output"): the validated
    job spec the converter executes with no further human input. This first
    version writes what the LIVE-BOOT leg needs and refuses what the rules
    forbid: a RED verdict never gets a job; legacy BIOS never gets a job; an
    unreadable BitLocker state never gets a job; a Windows time zone with no
    IANA mapping is a refusal, not a guess.

    The folder map (0.10.0, 2026-09-26): the person's six known folders,
    read by the harvester (Harvest-UpgradeState.ps1 -FolderMapOut, beside
    this script, in its own process), go into harvest.folders with their
    sizes, and harvest.stick_fit says whether they fit on the stick. It
    refuses when the map could be wrong: the folders belong to another
    account than the one signed in, a folder could not be fully read or
    counted (R6), or OneDrive online-only files were found and not made
    local by -Materialize and then found online-only again, or the download
    failed (R8). Online-only files that were not downloaded are NOT a
    refusal (decided 2026-09-26): they are recorded as left-in-cloud, and
    settle-in reconnects OneDrive instead of copying them. A clean-slate job whose folders do not fit
    the stick, or on a machine with other people's profiles (R5), is
    refused with the gap.

    The clock and Wi-Fi (0.15.0, 2026-09-27): harvest.clock records the
    time zone and whether the hardware clock holds local time; harvest.wifi
    lists the saved networks, and their passwords go to files under
    artifacts/credentials/wifi/ (secrets are files, never fields; RISKS
    R13). They are read one by one from the Native Wifi API and counted
    against the profiles Windows stores on disk; a mismatch is a refusal.
    The export runs only after every other check passed, and never for a
    verify-only job.

    Which Windows, and how it was activated (0.16.0, 2026-09-27, RISKS
    R30): harvest.windows_license records the edition, 10 or 11 (from the
    build: Windows 11's registry still says "Windows 10"), whether it is
    activated, the licence channel, and whether the firmware holds a key -
    for the way back to Windows. Never a key: the firmware key's presence is
    tested and the key dropped at once, and any value shaped like a key is
    left out. A failed read is recorded with its reason, not refused.

    What it does NOT yet do (said plainly so the job is read as what it is):
    it does not harvest browsers (that block is empty),
    does not extract the BitLocker key
    (a placeholder file is written where the key would go), and takes the
    account password hash as a parameter - the intent-capture UI that asks
    for a password is not built. -PasswordHash defaults to the hash of the
    word "verify-only": the reversible leg never creates the account.

.PARAMETER ScanDir
    Directory holding the scanner's -Json report (the newest one is used).
    The verdict comes from there; RED refuses.

.PARAMETER StickDrive
    Drive letter of the stick (e.g. E:). Its disk identity goes into the job.

.PARAMETER OutDir
    Where job.json and artifacts/ go - the stick's upgrade_\ directory.

.PARAMETER Desktop
    kde (default) or gnome - the edition installed.

.PARAMETER StartAt
    desktop or console: what the installed computer shows when it starts,
    as the person chose it on the launcher (2026-09-26). Required - a job is
    never written with a choice nobody made.

.PARAMETER EraseEverything
    The one-click erase and install (decided 2026-09-26, RISKS R27): the
    sentence the person typed on RUN-ERASE-AND-INSTALL.cmd. Verbatim, it
    makes a clean-slate job (reason user-chose-fresh-start) that names
    every internal drive to erase in erase_consent.disks: the drive with
    C: as system, a second internal drive as home. Anything else typed is
    a refusal. Needs -PasswordHashFile.

.PARAMETER PasswordHashFile
    A file holding the SHA-512 crypt hash of the account password
    (Read-Password.ps1 writes it). Required for an erase job: the default
    hash is the verify-only placeholder, and an installed account needs a
    password its owner chose.

.PARAMETER VerifyOnly
    The job is for the live-boot check only (RUN-VERIFY.cmd): nothing is
    installed, so the placeholder password hash is allowed. Any other job
    needs -PasswordHashFile - an installed account has a password its
    owner chose (2026-09-26: the keep-Windows launchers asked for none).

.PARAMETER PrintLinuxName
    Print the Linux sign-in name this job writer derives for the Windows
    account running it, and exit. The launchers show it before the
    password is chosen, so the name on the screen is the one in the job.

.PARAMETER HarvestSettingsOut
    Run only the clock, Wi-Fi and licence harvest (the same functions a job
    uses), write { clock, wifi, windows_license } as JSON to this file, and the Wi-Fi password
    files under -OutDir, then exit. For the rig, whose job is built by a
    stand-in (rig/hyperv/v1-job.py) because Hyper-V has no USB stick; it
    keeps the product code the thing under test. Refusals exit 2.

.PARAMETER Materialize
    Download OneDrive online-only files in the folders before the map is
    written (RISKS R8). Not used by any launcher: decided 2026-09-26 that
    online-only files stay in OneDrive and settle-in reconnects it.
#>
[CmdletBinding()]
param(
    [string]$ScanDir,
    [string]$StickDrive,
    [string]$OutDir,
    [ValidateSet('kde', 'gnome')][string]$Desktop = 'kde',
    [ValidateSet('desktop', 'console')][string]$StartAt,
    [string]$PasswordHash = '$6$upgradeV1$MkYfbaBe.FFp2fzSNrPiJ6RdPagcfI.crkepTcQpGsjGFMe8780OtkedouSyxvXdky5a6WiTWDy/.epwkWUk71',
    [ValidateSet('clean-slate', 'stop')][string]$IfCannotKeep = 'stop',
    [string]$AcknowledgeDataLoss,
    [switch]$Materialize,
    [string]$EraseEverything,
    [string]$PasswordHashFile,
    [switch]$VerifyOnly,
    [switch]$PrintLinuxName,
    [string]$HarvestSettingsOut,
    [switch]$SelfTest
)
$ErrorActionPreference = 'Stop'
$JobWriterVersion = '0.16.0'
# the harvester versions whose folder map this writer reads; any other is refused, not guessed
$KnownHarvestVersions = @('0.3.0')
$LinuxMinGB = 25
# The acknowledged-data-loss path (RISKS R23, decided 2026-09-13). The person
# types this sentence, verbatim, on the separate launcher; it lifts exactly
# the two refusals whose failure mode is losing THIS machine's files, and
# nothing else. Kept in one place so every module compares the same bytes.
$RiskStatement = 'I confirm that I understand the risks and could lose data'
# The one-click erase and install (RISKS R27, decided 2026-09-26): the person
# types this on RUN-ERASE-AND-INSTALL.cmd. Separate from $RiskStatement; neither
# stands in for the other. Kept in one place so every module compares the same bytes.
$EraseStatement = 'I confirm that everything on this computer will be deleted and nothing will be kept'
$VerifyOnlyHash = '$6$upgradeV1$MkYfbaBe.FFp2fzSNrPiJ6RdPagcfI.crkepTcQpGsjGFMe8780OtkedouSyxvXdky5a6WiTWDy/.epwkWUk71'
$AcknowledgeableChecks = @{ 'Disk health' = 'disk-health'; 'Volume health' = 'volume-health' }

function Test-JobAdmin {
    $id = [Security.Principal.WindowsIdentity]::GetCurrent()
    (New-Object Security.Principal.WindowsPrincipal($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

# --- pure mappers (self-tested) ------------------------------------------------

function ConvertTo-JobIanaTimeZone {
    param([string]$WindowsId)
    $map = @{
        'Eastern Standard Time' = 'America/New_York'; 'Central Standard Time' = 'America/Chicago'
        'Mountain Standard Time' = 'America/Denver'; 'Pacific Standard Time' = 'America/Los_Angeles'
        'Alaskan Standard Time' = 'America/Anchorage'; 'Hawaiian Standard Time' = 'Pacific/Honolulu'
        'US Mountain Standard Time' = 'America/Phoenix'; 'Atlantic Standard Time' = 'America/Halifax'
        'GMT Standard Time' = 'Europe/London'; 'W. Europe Standard Time' = 'Europe/Berlin'
        'Romance Standard Time' = 'Europe/Paris'; 'Central Europe Standard Time' = 'Europe/Budapest'
        'Central European Standard Time' = 'Europe/Warsaw'; 'E. Europe Standard Time' = 'Europe/Chisinau'
        'FLE Standard Time' = 'Europe/Kiev'; 'GTB Standard Time' = 'Europe/Athens'
        'AUS Eastern Standard Time' = 'Australia/Sydney'; 'Tokyo Standard Time' = 'Asia/Tokyo'
        'India Standard Time' = 'Asia/Kolkata'; 'China Standard Time' = 'Asia/Shanghai'
        'Singapore Standard Time' = 'Asia/Singapore'; 'New Zealand Standard Time' = 'Pacific/Auckland'
        'UTC' = 'UTC'
    }
    $map[$WindowsId]
}

function ConvertTo-JobKeymap {
    # Windows input method tip "LANGID:KLID" -> a kickstart/xkb layout name.
    param([string]$InputMethodTip)
    $klid = if ($InputMethodTip -match ':([0-9A-Fa-f]{8})$') { $matches[1].ToLower() } else { '' }
    $map = @{ '00000409' = 'us'; '00000809' = 'gb'; '00000407' = 'de'; '0000040c' = 'fr'; '0000080c' = 'be'
              '00000410' = 'it'; '0000040a' = 'es'; '00000c0a' = 'es'; '00000416' = 'br'; '00000816' = 'pt'
              '00000413' = 'nl'; '0000041d' = 'se'; '00000414' = 'no'; '00000406' = 'dk'; '0000040b' = 'fi'
              '00000807' = 'ch'; '00000405' = 'cz'; '00000415' = 'pl'; '00001009' = 'ca'; '00000c0c' = 'ca' }
    if ($map.ContainsKey($klid)) { return $map[$klid] }
    $null
}

function ConvertTo-JobLinuxName {
    param([string]$WindowsName)
    $n = ($WindowsName.ToLower() -replace '[^a-z0-9_-]', '')
    if ($n -eq '' -or $n -match '^[0-9]') { $n = 'user' }
    $n.Substring(0, [Math]::Min(32, $n.Length))
}

function ConvertTo-JobClock {
    # Pure (self-tested): Windows' clock facts -> harvest.clock (decided
    # 2026-09-26: the Aspire's installer clock was 4 h behind because Windows
    # keeps the hardware clock in local time). settle-in reads the hardware
    # clock as the local time it is and converts it with these facts.
    # RealTimeIsUniversal: absent or 0 = local time (Windows' default), 1 =
    # UTC; any other value is not guessed at.
    param([string]$WindowsZone, [string]$Iana, $RealTimeIsUniversal, $DynamicDstDisabled,
          [int]$OffsetMinutes, [int]$BaseOffsetMinutes, [bool]$DstActive, [string]$NowUtc)
    $rtu = $null; $local = $null
    if ($null -eq $RealTimeIsUniversal) { $local = $true }
    elseif ("$RealTimeIsUniversal" -match '^[01]$') { $rtu = [int]"$RealTimeIsUniversal"; $local = ($rtu -eq 0) }
    if ($null -eq $local) { return @{ Refusal = "Windows' RealTimeIsUniversal setting is '$RealTimeIsUniversal', neither 0 nor 1 - whether the hardware clock holds local time or UTC is not known, and it is not guessed"; Clock = $null } }
    $dstAuto = -not ("$DynamicDstDisabled" -eq '1')
    @{ Refusal = $null; Clock = [ordered]@{
        windows_zone = $WindowsZone; iana = $Iana
        rtc_is_local = $local; real_time_is_universal = $rtu
        dst_auto_adjust = $dstAuto
        utc_offset_minutes = $OffsetMinutes; base_utc_offset_minutes = $BaseOffsetMinutes; dst_active = $DstActive
        observed_utc = $NowUtc } }
}

function ConvertTo-JobLicense {
    # Pure (self-tested): Windows' licence facts -> harvest.windows_license
    # (decided 2026-09-27, RISKS R30), for the way back to Windows. Facts,
    # never a key: the live half passes only whether the firmware holds one,
    # and any value shaped like a product key is left out here as well.
    # A failed read is 'unreadable' with its reason - it costs a less
    # informed way back, never data, so it is not a refusal.
    param($Os, $Products, $Firmware, [string]$ReadError, [string]$NowUtc)
    $keyShape = '[A-Za-z0-9]{5}-[A-Za-z0-9]{5}-[A-Za-z0-9]{5}-[A-Za-z0-9]{5}-[A-Za-z0-9]{5}'
    function clean($v) { if ($null -eq $v -or "$v" -eq '') { return $null }; if ("$v" -match $keyShape) { $script:licDropped = $true; return $null }; "$v" }
    $script:licDropped = $false
    $build = $null; if ("$($Os.Build)" -match '^\d+$') { $build = [int]"$($Os.Build)" }
    $ver = $null; if ($build) { $ver = $(if ($build -ge 22000) { '11' } else { '10' }) }
    $lic = [ordered]@{
        result = 'read'; reason = $null; windows_version = $ver
        edition_id = (clean $Os.EditionId); product_name = (clean $Os.ProductName); display_version = (clean $Os.DisplayVersion); build = $build
        activated = $null; license_status = $null; channel = $null
        firmware_key_present = $null; firmware_key_description = $null
        observed_utc = $NowUtc }
    if ($ReadError) {
        $lic.result = 'unreadable'; $lic.reason = (clean "Windows' licensing service could not be read ($ReadError)")
        if (-not $lic.reason) { $lic.reason = "Windows' licensing service could not be read" }
    } else {
        # the Windows licence itself, not an add-on (the Windows 10 extended updates are one); a licensed one first
        $main = @($Products | Where-Object { -not $_.Addon })
        $p = @($main | Where-Object { [int]$_.LicenseStatus -eq 1 }) + @($main | Where-Object { [int]$_.LicenseStatus -ne 1 }) | Select-Object -First 1
        if ($p) {
            $st = [int]$p.LicenseStatus
            if ($st -ge 0 -and $st -le 6) { $lic.license_status = $st }
            $lic.activated = ($st -eq 1); $lic.channel = (clean $p.Channel)
        } else { $lic.result = 'unreadable'; $lic.reason = 'Windows reported no installed Windows licence' }
        if ($null -ne $Firmware) { $lic.firmware_key_present = [bool]$Firmware.Present; $lic.firmware_key_description = (clean $Firmware.Description) }
    }
    if ($script:licDropped) { $lic.reason = $(if ($lic.reason) { "$($lic.reason); " } else { '' }) + 'a value shaped like a product key was left out' }
    $lic
}

function ConvertFrom-JobWlanProfile {
    # Pure (self-tested): one Windows Wi-Fi profile (the XML the Native Wifi
    # API returns) -> a harvest.wifi.profiles row. The row never carries the
    # password: only whether there is one. What Linux can join is WPA/WPA2/
    # WPA3 personal and open networks (decided 2026-09-26); the rest is
    # listed with the reason, never guessed. WPA3 in transition mode (the
    # router also takes WPA2; 9 of 9 WPA3 profiles on the G16, 2026-09-27)
    # is joined as WPA2-personal, which such a router accepts.
    param([string]$Xml)
    try { [xml]$x = $Xml } catch { return $null }
    $p = $x.WLANProfile
    if (-not $p -or -not $p.SSIDConfig) { return $null }
    $ssidNode = @($p.SSIDConfig.SSID)[0]
    $ssid = "$($ssidNode.name)"
    $hex = "$($ssidNode.hex)".ToUpper()
    if (-not $hex) { $hex = (@([Text.Encoding]::UTF8.GetBytes($ssid) | ForEach-Object { $_.ToString('X2') }) -join '') }
    $sec = $p.MSM.security
    $auth = "$($sec.authEncryption.authentication)"; $enc = "$($sec.authEncryption.encryption)"
    $onex = "$($sec.authEncryption.useOneX)" -eq 'true'
    $transition = $false
    foreach ($n in @($sec.authEncryption.ChildNodes)) { if ($n.LocalName -eq 'transitionMode' -and "$($n.InnerText)" -eq 'true') { $transition = $true } }
    $hasKey = [bool]($sec.sharedKey -and "$($sec.sharedKey.protected)" -eq 'false' -and "$($sec.sharedKey.keyMaterial)")
    $km = 'UNSUPPORTED'; $why = $null
    if ("$($p.connectionType)" -ne 'ESS') { $why = 'an ad-hoc (computer-to-computer) network' }
    elseif ($onex -or $auth -in @('WPA', 'WPA2', 'WPA3', 'WPA3ENT', 'WPA3ENT192')) { $why = 'an enterprise network (a company or school sign-in)' }
    elseif ($auth -eq 'open' -and $enc -eq 'none') { $km = 'none' }
    elseif ($enc -eq 'WEP') { $why = 'WEP, an old and broken kind of Wi-Fi security' }
    elseif ($auth -in @('WPAPSK', 'WPA2PSK') -or ($auth -eq 'WPA3SAE' -and $transition)) { $km = 'wpa-psk' }
    elseif ($auth -eq 'WPA3SAE') { $km = 'sae' }
    else { $why = "a kind of Wi-Fi security this version does not set up ($auth/$enc)" }
    if ($km -in @('wpa-psk', 'sae') -and -not $hasKey) { $why = 'its password could not be read from Windows'; $km = 'UNSUPPORTED' }
    [ordered]@{
        name = "$($p.name)"; ssid = $ssid; ssid_hex = $hex
        hidden = ("$($p.SSIDConfig.nonBroadcast)" -eq 'true')
        windows_auth = "$auth/$enc"; key_mgmt = $km; supported = ($km -ne 'UNSUPPORTED')
        autoconnect = ("$($p.connectionMode)" -eq 'auto'); why_not = $why; secrets_file = $null
        HasKey = $hasKey }
}

function ConvertTo-JobWifi {
    # Pure (self-tested): what the Native Wifi API returned + how many profiles
    # Windows has stored on disk -> harvest.wifi, the files to write, or a
    # refusal. The two counts are independent; if they disagree, a network
    # would be silently missing, so it refuses (netsh's all-at-once export
    # lost one of 14 on the G16, 2026-09-27: it shortens file names to fit
    # the folder and two collided).
    param($Api, [int]$StoredCount, [string]$Dir = 'artifacts/credentials/wifi')
    if ($Api.Error) { return @{ Refusal = "the saved Wi-Fi networks could not be read ($($Api.Error))" } }
    if (-not $Api.Present) {
        if ($StoredCount -gt 0) { return @{ Refusal = "Windows has $StoredCount saved Wi-Fi network(s), but its Wi-Fi service is not running, so they cannot be read" } }
        return @{ Refusal = $null; Wifi = [ordered]@{ result = 'no-wireless'; secrets_dir = $null; profiles = @() }; Files = @() }
    }
    $rows = @(); $files = @(); $seen = @{}
    foreach ($pr in @($Api.Profiles)) {
        $r = ConvertFrom-JobWlanProfile -Xml $pr.Xml
        if (-not $r) { continue }
        $rows += , @{ Row = $r; Xml = $pr.Xml }
    }
    if ($rows.Count -ne $StoredCount) { return @{ Refusal = "Windows has $StoredCount saved Wi-Fi network(s) on disk but $($rows.Count) could be read - one would be missing after the move" } }
    $out = @(); $n = 0
    foreach ($e in $rows) {
        $r = $e.Row; $k = "$($r.name)|$($r.ssid_hex)"
        if ($seen.ContainsKey($k)) { continue }   # the same network saved on two Wi-Fi adapters
        $seen[$k] = $true
        if ($r.supported -and $r.HasKey) { $n++; $r.secrets_file = ('{0}/{1:D2}.xml' -f $Dir, $n); $files += , @{ Rel = $r.secrets_file; Xml = $e.Xml } }
        $r.Remove('HasKey'); $out += , $r
    }
    $res = if ($out.Count -eq 0) { 'none-saved' } else { 'exported' }
    @{ Refusal = $null; Wifi = [ordered]@{ result = $res; secrets_dir = $(if ($files.Count -gt 0) { $Dir } else { $null }); profiles = $out }; Files = $files }
}

function Get-JobClockFacts {
    # Live half of harvest.clock: the registry and .NET's view of the zone.
    $k = 'HKLM:\SYSTEM\CurrentControlSet\Control\TimeZoneInformation'
    $ti = Get-ItemProperty -Path $k -ErrorAction SilentlyContinue
    $tz = [TimeZoneInfo]::Local; $now = [DateTime]::UtcNow
    @{ WindowsZone = $tz.Id
       RealTimeIsUniversal = $(if ($ti -and $null -ne $ti.PSObject.Properties['RealTimeIsUniversal']) { $ti.RealTimeIsUniversal } else { $null })
       DynamicDstDisabled = $(if ($ti -and $null -ne $ti.PSObject.Properties['DynamicDaylightTimeDisabled']) { $ti.DynamicDaylightTimeDisabled } else { $null })
       OffsetMinutes = [int]$tz.GetUtcOffset($now).TotalMinutes; BaseOffsetMinutes = [int]$tz.BaseUtcOffset.TotalMinutes
       DstActive = $tz.IsDaylightSavingTime($now); NowUtc = $now.ToString('yyyy-MM-ddTHH:mm:ssZ') }
}

function Get-JobLicenseFacts {
    # Live half of harvest.windows_license: the registry and Windows'
    # licensing service (read-only). The firmware key is tested for being
    # there and dropped on the spot; it never leaves this function.
    $k = Get-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion' -ErrorAction SilentlyContinue
    $os = @{ ProductName = "$($k.ProductName)"; EditionId = "$($k.EditionID)"; DisplayVersion = "$($k.DisplayVersion)"; Build = "$($k.CurrentBuild)" }
    $r = @{ Os = $os; Products = @(); Firmware = $null; Error = $null; NowUtc = [DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ssZ') }
    try {
        $r.Products = @(Get-CimInstance -ClassName SoftwareLicensingProduct -Filter "ApplicationID='55c92734-d682-4d71-983e-d6ec3f16059f' AND PartialProductKey IS NOT NULL" -ErrorAction Stop | ForEach-Object {
            $ch = "$($_.ProductKeyChannel)"
            if (-not $ch -and "$($_.Description)" -match ',\s*(\S+)\s+channel') { $ch = $matches[1] }
            @{ LicenseStatus = [int]$_.LicenseStatus; Channel = $ch; Addon = [bool]$_.LicenseIsAddon } })
        $svc = Get-CimInstance -ClassName SoftwareLicensingService -ErrorAction Stop
        $r.Firmware = @{ Present = [bool]("$($svc.OA3xOriginalProductKey)".Trim()); Description = "$($svc.OA3xOriginalProductKeyDescription)" }
        $svc = $null
    } catch { $r.Error = "$($_.Exception.Message)" }
    $r
}

function ConvertTo-JobLicenseFromFacts {
    param($L)
    ConvertTo-JobLicense -Os $L.Os -Products $L.Products -Firmware $L.Firmware -ReadError $L.Error -NowUtc $L.NowUtc
}

function Get-JobWlanProfiles {
    # Live half of harvest.wifi: every saved profile, with its password in
    # clear (WLAN_PROFILE_GET_PLAINTEXT_KEY; elevated), straight from the
    # Native Wifi API - the interface netsh itself uses - one profile at a
    # time by name, so nothing is lost to file naming. Read-only.
    if (-not ('Upg.Wlan' -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
namespace Upg {
    public class WlanProfile { public Guid Interface; public string Name; public string Xml; }
    public static class Wlan {
        [DllImport("wlanapi.dll")] static extern uint WlanOpenHandle(uint ver, IntPtr res, out uint negotiated, out IntPtr handle);
        [DllImport("wlanapi.dll")] static extern uint WlanCloseHandle(IntPtr handle, IntPtr res);
        [DllImport("wlanapi.dll")] static extern uint WlanEnumInterfaces(IntPtr handle, IntPtr res, out IntPtr list);
        [DllImport("wlanapi.dll")] static extern uint WlanGetProfileList(IntPtr handle, ref Guid iface, IntPtr res, out IntPtr list);
        [DllImport("wlanapi.dll", CharSet = CharSet.Unicode)]
        static extern uint WlanGetProfile(IntPtr handle, ref Guid iface, string name, IntPtr res, out IntPtr xml, ref uint flags, out uint access);
        [DllImport("wlanapi.dll")] static extern void WlanFreeMemory(IntPtr p);
        const uint PlaintextKey = 4;          // WLAN_PROFILE_GET_PLAINTEXT_KEY
        const int InterfaceInfoSize = 532;    // GUID + WCHAR[256] + DWORD
        const int ProfileInfoSize = 516;      // WCHAR[256] + DWORD
        // 1062 = ERROR_SERVICE_NOT_ACTIVE: no Wi-Fi service, so no Wi-Fi.
        public static uint Read(List<WlanProfile> into) {
            uint neg; IntPtr h; uint rc = WlanOpenHandle(2, IntPtr.Zero, out neg, out h);
            if (rc != 0) return rc;
            try {
                IntPtr il; rc = WlanEnumInterfaces(h, IntPtr.Zero, out il); if (rc != 0) return rc;
                try {
                    int ni = Marshal.ReadInt32(il);
                    for (int i = 0; i < ni; i++) {
                        IntPtr item = new IntPtr(il.ToInt64() + 8 + (long)i * InterfaceInfoSize);
                        Guid g = (Guid)Marshal.PtrToStructure(item, typeof(Guid));
                        IntPtr pl; rc = WlanGetProfileList(h, ref g, IntPtr.Zero, out pl); if (rc != 0) return rc;
                        try {
                            int np = Marshal.ReadInt32(pl);
                            for (int j = 0; j < np; j++) {
                                string name = Marshal.PtrToStringUni(new IntPtr(pl.ToInt64() + 8 + (long)j * ProfileInfoSize));
                                uint flags = PlaintextKey, access; IntPtr x;
                                rc = WlanGetProfile(h, ref g, name, IntPtr.Zero, out x, ref flags, out access); if (rc != 0) return rc;
                                try { into.Add(new WlanProfile { Interface = g, Name = name, Xml = Marshal.PtrToStringUni(x) }); }
                                finally { WlanFreeMemory(x); }
                            }
                        } finally { WlanFreeMemory(pl); }
                    }
                } finally { WlanFreeMemory(il); }
            } finally { WlanCloseHandle(h, IntPtr.Zero); }
            return 0;
        }
    }
}
'@
    }
    $list = New-Object 'System.Collections.Generic.List[Upg.WlanProfile]'
    try { $rc = [Upg.Wlan]::Read($list) }
    catch [DllNotFoundException] { return @{ Present = $false; Profiles = @(); Error = $null } }
    if ($rc -eq 1062) { return @{ Present = $false; Profiles = @(); Error = $null } }
    if ($rc -ne 0) { return @{ Present = $true; Profiles = @(); Error = "Windows' Wi-Fi interface answered error $rc" } }
    @{ Present = $true; Profiles = @($list); Error = $null }
}

function Get-JobWlanStoredCount {
    # The second, independent count: the profile files Windows keeps on disk.
    # Only real network profiles count (a WLANProfile with an SSID); the G16
    # has one more file there that is not one (2026-09-27).
    $n = 0
    foreach ($f in @(Get-ChildItem -Path "$env:ProgramData\Microsoft\Wlansvc\Profiles\Interfaces" -Recurse -Filter *.xml -ErrorAction SilentlyContinue)) {
        try { [xml]$x = Get-Content -LiteralPath $f.FullName -Raw -ErrorAction Stop; if ($x.WLANProfile -and $x.WLANProfile.SSIDConfig) { $n++ } } catch { }
    }
    $n
}

function Export-JobWifi {
    # Live: read, judge, write the password files under <OutDir>. Returns
    # @{ Refusal; Wifi }. On any refusal or write failure nothing is left
    # behind: the directory is removed.
    param([string]$OutDir)
    $w = ConvertTo-JobWifi -Api (Get-JobWlanProfiles) -StoredCount (Get-JobWlanStoredCount)
    $dir = Join-Path $OutDir 'artifacts\credentials\wifi'
    if (Test-Path -LiteralPath $dir) { Remove-Item -LiteralPath $dir -Recurse -Force }
    if ($w.Refusal) { return $w }
    try {
        foreach ($f in @($w.Files)) {
            $p = Join-Path $OutDir ($f.Rel -replace '/', '\')
            New-Item -ItemType Directory -Path (Split-Path $p) -Force | Out-Null
            [IO.File]::WriteAllText($p, $f.Xml, (New-Object Text.UTF8Encoding($false)))
        }
    } catch {
        Remove-Item -LiteralPath $dir -Recurse -Force -ErrorAction SilentlyContinue
        return @{ Refusal = "the Wi-Fi passwords could not be written to the stick ($($_.Exception.Message))" }
    }
    $w
}

function Get-JobPath {
    # Pure: the path decision (architecture.md, "The conversion path is not a
    # coin flip"): keep Windows whenever the disk can, else clean slate, forced.
    # A volume that carries the dirty flag cannot be measured at all (RISKS
    # R18): on a Healthy disk that is NOT "no room" - it is a keep-windows job
    # whose number the prologue measures after its disk check, branching on
    # fork.if_cannot_keep if it then does not fit (decided 2026-09-08; the
    # 0.1.0 writer forced clean slate here, which pre-empted the fork).
    # Same rule for a volume whose dirty bit is clean while Windows has a full
    # chkdsk queued (NTFS event 98 / Get-Volume "Full Repair Needed"): on the
    # Aspire (2026-09-17) that state made the Storage API answer 0 GB with no
    # error, and the 0.4.0 writer forced clean slate on a number that was not
    # a measurement. RISKS R18.
    # And (2026-09-20, the same machine, read over SSH): the 0 GB was the cold
    # floor with hiberfil.sys on the last cluster - Windows names the last
    # unmovable file itself (Defrag event 259). When that file is one the
    # prologue turns off before it measures again (hibernation, page, swap),
    # a small cold number is not "no room": keep-windows, fork pending.
    # And (2026-09-22, the Aspire's fifth run, R18): the forced fallback is
    # the person's to allow, not ours. With if_cannot_keep = stop, the 0.7.0
    # writer still wrote clean slate for a cold 3.2 GB pinned by $UsnJrnl -
    # a wipe job the person had declined, under a launcher that described
    # keep-Windows. Under stop the answer is keep-windows whenever the disk
    # and the ESP allow it (the prologue re-measures and stops if it still
    # does not fit), and no path at all - a refusal - when they do not.
    param([string]$DiskHealth, [bool]$EspFits, $ShrinkableGB, [string]$Dirty = 'clean', [bool]$DiskHealthAcknowledged = $false, [bool]$RepairQueued = $false, [bool]$Mitigable = $false, [string]$IfCannotKeep = 'stop')
    if (($DiskHealth -eq 'Healthy' -or $DiskHealthAcknowledged) -and $EspFits) {
        if ($null -ne $ShrinkableGB -and $ShrinkableGB -ge $LinuxMinGB) { return @{ Path = 'keep-windows'; Reason = 'default' } }
        if ($null -ne $ShrinkableGB -and $Mitigable) { return @{ Path = 'keep-windows'; Reason = 'default' } }
        if ($null -eq $ShrinkableGB -and ($Dirty -eq 'dirty' -or $RepairQueued)) { return @{ Path = 'keep-windows'; Reason = 'default' } }
        if ($IfCannotKeep -ne 'clean-slate') { return @{ Path = 'keep-windows'; Reason = 'default' } }
    }
    if ($IfCannotKeep -ne 'clean-slate') { return @{ Path = $null; Reason = $null } }
    @{ Path = 'clean-slate'; Reason = 'forced-no-room' }
}

function ConvertFrom-JobFsutilDirty {
    param([string[]]$Lines)
    $t = (@($Lines) -join "`n")
    if ($t -match '(?i)\bis\s+NOT\s+Dirty\b') { return 'clean' }
    if ($t -match '(?i)\bis\s+Dirty\b') { return 'dirty' }
    'unknown'
}

function Test-JobRepairQueued {
    # Pure (self-tested): does Windows say C: has an offline repair queued,
    # whatever the dirty bit says? Two of its own statements count: the
    # volume's OperationalStatus naming a repair ("Full Repair Needed" on the
    # Aspire, 2026-09-13) and NTFS event 98 ("needs to be taken offline to
    # perform a Full Chkdsk") within the last 30 days. RISKS R18.
    # An event is history, not state (2026-09-20): on the Aspire the full
    # check ran on 09-15 (autochk log, Wininit 1001) and the 09-13 event 98
    # still sat inside the 30-day window. An event 98 counts only when no
    # completed check postdates it; the volume's current status always counts.
    param([string]$VolumeStatus, $NtfsFullChkdsk, $LastCheck)
    $why = @(); $stale = ''
    if ("$VolumeStatus" -match '(?i)repair') { $why += "Get-Volume reports '$VolumeStatus'" }
    if ($NtfsFullChkdsk) {
        if ($LastCheck -and ([DateTime]$LastCheck) -gt ([DateTime]$NtfsFullChkdsk)) { $stale = "NTFS asked for a full chkdsk on $NtfsFullChkdsk; a boot-time check completed after it, on $LastCheck" }
        else { $why += "NTFS logged on $NtfsFullChkdsk that C: needs a full chkdsk" }
    }
    @{ Queued = ($why.Count -gt 0); Why = ($why -join '; '); Stale = $stale }
}

function ConvertFrom-JobDefrag259 {
    # Pure (self-tested): Defrag event 259's text -> the last unmovable file
    # ("\hiberfil.sys"), or $null. Windows writes it after a shrink analysis.
    param([string]$Message)
    if ("$Message" -match '(?im)last unmovable file appears to be:\s*(\S.*?)\s*$') { return ($matches[1] -replace '::\$DATA$', '') }
    $null
}

function Test-JobShrinkMitigable {
    # Pure (self-tested): is the file pinning the shrink floor one the prologue
    # turns off before it measures again? Only these three; anything else
    # (the MFT, a restore point, $BadClus, a user's file) is not ours to move.
    param([string]$LastUnmovable)
    [bool]("$LastUnmovable" -match '(?i)^\\?(hiberfil|pagefile|swapfile)\.sys$')
}

function Get-JobLastUnmovable {
    # Live half, read-only. The shrink analysis behind Get-PartitionSupportedSize
    # logs Defrag 259; if none appeared, diskpart's `shrink querymax` (which only
    # reports) makes Windows write one.
    param([DateTime]$Since)
    function read259 { try { $e = Get-WinEvent -FilterHashtable @{ LogName = 'Application'; ProviderName = 'Microsoft-Windows-Defrag'; Id = 259; StartTime = $Since } -ErrorAction SilentlyContinue | Sort-Object TimeCreated -Descending | Select-Object -First 1; if ($e) { ConvertFrom-JobDefrag259 -Message "$($e.Message)" } } catch { } }
    $f = read259
    if (-not $f) {
        try {
            $s = [IO.Path]::GetTempFileName(); "select volume C`r`nshrink querymax`r`n" | Set-Content -Path $s -Encoding ASCII
            & diskpart /s $s 2>&1 | Out-Null; Remove-Item $s -Force -ErrorAction SilentlyContinue
            Start-Sleep -Seconds 3; $f = read259
        } catch { }
    }
    $f
}

function Get-JobRepairQueued {
    # Live half: the same two reads the scanner and the prologue make.
    $status = $null; $n98 = $null
    try { $v = Get-Volume -DriveLetter C -ErrorAction Stop; $status = (@($v.OperationalStatus) -join ',') } catch { }
    try {
        $e = @(Get-WinEvent -FilterHashtable @{ LogName = 'System'; Id = 98; StartTime = (Get-Date).AddDays(-30) } -ErrorAction SilentlyContinue |
               Where-Object { "$($_.ProviderName)" -match 'Ntfs' -and "$($_.Message)" -match '(?i)Full Chkdsk' -and "$($_.Message)" -match '(?i)Volume C:' } | Sort-Object TimeCreated -Descending | Select-Object -First 1)
        if ($e.Count) { $n98 = $e[0].TimeCreated }
    } catch { }
    # the last completed boot-time check: its Wininit 1001, or autochk's own log
    $last = $null
    try { $w = Get-WinEvent -FilterHashtable @{ LogName = 'Application'; Id = 1001; StartTime = (Get-Date).AddDays(-60) } -ErrorAction SilentlyContinue | Where-Object { "$($_.ProviderName)" -match 'Wininit' } | Sort-Object TimeCreated -Descending | Select-Object -First 1; if ($w) { $last = $w.TimeCreated } } catch { }
    try { $l = Get-ChildItem 'C:\System Volume Information\Chkdsk' -Force -Filter 'Chkdsk*.log' -ErrorAction Stop | Sort-Object LastWriteTime -Descending | Select-Object -First 1; if ($l -and (-not $last -or $l.LastWriteTime -gt $last)) { $last = $l.LastWriteTime } } catch { }
    Test-JobRepairQueued -VolumeStatus $status -NtfsFullChkdsk $n98 -LastCheck $last
}

function ConvertTo-JobSoftware {
    # Pure (self-tested): raw registry uninstall entries and Store packages ->
    # the harvest.software block. Drops what Apps & features hides (SystemComponent=1),
    # Windows updates and hotfixes, nameless entries, Store frameworks and system
    # packages; de-duplicates by name; sorts; caps at 2000 per list and says so.
    param($Desktop, $Store, [int]$Cap = 2000)
    $d = @{}
    foreach ($e in @($Desktop)) {
        $n = "$($e.DisplayName)".Trim()
        if (-not $n) { continue }
        if ("$($e.SystemComponent)" -eq '1') { continue }
        if ($n -match '^(Security Update|Update|Hotfix|Service Pack)\b.* for ' -or $n -match '^KB\d{6,}') { continue }
        if (-not $d.ContainsKey($n)) { $d[$n] = [ordered]@{ name = $n; version = $(if ($e.DisplayVersion) { "$($e.DisplayVersion)" } else { $null }); publisher = $(if ($e.Publisher) { "$($e.Publisher)" } else { $null }) } }
    }
    $st = @{}
    foreach ($e in @($Store)) {
        if ($e.IsFramework -or "$($e.SignatureKind)" -eq 'System' -or $e.NonRemovable) { continue }
        $n = "$($e.DisplayName)".Trim(); if (-not $n -or $n -match '^ms-resource:') { $n = "$($e.Name)".Trim() }
        if (-not $n) { continue }
        if (-not $st.ContainsKey($n)) { $st[$n] = [ordered]@{ name = $n; package = $(if ($e.Name) { "$($e.Name)" } else { $null }); version = $(if ($e.Version) { "$($e.Version)" } else { $null }); publisher = $(if ($e.PublisherDisplayName) { "$($e.PublisherDisplayName)" } elseif ($e.Publisher) { "$($e.Publisher)" } else { $null }) } }
    }
    $dl = @($d.Keys | Sort-Object | ForEach-Object { $d[$_] }); $sl = @($st.Keys | Sort-Object | ForEach-Object { $st[$_] })
    $trunc = ($dl.Count -gt $Cap) -or ($sl.Count -gt $Cap)
    [ordered]@{ desktop = @($dl | Select-Object -First $Cap); store = @($sl | Select-Object -First $Cap); truncated = $trunc }
}

function Get-JobSoftware {
    # Live half. The registry's Apps & features entries (three hives) and the
    # person's Store packages. Names only; nothing here reads inside a program.
    $desktop = @()
    foreach ($hive in 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*', 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*', 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*') {
        try { $desktop += @(Get-ItemProperty $hive -ErrorAction SilentlyContinue | Select-Object DisplayName, DisplayVersion, Publisher, SystemComponent) } catch { }
    }
    $store = @()
    try { $store = @(Get-AppxPackage -PackageTypeFilter Main -ErrorAction Stop | ForEach-Object {
            $m = $null; try { $m = Get-AppxPackageManifest $_ -ErrorAction Stop } catch { }
            [pscustomobject]@{ Name = $_.Name; Version = "$($_.Version)"; Publisher = $_.Publisher; IsFramework = $_.IsFramework; SignatureKind = "$($_.SignatureKind)"; NonRemovable = $_.NonRemovable
                               DisplayName = $(if ($m) { "$($m.Package.Properties.DisplayName)" } else { '' }); PublisherDisplayName = $(if ($m) { "$($m.Package.Properties.PublisherDisplayName)" } else { '' }) } }) } catch { }
    ConvertTo-JobSoftware -Desktop $desktop -Store $store
}

# --- collection ------------------------------------------------------------------

function Get-JobFolderMap {
    # Live half: the harvester's folder map, in its own process (the shape the
    # R8 materializer was proven in). Returns @{ Map; Error }.
    param([string]$StickDrive, [bool]$Materialize)
    $hs = Join-Path $PSScriptRoot 'Harvest-UpgradeState.ps1'
    if (-not (Test-Path -LiteralPath $hs)) { return @{ Map = $null; Error = 'Harvest-UpgradeState.ps1 is not beside the job writer' } }
    $tmp = Join-Path $env:TEMP ('upgrade-foldermap-' + [guid]::NewGuid().ToString('N') + '.json')
    $a = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $hs, '-FolderMapOut', $tmp)
    if ($StickDrive) { $a += @('-StickDrive', $StickDrive) }
    if ($Materialize) { $a += '-Materialize' }
    & "$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe" @a
    $code = $LASTEXITCODE
    if (-not (Test-Path -LiteralPath $tmp)) { return @{ Map = $null; Error = "the harvester wrote no folder map (exit $code)" } }
    try { $m = Get-Content -LiteralPath $tmp -Raw -Encoding UTF8 | ConvertFrom-Json; @{ Map = $m; Error = $null } }
    catch { @{ Map = $null; Error = "the folder map could not be parsed: $($_.Exception.Message)" } }
    finally { Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue }
}

function Get-JobFacts {
    param([string]$ScanDir, [string]$StickDrive, [bool]$Materialize = $false)
    $f = @{}
    $cs = Get-CimInstance Win32_ComputerSystem; $os = Get-CimInstance Win32_OperatingSystem
    $bios = Get-CimInstance Win32_BIOS; $sys = Get-CimInstance Win32_ComputerSystemProduct
    $f.Vendor = "$($cs.Manufacturer)"; $f.Model = "$($cs.Model)"; $f.Uuid = "$($sys.UUID)"
    $f.BiosSerial = "$($bios.SerialNumber)"; $f.BiosVersion = "$($bios.SMBIOSBIOSVersion)"
    $f.OsCaption = "$($os.Caption)"; $f.OsBuild = [int]$os.BuildNumber
    $f.Firmware = "$env:firmware_type"
    $f.SecureBoot = try { if (Confirm-SecureBootUEFI) { 'on' } else { 'off' } } catch { 'unknown' }

    $part = Get-Partition -DriveLetter C -ErrorAction Stop
    $disk = Get-Disk -Number $part.DiskNumber -ErrorAction Stop
    $f.Disk = @{ Number = $disk.Number; Serial = ("$($disk.SerialNumber)" -replace '\s', ''); UniqueId = "$($disk.UniqueId)"
                 Name = "$($disk.FriendlyName)"; Size = [long]$disk.Size; Style = "$($disk.PartitionStyle)" }
    $pd = Get-PhysicalDisk | Where-Object { "$($_.DeviceId)" -eq "$($disk.Number)" } | Select-Object -First 1
    $f.Health = if ($pd) { "$($pd.HealthStatus)" } else { 'Unknown' }
    # every disk Windows sees, for the erase job's list (R27); the stick is marked below
    $f.AllDisks = @(Get-Disk | ForEach-Object {
        $n = $_.Number; $p2 = Get-PhysicalDisk | Where-Object { "$($_.DeviceId)" -eq "$n" } | Select-Object -First 1
        @{ Number = [int]$n; Serial = ("$($_.SerialNumber)" -replace '\s', ''); UniqueId = "$($_.UniqueId)"; Size = [long]$_.Size; Name = "$($_.FriendlyName)"
           Bus = "$($_.BusType)"; Health = $(if ($p2) { "$($p2.HealthStatus)" } else { 'Unknown' }) } })
    $f.Operational = if ($pd) { (@($pd.OperationalStatus) -join ',') } else { 'unknown' }
    $f.MediaType = if ($pd) { "$($pd.MediaType)" } else { '' }

    $f.ShrinkGB = $null; $f.ShrinkError = $null; $measureStart = (Get-Date).AddSeconds(-5)
    try { $s = Get-PartitionSupportedSize -DriveLetter C -ErrorAction Stop; $f.ShrinkGB = [math]::Round(($part.Size - $s.SizeMin) / 1GB, 1) }
    catch { $f.ShrinkError = ($_.Exception.Message -replace '\s+', ' ').Trim() }

    $f.Dirty = 'unknown'
    try { $f.Dirty = ConvertFrom-JobFsutilDirty -Lines @(& fsutil dirty query C: 2>&1 | ForEach-Object { "$_" }) } catch { }
    $rq = Get-JobRepairQueued; $f.RepairQueued = [bool]$rq.Queued; $f.RepairQueuedWhy = $rq.Why; $f.RepairStale = $rq.Stale
    $f.LastUnmovable = $null
    if ($null -ne $f.ShrinkGB -and $f.ShrinkGB -lt $LinuxMinGB) { $f.LastUnmovable = Get-JobLastUnmovable -Since $measureStart }

    $esp = Get-Partition -DiskNumber $disk.Number | Where-Object { $_.GptType -eq '{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}' } | Select-Object -First 1
    $f.EspSize = 0; $f.EspFree = 0; $f.EspError = $null
    if ($esp) {
        $f.EspSize = [long]$esp.Size
        try { $v = Get-Volume -Partition $esp -ErrorAction Stop; $f.EspFree = [long]$v.SizeRemaining } catch { $f.EspError = "$($_.Exception.Message)" }
    } else { $f.EspError = 'no EFI System Partition on the system disk' }

    $f.BitLocker = 'unknown'
    try { $v = Get-BitLockerVolume -MountPoint 'C:' -ErrorAction Stop; $f.BitLocker = if ($v.ProtectionStatus -eq 'On') { 'on' } elseif ($v.ProtectionStatus -eq 'Off') { 'off' } else { 'unknown' } } catch { }
    if ($f.BitLocker -eq 'unknown') {
        try { $out = (& manage-bde -status C: 2>&1) -join "`n"; if ($out -match '(?im)^\s*Protection Status:\s*Protection (On|Off)\s*$') { $f.BitLocker = $matches[1].ToLower() } } catch { }
    }

    $f.Verdict = $null; $f.RequiredKernel = $null; $f.Report = $null; $f.FailedChecks = @(); $f.WarnChecks = @()
    if ($ScanDir -and (Test-Path $ScanDir)) {
        $j = Get-ChildItem -Path $ScanDir -Filter 'upgrade-report-*.json' | Sort-Object LastWriteTime -Descending | Select-Object -First 1
        if ($j) {
            $r = Get-Content $j.FullName -Raw | ConvertFrom-Json
            $f.Verdict = "$($r.Verdict.Level)"; $f.RequiredKernel = $r.RequiredKernel
            $f.Report = $j.FullName
            # which checks made it RED: only the two acknowledgeable ones may be lifted
            $f.FailedChecks = @($r.Checks | Where-Object { $_.Status -eq 'fail' -and $_.Section -ne 'Software' } | ForEach-Object { "$($_.Title)" })
            $f.WarnChecks = @($r.Checks | Where-Object { $_.Status -eq 'warn' } | ForEach-Object { "$($_.Title)" })
        }
    }

    $f.Stick = $null
    if ($StickDrive) {
        $l = $StickDrive.TrimEnd(':', '\')
        try {
            $sv = Get-Volume -DriveLetter $l -ErrorAction Stop
            $sp = Get-Partition -DriveLetter $l -ErrorAction Stop
            $sd = Get-Disk -Number $sp.DiskNumber -ErrorAction Stop
            $f.Stick = @{ UniqueId = "$($sd.UniqueId)"; Serial = ("$($sd.SerialNumber)" -replace '\s', ''); Size = [long]$sd.Size
                          Name = "$($sd.FriendlyName)"; Label = "$($sv.FileSystemLabel)"; Bus = "$($sd.BusType)" }
        } catch { $f.StickError = "$($_.Exception.Message)" }
    }

    $tz = Get-TimeZone; $loc = Get-WinSystemLocale
    $f.WindowsTz = $tz.Id; $f.Locale = $loc.Name
    $f.Clock = Get-JobClockFacts
    $f.License = Get-JobLicenseFacts
    $f.InputTip = try { (Get-WinUserLanguageList)[0].InputMethodTips[0] } catch { '' }
    $f.Software = Get-JobSoftware
    $fm = Get-JobFolderMap -StickDrive $StickDrive -Materialize $Materialize; $f.Harvest = $fm.Map; $f.HarvestError = $fm.Error
    $f.UserName = $env:USERNAME
    $f.FullName = try { (Get-CimInstance Win32_UserAccount -Filter "Name='$($env:USERNAME)' AND LocalAccount=True" | Select-Object -First 1).FullName } catch { $null }
    $f
}

# --- the job (pure given facts; self-tested) ---------------------------------------

function Get-JobAcknowledgement {
    # Pure (self-tested). Returns @{ Refusal = <text or $null>; Block = <risk_acknowledgement or $null> }.
    # A RED verdict is a job only when (a) the statement was typed verbatim
    # and (b) every failing hardware check is one of the two the statement may
    # lift. The overrides list what it lifted: the failing acknowledgeable
    # checks, plus volume-health whenever that check warned (the repair the
    # prologue will run on the acknowledged disk is itself a data-loss risk).
    param([string]$Verdict, [string[]]$FailedChecks, [string[]]$WarnChecks, [string]$Typed)
    if ($Verdict -ne 'RED') { return @{ Refusal = $null; Block = $null } }
    if (-not $Typed) { return @{ Refusal = 'the scanner verdict is RED - no job, no override'; Block = $null } }
    if ($Typed -cne $RiskStatement) { return @{ Refusal = "the scanner verdict is RED and the data-loss statement was not typed exactly (expected: $RiskStatement)"; Block = $null } }
    $notLiftable = @($FailedChecks | Where-Object { -not $AcknowledgeableChecks.ContainsKey($_) })
    if ($notLiftable.Count -gt 0) { return @{ Refusal = "the scanner verdict is RED for a reason no acknowledgement lifts: $($notLiftable -join ', ')"; Block = $null } }
    $ov = @($FailedChecks | ForEach-Object { $AcknowledgeableChecks[$_] })
    if (($WarnChecks -contains 'Volume health') -and ($ov -notcontains 'volume-health')) { $ov += 'volume-health' }
    if ($ov.Count -eq 0) { return @{ Refusal = 'the scanner verdict is RED but no failing check was found in the report; refusing'; Block = $null } }
    @{ Refusal = $null; Block = [ordered]@{ statement = $RiskStatement; accepted_utc = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ'); overrides = @($ov | Sort-Object -Unique) } }
}

function Get-JobEraseDisks {
    # Pure (self-tested). The drives an erase job names (RISKS R27): every
    # internal drive, the one holding C: first as system, at most one more as
    # home. The stick (by unique id) and removable buses are left alone - the
    # installer is told to use only the listed drives. A bus this version does
    # not know, a third internal drive, a home drive that is not Healthy, or a
    # drive with no unique id is a refusal, never a guess.
    param($Disks, [int]$SystemNumber, [string]$StickUniqueId)
    $internalBus = @('SATA', 'NVMe', 'SAS', 'SCSI', 'ATA', 'RAID')
    $removableBus = @('USB', 'SD', 'MMC')
    $sys = $null; $others = @(); $refusals = @()
    foreach ($d in @($Disks)) {
        if ($StickUniqueId -and "$($d.UniqueId)" -eq $StickUniqueId) { continue }
        if ($removableBus -contains "$($d.Bus)") { continue }
        if ($internalBus -notcontains "$($d.Bus)") { $refusals += "drive $($d.Number) ($($d.Name)) is on bus '$($d.Bus)', which this version neither erases nor knows is safe to leave"; continue }
        if (-not "$($d.UniqueId)") { $refusals += "drive $($d.Number) ($($d.Name)) has no unique id, so the installer could not be sure it is the same drive"; continue }
        if ([int]$d.Number -eq $SystemNumber) { $sys = $d } else { $others += $d }
    }
    if (-not $sys) { $refusals += 'the drive holding C: is not among the internal drives' }
    if ($others.Count -gt 1) { $refusals += "this computer has $($others.Count + 1) internal drives; this version erases at most two" }
    foreach ($o in $others) { if ("$($o.Health)" -ne 'Healthy') { $refusals += "the second drive ($($o.Name)) reports '$($o.Health)', and only a Healthy drive takes your home folder" } }
    if ($refusals.Count -gt 0) { return @{ Refusals = $refusals; Disks = @() } }
    $list = @([ordered]@{ role = 'system'; serial_number = "$($sys.Serial)"; unique_id = "$($sys.UniqueId)"; size_bytes = [long]$sys.Size; friendly_name = "$($sys.Name)"
                          health_status = $(if ("$($sys.Health)" -in @('Healthy', 'Warning', 'Unhealthy')) { "$($sys.Health)" } else { 'Unknown' }) })
    foreach ($o in $others) { $list += [ordered]@{ role = 'home'; serial_number = "$($o.Serial)"; unique_id = "$($o.UniqueId)"; size_bytes = [long]$o.Size; friendly_name = "$($o.Name)"; health_status = 'Healthy' } }
    @{ Refusals = @(); Disks = $list }
}

function Get-JobHarvestRefusals {
    # Pure (self-tested). The folder map is what settle-in pulls and what a
    # clean slate stages to the stick; a map that is wrong is silent data
    # loss, discovered after Windows is gone. So each of these is a refusal,
    # never a note (CLAUDE.md rule #1).
    param($H, [string]$HarvestError)
    if (-not $H) { return @("the list of your folders could not be read ($HarvestError)") }
    if ("$($H.HarvestVersion)" -notin $KnownHarvestVersions) { return @("the folder map comes from harvester '$($H.HarvestVersion)', which this job writer does not read (it reads $($KnownHarvestVersions -join ', ')) - the kit is mixed; nothing was changed") }
    $r = @()
    $o = $H.Owner
    $owners = @($o.DesktopOwnerSids | Where-Object { $_ })
    if ($owners.Count -eq 0) { $r += "could not tell who is signed in on this screen, so could not tell whose folders these are (this window runs as $($o.ProcessName)) - start the launcher from the desktop of the person whose computer this is" }
    elseif ($owners -notcontains "$($o.ProcessSid)") { $r += "this window runs as $($o.ProcessName), but another account is signed in on this screen - the folders read would be the wrong person's. Start the launcher from an account that is itself an administrator" }
    foreach ($f in @($H.UserFolders | Where-Object { $_.Exists })) {
        if ($f.Truncated) { $r += "$($f.Name) holds more files than this version counts ($($f.Files)), so its size would be too low (RISKS R6)" }
        if ([int]$f.Unreadable -gt 0) { $r += "Windows would not let the harvest read $($f.Unreadable) folder(s) inside $($f.Name) (first: $(@($f.UnreadableFirst)[0])) - its size would be too low and those files would not be copied (RISKS R6)" }
    }
    $c = $H.CloudFiles
    if ("$($c.Result)" -eq 'refused' -or [int]$c.Failed -gt 0) { $r += "$($c.Failed) of $($c.PlaceholdersFound) OneDrive online-only file(s) could not be downloaded; copied from Linux they would arrive EMPTY (RISKS R8). Check that OneDrive is running and signed in, then run it again" }
    # online-only files are not a refusal (decided 2026-09-26, the owner's call,
    # RISKS R8): their bytes are in OneDrive, not on this disk, and settle-in
    # reconnects OneDrive instead of copying them - the job records how many
    # (cloud_files.result = left-in-cloud), and settle-in must never copy one
    elseif ("$($c.Result)" -eq 'materialized') {
        $again = [int](@($H.UserFolders | Where-Object { $_.Exists } | Measure-Object -Property CloudOnlyNow -Sum).Sum)
        if ($again -gt 0) { $r += "after downloading, $again OneDrive file(s) were online-only again - OneDrive freed them while the list was being made (RISKS R8)" }
    }
    elseif ("$($c.Result)" -notin @('none-found', 'not-attempted')) { $r += "the OneDrive check ended as '$($c.Result)', which this version does not understand" }
    if (-not $H.StickFit) { $r += "the stick's free space could not be read ($($H.Stick.Error))" }
    $r
}

function Get-JobStickFitRefusal {
    # Pure (self-tested). Clean slate stages the folders to the stick and then
    # wipes Windows: a job whose folders do not fit, or that would leave other
    # people's files behind (RISKS R5), is no job - with the gap, so the next
    # try can converge (architecture.md, "It refuses").
    param($H, [string]$Path)
    if ($Path -ne 'clean-slate') { return $null }
    $fit = $H.StickFit
    if (-not $fit.Fits) {
        $more = if ([long]$fit.GapBytes -gt 0) { ("; a stick with at least {0:N1} GB free would hold them" -f ([math]::Ceiling([long]$fit.NeededBytes / 1GB * 10) / 10)) } else { '' }
        return "Windows cannot be kept, and your folders do not fit on this stick: $($fit.Reason)$more"
    }
    $others = @($H.Owner.OtherProfiles)
    if ($others.Count -gt 0) { return "this computer has $($others.Count) other account(s) ($(@($others | ForEach-Object { $_.Path }) -join ', ')); a clean slate would delete their files, and this version copies only yours (RISKS R5)" }
    $null
}

function ConvertTo-JobHarvest {
    # Pure (self-tested): the harvester's folder map -> the job's harvest.folders,
    # harvest.cloud_files and harvest.stick_fit. Called only after
    # Get-JobHarvestRefusals found nothing.
    param($H)
    $folders = @(@($H.UserFolders) | ForEach-Object {
        [ordered]@{ name = "$($_.Name)"; path = $(if ($_.Path) { "$($_.Path)" } else { $null }); exists = [bool]$_.Exists; is_onedrive = [bool]$_.IsOneDrive
                    files = [int]$_.Files; bytes = [long]$_.Bytes; cloud_only_files = [int]$_.CloudOnlyFiles; truncated = $false } })
    $c = $H.CloudFiles
    $fit = $H.StickFit
    [ordered]@{
        folders = $folders
        cloud_files = [ordered]@{ placeholders_found = [int]$c.PlaceholdersFound; materialized = [int]$c.Materialized; failed = 0; result = $(if ("$($c.Result)" -eq 'not-attempted') { 'left-in-cloud' } else { "$($c.Result)" }) }
        stick_fit = [ordered]@{ filesystem = "$($fit.FileSystem)"; cluster_bytes = [long]$fit.ClusterBytes; free_bytes = [long]$fit.FreeBytes; files_bytes = [long]$fit.FilesBytes
                                needed_bytes = [long]$fit.NeededBytes; files_over_4gib = [int]$fit.FilesOver4GiB; fits = [bool]$fit.Fits; gap_bytes = [long]$fit.GapBytes }
    }
}

function New-JobDocument {
    param($F, [string]$Desktop, [string]$PasswordHash, [string]$IfCannotKeep, [string]$ReportRel, [string]$AcknowledgeDataLoss, [string]$EraseEverything, [string]$StartAt = 'desktop')
    $refusals = @()
    $erase = [bool]$EraseEverything
    $eraseDisks = @()
    if ($erase) {
        if ($EraseEverything -cne $EraseStatement) { $refusals += "the erase sentence was not typed exactly (expected: $EraseStatement)" }
        if (-not $PasswordHash -or $PasswordHash -eq $VerifyOnlyHash) { $refusals += 'no password was chosen for the new account' }
        elseif ($PasswordHash -cnotmatch '^\$6\$') { $refusals += 'the password hash is not SHA-512 crypt' }
        $ed = Get-JobEraseDisks -Disks $F.AllDisks -SystemNumber ([int]$F.Disk.Number) -StickUniqueId "$($F.Stick.UniqueId)"
        $refusals += @($ed.Refusals); $eraseDisks = @($ed.Disks)
    }
    $ack = Get-JobAcknowledgement -Verdict "$($F.Verdict)" -FailedChecks @($F.FailedChecks) -WarnChecks @($F.WarnChecks) -Typed $AcknowledgeDataLoss
    if ($ack.Refusal) { $refusals += $ack.Refusal }
    elseif ($F.Verdict -notin @('GREEN', 'YELLOW', 'RED')) { $refusals += "no scanner verdict found (got '$($F.Verdict)') - run the scanner with -Json first" }
    if ($F.Firmware -ne 'UEFI') { $refusals += "firmware is '$($F.Firmware)', not UEFI - the boot handoff does not apply" }
    if ($F.BitLocker -notin @('on', 'off')) { $refusals += 'BitLocker state on C: could not be determined' }
    $iana = ConvertTo-JobIanaTimeZone -WindowsId $F.WindowsTz
    if (-not $iana) { $refusals += "Windows time zone '$($F.WindowsTz)' has no IANA mapping in this version" }
    $c = $F.Clock
    $clock = ConvertTo-JobClock -WindowsZone "$($F.WindowsTz)" -Iana "$iana" -RealTimeIsUniversal $c.RealTimeIsUniversal -DynamicDstDisabled $c.DynamicDstDisabled -OffsetMinutes ([int]$c.OffsetMinutes) -BaseOffsetMinutes ([int]$c.BaseOffsetMinutes) -DstActive ([bool]$c.DstActive) -NowUtc "$($c.NowUtc)"
    if ($clock.Refusal) { $refusals += $clock.Refusal }
    $keymap = ConvertTo-JobKeymap -InputMethodTip $F.InputTip
    if (-not $keymap) { $refusals += "keyboard layout '$($F.InputTip)' has no mapping in this version" }
    if (-not $F.Stick) { $refusals += "the stick's identity could not be read ($($F.StickError))" }
    if ($F.Stick -and $F.Stick.Bus -ne 'USB') { $refusals += "the stick is on bus '$($F.Stick.Bus)', not USB" }
    if (-not $F.Disk.UniqueId) { $refusals += 'the system disk has no unique id' }
    # the folder map decides what is kept; an erase job keeps nothing, so it
    # only needs the map to exist (it lists what will be deleted)
    if ($erase) { if (-not $F.Harvest) { $refusals += "the list of your folders could not be read ($($F.HarvestError))" } }
    else { $refusals += @(Get-JobHarvestRefusals -H $F.Harvest -HarvestError "$($F.HarvestError)") }
    if ($refusals.Count -gt 0) { return @{ Refusals = $refusals; Job = $null } }

    $espFits = ($F.EspFree -ge 32MB)
    $diskAck = [bool]($ack.Block -and ($ack.Block.overrides -contains 'disk-health'))
    # A shrink number read while Windows has a full chkdsk queued is not a
    # measurement (the Aspire answered 0 GB with no error, 2026-09-17, R18):
    # record it as unmeasured with the reason, and let the prologue measure
    # after the check it will run.
    $shrinkGB = $F.ShrinkGB; $shrinkError = $F.ShrinkError
    if ($F.RepairQueued -and ($null -eq $shrinkGB -or $shrinkGB -lt $LinuxMinGB)) {
        $answer = if ($null -ne $shrinkGB) { "$shrinkGB GB" } else { "no number ($shrinkError)" }
        $shrinkError = "Windows answered $answer while a full disk check is queued ($($F.RepairQueuedWhy)) - not a trustworthy measurement; the prologue measures again after the check"
        $shrinkGB = $null
    }
    $mitigable = Test-JobShrinkMitigable -LastUnmovable "$($F.LastUnmovable)"
    $path = if ($erase) { @{ Path = 'clean-slate'; Reason = 'user-chose-fresh-start' } }
            else { Get-JobPath -DiskHealth $F.Health -EspFits $espFits -ShrinkableGB $shrinkGB -Dirty $F.Dirty -DiskHealthAcknowledged $diskAck -RepairQueued ([bool]$F.RepairQueued) -Mitigable $mitigable -IfCannotKeep $IfCannotKeep }
    if (-not $path.Path) {
        $why = if (-not $espFits) { "the EFI system partition has $([math]::Round([long]$F.EspFree/1MB,1)) MB free, too little for Linux's boot files beside Windows'" } else { "the system disk reports '$($F.Health)'" }
        return @{ Refusals = @("Windows cannot be kept on this machine ($why), and you chose to stop rather than wipe it - no job; nothing was changed"); Job = $null }
    }
    $fitRefusal = if ($erase) { $null } else { Get-JobStickFitRefusal -H $F.Harvest -Path $path.Path }
    if ($fitRefusal) { return @{ Refusals = @("$fitRefusal - no job; nothing was changed"); Job = $null } }
    $hv = ConvertTo-JobHarvest -H $F.Harvest
    $health = if ($F.Health -in @('Healthy', 'Warning', 'Unhealthy')) { $F.Health } else { 'Unknown' }
    $bl = $F.BitLocker
    $job = [ordered]@{
        schema = 'job/1'
        job_id = [guid]::NewGuid().ToString()
        created_utc = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
        evaluate = [ordered]@{ version = $JobWriterVersion; scanner_version = 'see report'; harvest_version = "$($F.Harvest.HarvestVersion)"; ran_as_admin = $true }
        identity = [ordered]@{
            vendor = $F.Vendor; model = $F.Model; system_uuid = $F.Uuid; bios_serial = $F.BiosSerial; bios_version = $F.BiosVersion
            firmware_mode = 'UEFI'; secure_boot = $F.SecureBoot; os_caption = $F.OsCaption; os_build = [int]$F.OsBuild
            system_disk = [ordered]@{ number = [int]$F.Disk.Number; serial_number = "$($F.Disk.Serial)"; unique_id = $F.Disk.UniqueId
                                      friendly_name = $F.Disk.Name; size_bytes = [long]$F.Disk.Size; partition_style = $F.Disk.Style }
        }
        scan = [ordered]@{ verdict = $F.Verdict; required_kernel = $(if ($F.RequiredKernel) { "$($F.RequiredKernel)" } else { $null }); report = $ReportRel }
        intent = [ordered]@{
            path = $path.Path; path_reason = $path.Reason; desktop = $Desktop; start_at = $StartAt
            distro = [ordered]@{ name = 'fedora'; release = '42' }
            account = [ordered]@{ windows_name = $F.UserName; full_name = $(if ($F.FullName) { "$($F.FullName)" } else { $null })
                                  linux_name = (ConvertTo-JobLinuxName $F.UserName); password_hash = $PasswordHash }
            locale = [ordered]@{ lang = (($F.Locale -replace '-', '_') + '.UTF-8'); timezone = $iana; keymap = $keymap }
        }
        # restore_points_consented (R18, decided 2026-09-20): the launcher's step-of-decision text says restore points go if they are in the way of the shrink and that this cannot be undone; CONVERT typed there is the consent
        # usn_journal_consented (R18, decided 2026-09-22): the same screen says NTFS's change journal goes if it is what stops the shrink - Windows' record of recent file changes, not a file; search and sync programs look through the files again
        fork = [ordered]@{ if_cannot_keep = $IfCannotKeep; volume_check_consented = $true; restore_points_consented = $true; usn_journal_consented = $true }
        storage = [ordered]@{
            shrinkable_gb = $shrinkGB; shrink_source = $(if ($null -ne $shrinkGB) { 'storage-api' } else { $null }); shrink_error = $shrinkError
            last_unmovable_file = $(if ($F.LastUnmovable) { "$($F.LastUnmovable)" } else { $null })
            linux_min_gb = $LinuxMinGB
            volume_health = [ordered]@{ dirty = $F.Dirty; repair_queued = [bool]$F.RepairQueued; scan = $null }
            physical_disk = [ordered]@{ health_status = $health; operational_status = "$($F.Operational)"; media_type = "$($F.MediaType)" }
            esp = [ordered]@{ size_bytes = [long]$F.EspSize; free_bytes = [long]$F.EspFree; fits_alongside_install = $espFits }
        }
        harvest = [ordered]@{
            clock = $clock.Clock
            windows_license = $(if ($F.License) { ConvertTo-JobLicenseFromFacts $F.License } else { ConvertTo-JobLicense -ReadError 'not read' -NowUtc (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ') })
            folders = $hv.folders
            cloud_files = $hv.cloud_files
            stick_fit = $hv.stick_fit
            browsers = @()
            # the Wi-Fi export runs in main, after every refusal above has had its say
            # (a refused job never leaves passwords on the stick); a verify-only job installs nothing and exports nothing
            wifi = [ordered]@{ result = 'not-harvested'; secrets_dir = $null; profiles = @() }
            bitlocker = [ordered]@{ status = $bl; recovery_key_file = $(if ($bl -eq 'on') { 'artifacts/credentials/bitlocker-C.txt' } else { $null }) }
            firmware_artifacts = @()
            software = $(if ($F.Software) { $F.Software } else { [ordered]@{ desktop = @(); store = @(); truncated = $false } })
        }
        stick = [ordered]@{ unique_id = $F.Stick.UniqueId; serial_number = "$($F.Stick.Serial)"; size_bytes = [long]$F.Stick.Size
                            friendly_name = $F.Stick.Name; label = $(if ($F.Stick.Label) { $F.Stick.Label } else { 'UPGV0' }); manifest = 'SHA256SUMS' }
    }
    if ($ack.Block) { $job.risk_acknowledgement = $ack.Block }
    if ($erase) { $job.erase_consent = [ordered]@{ statement = $EraseStatement; accepted_utc = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ'); disks = @($eraseDisks) } }
    elseif ($path.Path -eq 'clean-slate') { $job.staged = [ordered]@{ files = 0; bytes = 0; manifest = 'staging/SHA256SUMS' } }
    if ($path.Path -eq 'keep-windows' -and $F.Dirty -eq 'dirty') { $job.fork.volume_check_consented = $true }
    @{ Refusals = @(); Job = $job }
}

function ConvertTo-JobJson {
    # PS 5.1's ConvertTo-Json writes CRLF and escapes '<' etc.; the Linux
    # side reads it with python - LF, UTF-8, no BOM.
    param($Job)
    (($Job | ConvertTo-Json -Depth 8) -replace "`r`n", "`n")
}

# --- self-test ---------------------------------------------------------------------

function Invoke-SelfTest {
    # a folder map as the harvester's -FolderMapOut writes it (parsed JSON shape)
    function New-TestHarvest {
        param([string]$Version = '0.3.0', $Owners = @('S-1-5-21-9-1001'), [string]$Sid = 'S-1-5-21-9-1001', $Others = @(), [bool]$Truncated = $false, [int]$Unreadable = 0,
              [string]$CloudResult = 'none-found', [int]$Found = 0, [int]$Failed = 0, [int]$Again = 0, [bool]$Fits = $true, [long]$Free = 1833394176, [long]$Needed = 1500000000,
              [string]$FitReason = $null, [switch]$NoStick)
        $docs = [pscustomobject]@{ Name = 'Documents'; Path = 'C:\Users\a\OneDrive\Documents'; Exists = $true; IsOneDrive = $true; Files = 473; Bytes = 1400000000; CloudOnlyFiles = $Found; CloudOnlyNow = $Again
                                   Truncated = $Truncated; Unreadable = $Unreadable; UnreadableFirst = @($(if ($Unreadable) { 'C:\Users\a\OneDrive\Documents\locked' })) }
        $music = [pscustomobject]@{ Name = 'Music'; Path = 'C:\Users\a\Music'; Exists = $false; IsOneDrive = $false; Files = 0; Bytes = 0; CloudOnlyFiles = 0; CloudOnlyNow = 0; Truncated = $false; Unreadable = 0; UnreadableFirst = @() }
        $fit = [pscustomobject]@{ FileSystem = 'FAT32'; ClusterBytes = 4096; FreeBytes = $Free; FilesBytes = 1400000000; NeededBytes = $Needed; FilesOver4GiB = 0; Fits = $Fits
                                  GapBytes = [long]([math]::Max([long]0, [long]($Needed - $Free))); Reason = $FitReason }
        [pscustomobject]@{ HarvestVersion = $Version
                           Owner = [pscustomobject]@{ ProcessSid = $Sid; ProcessName = 'PC\a'; SessionId = 1; DesktopOwnerSids = @($Owners); OtherProfiles = @($Others) }
                           UserFolders = @($docs, $music)
                           CloudFiles = [pscustomobject]@{ PlaceholdersFound = $Found; Materialized = $(if ($CloudResult -eq 'materialized') { $Found - $Failed } else { 0 }); Failed = $Failed; Result = $CloudResult }
                           Stick = [pscustomobject]@{ Error = 'the drive was removed' }; StickFit = $(if ($NoStick) { $null } else { $fit }) }
    }
    $good = @{ Vendor = 'Acer'; Model = 'Aspire'; Uuid = 'u'; BiosSerial = 's'; BiosVersion = 'v'; OsCaption = 'Windows 10'; OsBuild = 19045
               Firmware = 'UEFI'; SecureBoot = 'on'; Disk = @{ Number = 0; Serial = 'S1'; UniqueId = 'eui.1'; Name = 'SSD'; Size = 250059350016; Style = 'GPT' }
               Health = 'Healthy'; Operational = 'OK'; MediaType = 'SSD'; ShrinkGB = 61.4; ShrinkError = $null; Dirty = 'clean'
               EspSize = 104857600; EspFree = 72219648; BitLocker = 'on'; Verdict = 'YELLOW'; RequiredKernel = '6.7'; Report = 'x'
               Stick = @{ UniqueId = 'USBSTOR\X'; Serial = ''; Size = 8053063680; Name = 'General UDisk'; Label = 'UPGV0'; Bus = 'USB' }
               WindowsTz = 'Eastern Standard Time'; Locale = 'en-US'; InputTip = '0409:00000409'; UserName = 'Addison'; FullName = 'Addison Example'
               FailedChecks = @(); WarnChecks = @(); RepairQueued = $false; RepairQueuedWhy = ''; RepairStale = ''; LastUnmovable = $null
               Harvest = (New-TestHarvest); HarvestError = $null
               Clock = @{ WindowsZone = 'Eastern Standard Time'; RealTimeIsUniversal = $null; DynamicDstDisabled = $null; OffsetMinutes = -240; BaseOffsetMinutes = -300; DstActive = $true; NowUtc = '2026-09-27T12:00:00Z' }
               License = @{ Os = @{ ProductName = 'Windows 10 Home'; EditionId = 'Core'; DisplayVersion = '24H2'; Build = '26100' }
                            Products = @(@{ LicenseStatus = 1; Channel = 'OEM:DM'; Addon = $false }); Firmware = @{ Present = $true; Description = '[4.0] Core OEM:DM' }; Error = $null; NowUtc = '2026-09-27T12:00:00Z' }
               AllDisks = @(@{ Number = 0; Serial = 'S1'; UniqueId = 'eui.1'; Size = 250059350016; Name = 'SSD'; Bus = 'NVMe'; Health = 'Healthy' },
                            @{ Number = 2; Serial = ''; UniqueId = 'USBSTOR\X'; Size = 8053063680; Name = 'General UDisk'; Bus = 'USB'; Health = 'Healthy' }) }
    function With { param($h, [string]$k, $v) $c = @{}; foreach ($e in $h.GetEnumerator()) { $c[$e.Key] = $e.Value }; $c[$k] = $v; $c }
    $ph = '$6$upgradeV1$MkYfbaBe.FFp2fzSNrPiJ6RdPagcfI.crkepTcQpGsjGFMe8780OtkedouSyxvXdky5a6WiTWDy/.epwkWUk71'
    $hp = '$6$abcdefghijklmnop$Z142AM4CbyHnvFikRKauX.vgsnvjYLvt4bZlZZZrlgVhDW0zltnUun6G9I5xvVirZ/Y9MRz96lJh5eoUicidR.'
    $es = 'I confirm that everything on this computer will be deleted and nothing will be kept'
    $aspire = With (With (With (With $good 'ShrinkGB' 0.0) 'Dirty' 'clean') 'RepairQueued' $true) 'RepairQueuedWhy' "Get-Volume reports 'Full Repair Needed'; NTFS logged on 2026-09-13T15:16:48Z that C: needs a full chkdsk"
    $hiber = With (With (With $good 'ShrinkGB' 0.0) 'Dirty' 'clean') 'LastUnmovable' '\hiberfil.sys'
    # a Windows Wi-Fi profile as the Native Wifi API returns it (the G16's shape, 2026-09-27; names made up)
    function New-TestWlan {
        param([string]$Name = 'Home Net', [string]$Auth = 'WPA2PSK', [string]$Enc = 'AES', [string]$Key = 'correct horse', [string]$Protected = 'false',
              [string]$Mode = 'auto', [string]$OneX = 'false', [bool]$Transition = $false, [bool]$Hidden = $false, [bool]$NoHex = $false, [string]$Type = 'ESS')
        $hex = -join ([Text.Encoding]::UTF8.GetBytes($Name) | ForEach-Object { $_.ToString('X2') })
        $sk = if ($Key) { "<sharedKey><keyType>passPhrase</keyType><protected>$Protected</protected><keyMaterial>$Key</keyMaterial></sharedKey>" } else { '' }
        $tm = if ($Transition) { '<transitionMode xmlns="http://www.microsoft.com/networking/WLAN/profile/v4">true</transitionMode>' } else { '' }
        '<?xml version="1.0"?><WLANProfile xmlns="http://www.microsoft.com/networking/WLAN/profile/v1"><name>' + $Name + '</name><SSIDConfig><SSID>' + $(if (-not $NoHex) { "<hex>$hex</hex>" }) + '<name>' + $Name + '</name></SSID>' + $(if ($Hidden) { '<nonBroadcast>true</nonBroadcast>' }) + '</SSIDConfig>' +
        "<connectionType>$Type</connectionType><connectionMode>$Mode</connectionMode><MSM><security><authEncryption><authentication>$Auth</authentication><encryption>$Enc</encryption><useOneX>$OneX</useOneX>$tm</authEncryption>$sk</security></MSM></WLANProfile>"
    }
    function Api { param($Xmls, [bool]$Present = $true, [string]$Err = $null) @{ Present = $Present; Error = $Err; Profiles = @($Xmls | ForEach-Object { [pscustomobject]@{ Xml = $_ } }) } }
    function Row { param([string]$Xml) $r = ConvertFrom-JobWlanProfile -Xml $Xml; "$($r.key_mgmt):$($r.supported):$($r.autoconnect):$($r.hidden):$($r.why_not)" }
    $cases = @(
        @{ Name = 'clock: RealTimeIsUniversal absent = the hardware clock holds local time (Windows default)'
           Run = { $c = (ConvertTo-JobClock -WindowsZone 'Eastern Standard Time' -Iana 'America/New_York' -RealTimeIsUniversal $null -DynamicDstDisabled $null -OffsetMinutes -240 -BaseOffsetMinutes -300 -DstActive $true -NowUtc 'x').Clock; "$($c.rtc_is_local):$($null -eq $c.real_time_is_universal):$($c.dst_auto_adjust):$($c.utc_offset_minutes)" }; Expect = 'True:True:True:-240' }
        @{ Name = 'clock: RealTimeIsUniversal 1 = UTC, 0 = local'
           Run = { $a = (ConvertTo-JobClock -RealTimeIsUniversal 1 -OffsetMinutes 0 -BaseOffsetMinutes 0 -DstActive $false).Clock; $b = (ConvertTo-JobClock -RealTimeIsUniversal 0 -OffsetMinutes 0 -BaseOffsetMinutes 0 -DstActive $false).Clock; "$($a.rtc_is_local):$($a.real_time_is_universal):$($b.rtc_is_local):$($b.real_time_is_universal)" }; Expect = 'False:1:True:0' }
        @{ Name = 'clock: refuse a RealTimeIsUniversal value that is neither 0 nor 1 (not guessed)'
           Run = { [bool]((ConvertTo-JobClock -RealTimeIsUniversal 7 -OffsetMinutes 0 -BaseOffsetMinutes 0 -DstActive $false).Refusal -match "'7'") }; Expect = $true }
        @{ Name = 'clock: "adjust for daylight saving automatically" turned off is recorded'
           Run = { (ConvertTo-JobClock -RealTimeIsUniversal $null -DynamicDstDisabled 1 -OffsetMinutes -300 -BaseOffsetMinutes -300 -DstActive $false).Clock.dst_auto_adjust }; Expect = $false }
        @{ Name = 'clock: the job carries harvest.clock with the IANA name'
           Run = { $c = (New-JobDocument -F $good -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Job.harvest.clock; "$($c.windows_zone):$($c.iana):$($c.rtc_is_local):$($c.base_utc_offset_minutes):$($c.dst_active)" }; Expect = 'Eastern Standard Time:America/New_York:True:-300:True' }
        @{ Name = 'clock: a bad RealTimeIsUniversal refuses the job'
           Run = { $c = $good.Clock.Clone(); $c.RealTimeIsUniversal = 9; [bool]((New-JobDocument -F (With $good 'Clock' $c) -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Refusals -match 'RealTimeIsUniversal') }; Expect = $true }
        @{ Name = 'wifi: WPA2-personal with its password -> wpa-psk, joined automatically'
           Run = { Row (New-TestWlan) }; Expect = 'wpa-psk:True:True:False:' }
        @{ Name = 'wifi: WPA3 in transition mode -> wpa-psk (the router also takes WPA2); WPA3 only -> sae'
           Run = { "$(Row (New-TestWlan -Auth WPA3SAE -Transition $true))|$(Row (New-TestWlan -Auth WPA3SAE))" }; Expect = 'wpa-psk:True:True:False:|sae:True:True:False:' }
        @{ Name = 'wifi: an open network needs no password; manual connect and hidden are kept'
           Run = { Row (New-TestWlan -Auth open -Enc none -Key '' -Mode manual -Hidden $true) }; Expect = 'none:True:False:True:' }
        @{ Name = 'wifi: enterprise is listed, not set up'
           Run = { Row (New-TestWlan -Auth WPA2 -OneX true -Key '') }; Expect = 'UNSUPPORTED:False:True:False:an enterprise network (a company or school sign-in)' }
        @{ Name = 'wifi: WEP and ad-hoc are listed, not set up'
           Run = { "$(Row (New-TestWlan -Auth open -Enc WEP))|$(Row (New-TestWlan -Type IBSS))" }; Expect = 'UNSUPPORTED:False:True:False:WEP, an old and broken kind of Wi-Fi security|UNSUPPORTED:False:True:False:an ad-hoc (computer-to-computer) network' }
        @{ Name = 'wifi: a password Windows kept encrypted (protected) is not a password - listed, not set up'
           Run = { Row (New-TestWlan -Protected true) }; Expect = 'UNSUPPORTED:False:True:False:its password could not be read from Windows' }
        @{ Name = 'wifi: an XML that is not a network profile is skipped'
           Run = { $null -eq (ConvertFrom-JobWlanProfile -Xml '<?xml version="1.0"?><Other/>') }; Expect = $true }
        @{ Name = 'wifi: the SSID bytes are computed when Windows gives no hex'
           Run = { (ConvertFrom-JobWlanProfile -Xml (New-TestWlan -Name 'Caf&#233;' -NoHex $true)).ssid_hex }; Expect = '436166C3A9' }
        @{ Name = 'wifi: the row carries no password, only the file it is in'
           Run = { $w = ConvertTo-JobWifi -Api (Api @((New-TestWlan))) -StoredCount 1; $j = $w.Wifi | ConvertTo-Json -Depth 5; "$($j -match 'correct horse'):$($j -match 'HasKey'):$($w.Wifi.profiles[0].secrets_file):$($w.Files[0].Xml -match 'correct horse')" }; Expect = 'False:False:artifacts/credentials/wifi/01.xml:True' }
        @{ Name = 'wifi: only networks with a password get a file; the result is exported'
           Run = { $w = ConvertTo-JobWifi -Api (Api @((New-TestWlan -Name A), (New-TestWlan -Name B -Auth open -Enc none -Key ''), (New-TestWlan -Name C -Auth WPA2 -OneX true -Key ''), (New-TestWlan -Name D -Auth WPA3SAE))) -StoredCount 4
                   "$($w.Wifi.result):$($w.Wifi.secrets_dir):$($w.Files.Count):$(@($w.Wifi.profiles | ForEach-Object { if ($_.secrets_file) { Split-Path $_.secrets_file -Leaf } else { '-' } }) -join ',')" }; Expect = 'exported:artifacts/credentials/wifi:2:01.xml,-,-,02.xml' }
        @{ Name = 'wifi: refuse when Windows stores more networks than could be read (one would go missing)'
           Run = { [bool]((ConvertTo-JobWifi -Api (Api @((New-TestWlan))) -StoredCount 2).Refusal -match '2 saved Wi-Fi network\(s\) on disk but 1') }; Expect = $true }
        @{ Name = 'wifi: the same network saved on two adapters is set up once'
           Run = { $w = ConvertTo-JobWifi -Api (Api @((New-TestWlan), (New-TestWlan))) -StoredCount 2; "$($w.Wifi.profiles.Count):$($w.Files.Count)" }; Expect = '1:1' }
        @{ Name = 'wifi: no Wi-Fi service and nothing stored = no-wireless; stored but unreadable = refusal; an API error = refusal'
           Run = { $a = (ConvertTo-JobWifi -Api (Api @() -Present $false) -StoredCount 0).Wifi.result; $b = [bool](ConvertTo-JobWifi -Api (Api @() -Present $false) -StoredCount 3).Refusal; $c = [bool](ConvertTo-JobWifi -Api (Api @() -Err 'error 5') -StoredCount 0).Refusal; "$($a):$($b):$($c)" }; Expect = 'no-wireless:True:True' }
        @{ Name = 'wifi: a Wi-Fi adapter with nothing saved = none-saved, no directory'
           Run = { $w = (ConvertTo-JobWifi -Api (Api @()) -StoredCount 0).Wifi; "$($w.result):$($null -eq $w.secrets_dir)" }; Expect = 'none-saved:True' }
        @{ Name = 'wifi: the job writes not-harvested until main exports (a verify-only job never exports)'
           Run = { (New-JobDocument -F $good -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Job.harvest.wifi.result }; Expect = 'not-harvested' }
        # R18, 2026-09-20: what the read-only diagnostic found on the Aspire
        @{ Name = 'mitigable (R18): 0 GB with hiberfil.sys named as the last unmovable file is a keep-windows job, the cold number kept as measured'
           Run = { $j = (New-JobDocument -F $hiber -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Job; "$($j.intent.path):$($j.intent.path_reason):$($j.storage.shrinkable_gb):$($j.storage.shrink_source):$($j.storage.last_unmovable_file):$($j.storage.volume_health.repair_queued)" }; Expect = 'keep-windows:default:0:storage-api:\hiberfil.sys:False' }
        @{ Name = 'mitigable (R18): 0 GB pinned by anything else, fork clean-slate, is no room (clean slate, forced - the person allowed it)'
           Run = { $j = (New-JobDocument -F (With $hiber 'LastUnmovable' '\$Mft::$DATA') -Desktop kde -PasswordHash $ph -IfCannotKeep clean-slate -ReportRel 'r').Job; "$($j.intent.path):$($j.intent.path_reason):$($j.storage.last_unmovable_file)" }; Expect = 'clean-slate:forced-no-room:\$Mft::$DATA' }
        @{ Name = 'fork stop (R18, 2026-09-22): 0 GB pinned by anything else is keep-windows - the prologue re-measures and stops; never a wipe the person declined'
           Run = { $j = (New-JobDocument -F (With $hiber 'LastUnmovable' '\$Mft::$DATA') -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Job; "$($j.intent.path):$($j.intent.path_reason):$($j.fork.if_cannot_keep):$($null -eq $j.staged)" }; Expect = 'keep-windows:default:stop:True' }
        @{ Name = 'fork stop (R18, 2026-09-22): the Aspire run 5 as scanned - RED on Disk health acknowledged, 3.2 GB pinned by $UsnJrnl - is keep-windows, no staged block'
           Run = { $f = With (With (With (With (With $good 'Verdict' 'RED') 'FailedChecks' @('Disk health')) 'ShrinkGB' 3.2) 'Dirty' 'clean') 'LastUnmovable' '\$Extend\$UsnJrnl:$J:$DATA'; $r = New-JobDocument -F $f -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r' -AcknowledgeDataLoss 'I confirm that I understand the risks and could lose data'; "$($r.Refusals.Count):$($r.Job.intent.path):$($r.Job.intent.path_reason):$($r.Job.storage.shrinkable_gb):$($null -eq $r.Job.staged)" }; Expect = '0:keep-windows:default:3.2:True' }
        @{ Name = 'mitigable (R18): it never buys a path the disk gate or the ESP refuses'
           Run = { "$((New-JobDocument -F (With $hiber 'Health' 'Warning') -Desktop kde -PasswordHash $ph -IfCannotKeep clean-slate -ReportRel 'r').Job.intent.path)/$((New-JobDocument -F (With $hiber 'EspFree' 1000000) -Desktop kde -PasswordHash $ph -IfCannotKeep clean-slate -ReportRel 'r').Job.intent.path)" }; Expect = 'clean-slate/clean-slate' }
        @{ Name = 'mitigable (R18): hiberfil, pagefile and swapfile only'
           Run = { "$(Test-JobShrinkMitigable '\hiberfil.sys')/$(Test-JobShrinkMitigable '\pagefile.sys')/$(Test-JobShrinkMitigable '\swapfile.sys')/$(Test-JobShrinkMitigable '\$BadClus:$Bad')/$(Test-JobShrinkMitigable '\Users\a\pagefile.sys')/$(Test-JobShrinkMitigable '')" }; Expect = 'True/True/True/False/False/False' }
        @{ Name = 'defrag 259: the Aspire''s real text parses to \hiberfil.sys'
           Run = { ConvertFrom-JobDefrag259 -Message "A volume shrink analysis was initiated on volume Acer (C:).`n Diagnostic details:`n - The last unmovable file appears to be: \hiberfil.sys::`$DATA`n - The last cluster of the file is: 0x3b562fe" }; Expect = '\hiberfil.sys' }
        @{ Name = 'defrag 259: text without the line parses to null'; Run = { $null -eq (ConvertFrom-JobDefrag259 -Message 'The storage optimizer successfully completed shrink estimation on Acer (C:)') }; Expect = $true }
        @{ Name = 'repair queued (R18, 2026-09-20): an event 98 older than the last completed check is history, not a queued repair'
           Run = { $t = Test-JobRepairQueued -VolumeStatus 'OK' -NtfsFullChkdsk ([DateTime]'2026-09-13T15:16:48') -LastCheck ([DateTime]'2026-09-15T18:02:32'); "$($t.Queued):$([bool]$t.Stale)" }; Expect = 'False:True' }
        @{ Name = 'repair queued (R18, 2026-09-20): an event 98 newer than the last completed check still counts'
           Run = { (Test-JobRepairQueued -VolumeStatus 'OK' -NtfsFullChkdsk ([DateTime]'2026-09-16T09:00:00') -LastCheck ([DateTime]'2026-09-15T18:02:32')).Queued }; Expect = $true }
        @{ Name = 'repair queued (R18, 2026-09-20): the volume''s own status counts whatever the history says'
           Run = { (Test-JobRepairQueued -VolumeStatus 'Full Repair Needed' -NtfsFullChkdsk ([DateTime]'2026-09-13T15:16:48') -LastCheck ([DateTime]'2026-09-15T18:02:32')).Queued }; Expect = $true }
        # R18, 2026-09-17: a clean dirty bit with a full chkdsk queued is the Aspire's state; the Storage API's 0 GB is not a measurement
        @{ Name = 'repair queued (R18): 0 GB with a clean bit and a queued full chkdsk is a keep-windows job, shrinkable unmeasured'
           Run = { $r = New-JobDocument -F $aspire -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r'; "$($r.Job.intent.path):$($r.Job.intent.path_reason):$($null -eq $r.Job.storage.shrinkable_gb):$($r.Job.storage.volume_health.dirty):$($r.Job.storage.volume_health.repair_queued)" }; Expect = 'keep-windows:default:True:clean:True' }
        @{ Name = 'repair queued (R18): the untrusted number and its reason are carried in shrink_error'
           Run = { $j = (New-JobDocument -F $aspire -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Job; [bool]($j.storage.shrink_error -match '^Windows answered 0 GB while a full disk check is queued \(Get-Volume reports') -and ($null -eq $j.storage.shrink_source) }; Expect = $true }
        @{ Name = 'repair queued (R18): 0 GB with a clean bit and NO repair queued, fork clean-slate, is no room (clean slate, forced)'
           Run = { $j = (New-JobDocument -F (With (With $good 'ShrinkGB' 0.0) 'Dirty' 'clean') -Desktop kde -PasswordHash $ph -IfCannotKeep clean-slate -ReportRel 'r').Job; "$($j.intent.path):$($j.intent.path_reason):$($j.storage.shrinkable_gb):$($j.storage.volume_health.repair_queued)" }; Expect = 'clean-slate:forced-no-room:0:False' }
        @{ Name = 'repair queued (R18): a measured number that already fits is kept as measured'
           Run = { $j = (New-JobDocument -F (With $good 'RepairQueued' $true) -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Job; "$($j.intent.path):$($j.storage.shrinkable_gb):$($j.storage.shrink_source)" }; Expect = 'keep-windows:61.4:storage-api' }
        @{ Name = 'repair queued (R18): the Aspire as scanned - RED on Disk health, acknowledged, healthy status, 0 GB, repair queued - is a keep-windows job'
           Run = { $f = With (With (With $aspire 'Verdict' 'RED') 'FailedChecks' @('Disk health')) 'WarnChecks' @('Volume health'); $r = New-JobDocument -F $f -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r' -AcknowledgeDataLoss 'I confirm that I understand the risks and could lose data'; "$($r.Refusals.Count):$($r.Job.intent.path):$($r.Job.risk_acknowledgement.overrides -join ',')" }; Expect = '0:keep-windows:disk-health,volume-health' }
        @{ Name = 'repair queued: Get-Volume Full Repair Needed counts'; Run = { $t = Test-JobRepairQueued -VolumeStatus 'Full Repair Needed' -NtfsFullChkdsk $null; "$($t.Queued):$($t.Why)" }; Expect = "True:Get-Volume reports 'Full Repair Needed'" }
        @{ Name = 'repair queued: NTFS event 98 counts on its own'; Run = { (Test-JobRepairQueued -VolumeStatus 'OK' -NtfsFullChkdsk '2026-09-13T15:16:48Z').Queued }; Expect = $true }
        @{ Name = 'repair queued: OK and no event is not queued'; Run = { $t = Test-JobRepairQueued -VolumeStatus 'OK' -NtfsFullChkdsk $null; "$($t.Queued):[$($t.Why)]" }; Expect = 'False:[]' }
        @{ Name = 'consents (R18): every job carries the launcher''s restore-point and change-journal consent'
           Run = { $f = (New-JobDocument -F $good -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Job.fork; "$($f.restore_points_consented):$($f.usn_journal_consented)" }; Expect = 'True:True' }
        @{ Name = 'a healthy machine with room gets a keep-windows job'
           Run = { $r = New-JobDocument -F $good -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r.txt'; "$($r.Refusals.Count):$($r.Job.intent.path):$($r.Job.intent.path_reason)" }; Expect = '0:keep-windows:default' }
        @{ Name = 'too little shrink room, fork clean-slate, forces clean slate, with staged block'
           Run = { $r = New-JobDocument -F (With $good 'ShrinkGB' 10.0) -Desktop kde -PasswordHash $ph -IfCannotKeep clean-slate -ReportRel 'r'; "$($r.Job.intent.path):$($r.Job.intent.path_reason):$([bool]$r.Job.staged)" }; Expect = 'clean-slate:forced-no-room:True' }
        @{ Name = 'too little shrink room, fork stop, is keep-windows with no staged block (R18, 2026-09-22)'
           Run = { $r = New-JobDocument -F (With $good 'ShrinkGB' 10.0) -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r'; "$($r.Job.intent.path):$($r.Job.intent.path_reason):$([bool]$r.Job.staged)" }; Expect = 'keep-windows:default:False' }
        @{ Name = 'an unmeasured shrink (null) on a clean volume: clean slate under fork clean-slate, keep-windows under fork stop'
           Run = { "$((New-JobDocument -F (With $good 'ShrinkGB' $null) -Desktop kde -PasswordHash $ph -IfCannotKeep clean-slate -ReportRel 'r').Job.intent.path)/$((New-JobDocument -F (With $good 'ShrinkGB' $null) -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Job.intent.path)" }; Expect = 'clean-slate/keep-windows' }
        @{ Name = 'an unmeasured shrink on a FLAGGED volume, Healthy disk, is a keep-windows job with the fork pending (R18)'
           Run = { $r = New-JobDocument -F (With (With $good 'ShrinkGB' $null) 'Dirty' 'dirty') -Desktop kde -PasswordHash $ph -IfCannotKeep 'clean-slate' -ReportRel 'r'; "$($r.Job.intent.path):$($r.Job.fork.if_cannot_keep):$($r.Job.fork.volume_check_consented):$($r.Job.storage.volume_health.dirty):$($null -eq $r.Job.storage.shrinkable_gb)" }; Expect = 'keep-windows:clean-slate:True:dirty:True' }
        @{ Name = 'a flagged volume on a Warning disk still forces clean slate (fork clean-slate)'
           Run = { (New-JobDocument -F (With (With (With $good 'ShrinkGB' $null) 'Dirty' 'dirty') 'Health' 'Warning') -Desktop kde -PasswordHash $ph -IfCannotKeep clean-slate -ReportRel 'r').Job.intent.path }; Expect = 'clean-slate' }
        @{ Name = 'a Warning disk forces clean slate (fork clean-slate)'
           Run = { (New-JobDocument -F (With $good 'Health' 'Warning') -Desktop kde -PasswordHash $ph -IfCannotKeep clean-slate -ReportRel 'r').Job.intent.path }; Expect = 'clean-slate' }
        @{ Name = 'a full ESP forces clean slate (fork clean-slate)'
           Run = { (New-JobDocument -F (With $good 'EspFree' 1000000) -Desktop kde -PasswordHash $ph -IfCannotKeep clean-slate -ReportRel 'r').Job.intent.path }; Expect = 'clean-slate' }
        @{ Name = 'refuse (R18, 2026-09-22): a Warning disk under fork stop is no job, never a wipe - named'
           Run = { $r = New-JobDocument -F (With $good 'Health' 'Warning') -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r'; "$($null -eq $r.Job):$([bool]($r.Refusals -match '^Windows cannot be kept on this machine \(the system disk reports .Warning.\), and you chose to stop rather than wipe it'))" }; Expect = 'True:True' }
        @{ Name = 'refuse (R18, 2026-09-22): a full ESP under fork stop is no job, never a wipe - named'
           Run = { $r = New-JobDocument -F (With $good 'EspFree' 1000000) -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r'; "$($null -eq $r.Job):$([bool]($r.Refusals -match 'EFI system partition has 1 MB free'))" }; Expect = 'True:True' }
        @{ Name = 'fork stop never yields a clean-slate job, over every disk state (R18, 2026-09-22)'
           Run = { $bad = 0; foreach ($h in 'Healthy', 'Warning', 'Unknown') { foreach ($e in 1000000, 50000000) { foreach ($g in $null, 0.0, 3.2, 24.9, 61.4) { foreach ($d in 'clean', 'dirty') { foreach ($l in $null, '\hiberfil.sys', '\$Extend\$UsnJrnl:$J:$DATA') {
                   $r = New-JobDocument -F (With (With (With (With (With $good 'Health' $h) 'EspFree' $e) 'ShrinkGB' $g) 'Dirty' $d) 'LastUnmovable' $l) -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r'
                   if ($r.Job -and $r.Job.intent.path -ne 'keep-windows') { $bad++ } } } } } }; $bad }; Expect = 0 }
        @{ Name = 'refuse: RED verdict'
           Run = { (New-JobDocument -F (With $good 'Verdict' 'RED') -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Refusals -join ';' }; Expect = 'the scanner verdict is RED - no job, no override' }
        @{ Name = 'R23: RED for Disk health with the statement typed verbatim is a job carrying the acknowledgement (RED kept, overrides named)'
           Run = { $f = With (With (With $good 'Verdict' 'RED') 'FailedChecks' @('Disk health')) 'WarnChecks' @('Volume health'); $r = New-JobDocument -F $f -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r' -AcknowledgeDataLoss 'I confirm that I understand the risks and could lose data'
                   "$($r.Refusals.Count):$($r.Job.scan.verdict):$($r.Job.risk_acknowledgement.overrides -join '+'):$($r.Job.risk_acknowledgement.statement -ceq $RiskStatement)" }; Expect = '0:RED:disk-health+volume-health:True' }
        @{ Name = 'R23: a paraphrased statement lifts nothing'
           Run = { [bool]((New-JobDocument -F (With (With $good 'Verdict' 'RED') 'FailedChecks' @('Disk health')) -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r' -AcknowledgeDataLoss 'I understand the risks and could lose data').Refusals -match 'not typed exactly') }; Expect = $true }
        @{ Name = 'R23: the statement never lifts a RED from another check (CPU architecture, VMD)'
           Run = { [bool]((New-JobDocument -F (With (With $good 'Verdict' 'RED') 'FailedChecks' @('Disk health', 'Storage controller mode')) -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r' -AcknowledgeDataLoss 'I confirm that I understand the risks and could lose data').Refusals -match 'no acknowledgement lifts: Storage controller mode') }; Expect = $true }
        @{ Name = 'R23: the statement typed on a YELLOW machine adds no acknowledgement block'
           Run = { $null -eq (New-JobDocument -F $good -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r' -AcknowledgeDataLoss 'I confirm that I understand the risks and could lose data').Job.risk_acknowledgement }; Expect = $true }
        @{ Name = 'R23: with disk-health acknowledged, keep-windows is offered on a Warning disk that has room'
           Run = { $f = With (With (With $good 'Verdict' 'RED') 'FailedChecks' @('Disk health')) 'Health' 'Warning'; (New-JobDocument -F $f -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r' -AcknowledgeDataLoss 'I confirm that I understand the risks and could lose data').Job.intent.path }; Expect = 'keep-windows' }
        @{ Name = 'refuse: no verdict'
           Run = { [bool](New-JobDocument -F (With $good 'Verdict' $null) -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Refusals }; Expect = $true }
        @{ Name = 'refuse: legacy BIOS'
           Run = { [bool]((New-JobDocument -F (With $good 'Firmware' 'Legacy') -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Refusals -match 'UEFI') }; Expect = $true }
        @{ Name = 'refuse: unknown BitLocker state'
           Run = { [bool]((New-JobDocument -F (With $good 'BitLocker' 'unknown') -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Refusals -match 'BitLocker') }; Expect = $true }
        @{ Name = 'refuse: unmapped time zone, not a guess'
           Run = { [bool]((New-JobDocument -F (With $good 'WindowsTz' 'Nepal Standard Time') -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Refusals -match 'time zone') }; Expect = $true }
        @{ Name = 'refuse: a stick that is not on USB'
           Run = { [bool]((New-JobDocument -F (With $good 'Stick' @{ UniqueId = 'x'; Serial = ''; Size = 1; Name = 'n'; Label = 'L'; Bus = 'SAS' }) -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Refusals -match 'USB') }; Expect = $true }
        @{ Name = 'BitLocker on names the key file; off names none'
           Run = { $a = (New-JobDocument -F $good -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Job.harvest.bitlocker.recovery_key_file; $b = (New-JobDocument -F (With $good 'BitLocker' 'off') -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Job.harvest.bitlocker.recovery_key_file; "$a|$($null -eq $b)" }; Expect = 'artifacts/credentials/bitlocker-C.txt|True' }
        @{ Name = 'software: updates, hidden system components and nameless entries are dropped; duplicates merged; sorted'
           Run = { $sw = ConvertTo-JobSoftware -Desktop @(
                     [pscustomobject]@{ DisplayName = 'VLC media player'; DisplayVersion = '3.0.21'; Publisher = 'VideoLAN'; SystemComponent = $null },
                     [pscustomobject]@{ DisplayName = 'Security Update for Microsoft Office (KB5002544)'; DisplayVersion = '1'; Publisher = 'Microsoft'; SystemComponent = $null },
                     [pscustomobject]@{ DisplayName = 'Microsoft Visual C++ 2015 Redistributable'; DisplayVersion = '14'; Publisher = 'Microsoft'; SystemComponent = 1 },
                     [pscustomobject]@{ DisplayName = ''; DisplayVersion = '1'; Publisher = 'x'; SystemComponent = $null },
                     [pscustomobject]@{ DisplayName = 'Adobe Photoshop'; DisplayVersion = '25'; Publisher = 'Adobe'; SystemComponent = $null },
                     [pscustomobject]@{ DisplayName = 'VLC media player'; DisplayVersion = '3.0.21'; Publisher = 'VideoLAN'; SystemComponent = $null }) -Store @()
                   "$($sw.desktop.Count):$($sw.desktop[0].name):$($sw.desktop[1].name):$($sw.truncated)" }; Expect = '2:Adobe Photoshop:VLC media player:False' }
        @{ Name = 'software: Store frameworks and system packages are dropped, ms-resource names fall back to the package name'
           Run = { $sw = ConvertTo-JobSoftware -Desktop @() -Store @(
                     [pscustomobject]@{ Name = 'SpotifyAB.SpotifyMusic'; DisplayName = 'Spotify'; Version = '1.2'; PublisherDisplayName = 'Spotify AB'; IsFramework = $false; SignatureKind = 'Store'; NonRemovable = $false },
                     [pscustomobject]@{ Name = 'Microsoft.VCLibs.140.00'; DisplayName = 'VCLibs'; Version = '14'; PublisherDisplayName = 'Microsoft'; IsFramework = $true; SignatureKind = 'Store'; NonRemovable = $false },
                     [pscustomobject]@{ Name = 'Microsoft.Windows.ShellExperienceHost'; DisplayName = 'Shell'; Version = '10'; PublisherDisplayName = 'Microsoft'; IsFramework = $false; SignatureKind = 'System'; NonRemovable = $true },
                     [pscustomobject]@{ Name = 'Contoso.Thing'; DisplayName = 'ms-resource:AppName'; Version = '2'; PublisherDisplayName = 'Contoso'; IsFramework = $false; SignatureKind = 'Store'; NonRemovable = $false })
                   "$($sw.store.Count):$($sw.store[0].name):$($sw.store[1].name):$($sw.store[1].package)" }; Expect = '2:Contoso.Thing:Spotify:SpotifyAB.SpotifyMusic' }
        @{ Name = 'software: a list over the cap is cut and truncated=true'
           Run = { $many = @(1..5 | ForEach-Object { [pscustomobject]@{ DisplayName = "App $_"; DisplayVersion = $null; Publisher = $null; SystemComponent = $null } }); $sw = ConvertTo-JobSoftware -Desktop $many -Store @() -Cap 3; "$($sw.desktop.Count):$($sw.truncated)" }; Expect = '3:True' }
        @{ Name = 'software: the job carries the block, and an empty inventory is an empty block, not a refusal'
           Run = { $r = New-JobDocument -F $good -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r'; "$($r.Refusals.Count):$($null -ne $r.Job.harvest.software):$($r.Job.harvest.software.desktop.Count)" }; Expect = '0:True:0' }
        # --- the folder map (roadmap item 3, 2026-09-26) ---------------------
        @{ Name = 'folder map: the job carries the folders, the OneDrive result, the stick fit and the harvester version'
           Run = { $j = (New-JobDocument -F $good -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Job; $h = $j.harvest
                   "$($h.folders.Count):$($h.folders[0].name):$($h.folders[0].bytes):$($h.folders[0].is_onedrive):$($h.folders[1].exists):$($h.folders[1].path):$($h.cloud_files.result):$($h.stick_fit.fits):$($h.stick_fit.needed_bytes):$($j.evaluate.harvest_version)" }
           Expect = '2:Documents:1400000000:True:False:C:\Users\a\Music:none-found:True:1500000000:0.3.0' }
        @{ Name = 'folder map: refuse when there is no map, with the reason'
           Run = { (New-JobDocument -F (With (With $good 'Harvest' $null) 'HarvestError' 'the harvester wrote no folder map (exit 1)') -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Refusals -join ';' }
           Expect = 'the list of your folders could not be read (the harvester wrote no folder map (exit 1))' }
        @{ Name = 'folder map: refuse a harvester version this writer does not read (a mixed kit)'
           Run = { [bool]((New-JobDocument -F (With $good 'Harvest' (New-TestHarvest -Version '0.2.0')) -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Refusals -match "harvester '0\.2\.0'") }; Expect = $true }
        @{ Name = 'folder map: refuse when another account is signed in on this screen (UAC with someone else''s password)'
           Run = { [bool]((New-JobDocument -F (With $good 'Harvest' (New-TestHarvest -Owners @('S-1-5-21-9-1002'))) -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Refusals -match 'the wrong person''s') }; Expect = $true }
        @{ Name = 'folder map: refuse when no desktop owner can be found (who these folders belong to is unknown)'
           Run = { [bool]((New-JobDocument -F (With $good 'Harvest' (New-TestHarvest -Owners @())) -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Refusals -match '^could not tell who is signed in') }; Expect = $true }
        @{ Name = 'folder map (R6): refuse a folder that hit the file cap'
           Run = { [bool]((New-JobDocument -F (With $good 'Harvest' (New-TestHarvest -Truncated $true)) -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Refusals -match '^Documents holds more files than this version counts') }; Expect = $true }
        @{ Name = 'folder map (R6): refuse a folder with parts Windows would not list, naming the first'
           Run = { [bool]((New-JobDocument -F (With $good 'Harvest' (New-TestHarvest -Unreadable 2)) -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Refusals -match 'read 2 folder\(s\) inside Documents \(first: C:\\Users\\a\\OneDrive\\Documents\\locked\)') }; Expect = $true }
        @{ Name = 'folder map (R8, decided 2026-09-26): online-only files not downloaded are a job recording them as left-in-cloud, not a refusal'
           Run = { $r = New-JobDocument -F (With $good 'Harvest' (New-TestHarvest -CloudResult 'not-attempted' -Found 12 -Again 12)) -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r'; "$($r.Refusals.Count):$($r.Job.harvest.cloud_files.result):$($r.Job.harvest.cloud_files.placeholders_found):$($r.Job.harvest.cloud_files.materialized):$($r.Job.harvest.folders[0].cloud_only_files)" }; Expect = '0:left-in-cloud:12:0:12' }
        @{ Name = 'folder map (R8): a download that failed is a refusal'
           Run = { [bool]((New-JobDocument -F (With $good 'Harvest' (New-TestHarvest -CloudResult 'refused' -Found 12 -Failed 1)) -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Refusals -match '^1 of 12 OneDrive online-only file\(s\) could not be downloaded') }; Expect = $true }
        @{ Name = 'folder map (R8): files online-only again after the download are a refusal'
           Run = { [bool]((New-JobDocument -F (With $good 'Harvest' (New-TestHarvest -CloudResult 'materialized' -Found 12 -Again 3)) -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Refusals -match '^after downloading, 3 OneDrive file') }; Expect = $true }
        @{ Name = 'folder map (R8): a clean download is a job recording what was online-only before it'
           Run = { $j = (New-JobDocument -F (With $good 'Harvest' (New-TestHarvest -CloudResult 'materialized' -Found 12)) -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Job; "$($j.harvest.cloud_files.result):$($j.harvest.cloud_files.placeholders_found):$($j.harvest.cloud_files.materialized):$($j.harvest.folders[0].cloud_only_files)" }
           Expect = 'materialized:12:12:12' }
        @{ Name = 'folder map: a stick whose free space cannot be read is a refusal'
           Run = { [bool]((New-JobDocument -F (With $good 'Harvest' (New-TestHarvest -NoStick)) -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Refusals -match 'free space could not be read \(the drive was removed\)') }; Expect = $true }
        @{ Name = 'stick fit: keep-windows whose folders do not fit the stick is still a job - it records fits=false (no discard offer later, R26)'
           Run = { $j = (New-JobDocument -F (With $good 'Harvest' (New-TestHarvest -Fits $false -Needed 14320352249 -FitReason 'x')) -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Job; "$($j.intent.path):$($j.harvest.stick_fit.fits):$($j.harvest.stick_fit.gap_bytes)" }
           Expect = 'keep-windows:False:12486958073' }
        @{ Name = 'stick fit: clean slate whose folders do not fit is no job, with the gap (a stick of at least N GB)'
           Run = { $r = New-JobDocument -F (With (With $good 'ShrinkGB' 3.0) 'Harvest' (New-TestHarvest -Fits $false -Needed 14320352249 -FitReason 'the folders need 13.34 GB on the stick and it has 1.71 GB free')) -Desktop kde -PasswordHash $ph -IfCannotKeep clean-slate -ReportRel 'r'; "$($null -eq $r.Job):$($r.Refusals -join ';')" }
           Expect = 'True:Windows cannot be kept, and your folders do not fit on this stick: the folders need 13.34 GB on the stick and it has 1.71 GB free; a stick with at least 13.4 GB free would hold them - no job; nothing was changed' }
        @{ Name = 'stick fit (R5): clean slate on a computer with another account is no job - naming it'
           Run = { $r = New-JobDocument -F (With (With $good 'ShrinkGB' 3.0) 'Harvest' (New-TestHarvest -Others @([pscustomobject]@{ Sid = 'S-1-5-21-9-1002'; Path = 'C:\Users\kid' }))) -Desktop kde -PasswordHash $ph -IfCannotKeep clean-slate -ReportRel 'r'; "$($null -eq $r.Job):$([bool]($r.Refusals -match '1 other account\(s\) \(C:\\Users\\kid\); a clean slate would delete their files'))" }
           Expect = 'True:True' }
        @{ Name = 'stick fit: clean slate that fits, one account, is a job with the staged block'
           Run = { $j = (New-JobDocument -F (With $good 'ShrinkGB' 3.0) -Desktop kde -PasswordHash $ph -IfCannotKeep clean-slate -ReportRel 'r').Job; "$($j.intent.path):$([bool]$j.staged):$($j.harvest.stick_fit.fits)" }; Expect = 'clean-slate:True:True' }
        @{ Name = 'folder map (R5): another account on a keep-windows machine is not a refusal (their files stay in the kept Windows)'
           Run = { $null -ne (New-JobDocument -F (With $good 'Harvest' (New-TestHarvest -Others @([pscustomobject]@{ Sid = 's'; Path = 'C:\Users\kid' }))) -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Job }; Expect = $true }
        # --- the one-click erase and install (R27, 2026-09-26) -------------------
        @{ Name = 'erase: the sentence verbatim + a chosen password is a clean-slate fresh-start job naming the system drive, nothing staged'
           Run = { $j = (New-JobDocument -F $good -Desktop kde -PasswordHash $hp -IfCannotKeep stop -ReportRel 'r' -EraseEverything $es).Job
                   "$($j.intent.path):$($j.intent.path_reason):$($j.erase_consent.statement -ceq $es):$(@($j.erase_consent.disks).Count):$($j.erase_consent.disks[0].role):$($j.erase_consent.disks[0].unique_id):$($null -eq $j.staged):$($j.intent.account.password_hash -eq $hp)" }
           Expect = 'clean-slate:user-chose-fresh-start:True:1:system:eui.1:True:True' }
        @{ Name = 'erase (Aspire layout): a second Healthy internal drive becomes home; the USB stick is left out'
           Run = { $j = (New-JobDocument -F (With $good 'AllDisks' ($good.AllDisks + @(@{ Number = 1; Serial = 'WD1'; UniqueId = 'SCSI\DISK&VEN_WDC'; Size = 1000204886016; Name = 'WDC WD10SPZX'; Bus = 'SATA'; Health = 'Healthy' }))) -Desktop kde -PasswordHash $hp -IfCannotKeep stop -ReportRel 'r' -EraseEverything $es).Job
                   (@($j.erase_consent.disks) | ForEach-Object { "$($_.role)=$($_.friendly_name)" }) -join ',' }
           Expect = 'system=SSD,home=WDC WD10SPZX' }
        @{ Name = 'erase (rig): a stick on the same bus as the internal drive is left out by its unique id'
           Run = { $f = With $good 'AllDisks' @(@{ Number = 0; Serial = 'S1'; UniqueId = 'eui.1'; Size = 1; Name = 'Virtual Disk'; Bus = 'SCSI'; Health = 'Healthy' }, @{ Number = 1; Serial = ''; UniqueId = 'USBSTOR\X'; Size = 2; Name = 'Virtual Disk'; Bus = 'SCSI'; Health = 'Healthy' })
                   (@((New-JobDocument -F $f -Desktop kde -PasswordHash $hp -IfCannotKeep stop -ReportRel 'r' -EraseEverything $es).Job.erase_consent.disks) | ForEach-Object { $_.role }) -join ',' }
           Expect = 'system' }
        @{ Name = 'erase: a paraphrased sentence is a refusal'
           Run = { [bool]((New-JobDocument -F $good -Desktop kde -PasswordHash $hp -IfCannotKeep stop -ReportRel 'r' -EraseEverything 'I confirm everything will be deleted').Refusals -match '^the erase sentence was not typed exactly') }; Expect = $true }
        @{ Name = 'erase: no chosen password (the verify-only placeholder) is a refusal'
           Run = { [bool]((New-JobDocument -F $good -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r' -EraseEverything $es).Refusals -match '^no password was chosen') }; Expect = $true }
        @{ Name = 'erase: a third internal drive is a refusal'
           Run = { $d = $good.AllDisks + @(@{ Number = 1; Serial = 'A'; UniqueId = 'a'; Size = 1; Name = 'A'; Bus = 'SATA'; Health = 'Healthy' }, @{ Number = 3; Serial = 'B'; UniqueId = 'b'; Size = 1; Name = 'B'; Bus = 'SATA'; Health = 'Healthy' })
                   [bool]((New-JobDocument -F (With $good 'AllDisks' $d) -Desktop kde -PasswordHash $hp -IfCannotKeep stop -ReportRel 'r' -EraseEverything $es).Refusals -match 'has 3 internal drives; this version erases at most two') }; Expect = $true }
        @{ Name = 'erase: a second drive that is not Healthy never takes the home folder'
           Run = { $d = $good.AllDisks + @(@{ Number = 1; Serial = 'A'; UniqueId = 'a'; Size = 1; Name = 'Old HDD'; Bus = 'SATA'; Health = 'Warning' })
                   [bool]((New-JobDocument -F (With $good 'AllDisks' $d) -Desktop kde -PasswordHash $hp -IfCannotKeep stop -ReportRel 'r' -EraseEverything $es).Refusals -match "the second drive \(Old HDD\) reports 'Warning'") }; Expect = $true }
        @{ Name = 'erase: a drive on a bus this version does not know is a refusal, not a guess'
           Run = { $d = $good.AllDisks + @(@{ Number = 4; Serial = ''; UniqueId = 'v'; Size = 1; Name = 'Msft Virtual Disk'; Bus = 'File Backed Virtual'; Health = 'Healthy' })
                   [bool]((New-JobDocument -F (With $good 'AllDisks' $d) -Desktop kde -PasswordHash $hp -IfCannotKeep stop -ReportRel 'r' -EraseEverything $es).Refusals -match "bus 'File Backed Virtual'") }; Expect = $true }
        @{ Name = 'erase: an SD card is left alone, not a refusal'
           Run = { $d = $good.AllDisks + @(@{ Number = 5; Serial = ''; UniqueId = 'sd'; Size = 1; Name = 'SD Card'; Bus = 'SD'; Health = 'Healthy' })
                   @((New-JobDocument -F (With $good 'AllDisks' $d) -Desktop kde -PasswordHash $hp -IfCannotKeep stop -ReportRel 'r' -EraseEverything $es).Job.erase_consent.disks).Count }; Expect = 1 }
        @{ Name = 'erase: a RED machine still needs the R23 sentence too - the erase sentence does not lift it'
           Run = { [bool]((New-JobDocument -F (With (With $good 'Verdict' 'RED') 'FailedChecks' @('Disk health')) -Desktop kde -PasswordHash $hp -IfCannotKeep stop -ReportRel 'r' -EraseEverything $es).Refusals -match '^the scanner verdict is RED - no job, no override$') }; Expect = $true }
        @{ Name = 'erase (the Aspire): RED on disk health with both sentences is a job carrying both blocks'
           Run = { $j = (New-JobDocument -F (With (With $good 'Verdict' 'RED') 'FailedChecks' @('Disk health')) -Desktop kde -PasswordHash $hp -IfCannotKeep stop -ReportRel 'r' -AcknowledgeDataLoss 'I confirm that I understand the risks and could lose data' -EraseEverything $es).Job
                   "$($j.risk_acknowledgement.overrides -join ','):$([bool]$j.erase_consent):$($j.intent.path)" }; Expect = 'disk-health:True:clean-slate' }
        @{ Name = 'erase: the folder map''s refusals do not apply (nothing is kept) - another account signed in is still a job'
           Run = { $null -ne (New-JobDocument -F (With $good 'Harvest' (New-TestHarvest -Owners @('S-1-5-21-9-1002') -CloudResult 'not-attempted' -Found 3 -Fits $false)) -Desktop kde -PasswordHash $hp -IfCannotKeep stop -ReportRel 'r' -EraseEverything $es).Job }; Expect = $true }
        @{ Name = 'erase: with no folder map at all it refuses (it lists what will be deleted)'
           Run = { [bool]((New-JobDocument -F (With (With $good 'Harvest' $null) 'HarvestError' 'x') -Desktop kde -PasswordHash $hp -IfCannotKeep stop -ReportRel 'r' -EraseEverything $es).Refusals -match '^the list of your folders could not be read') }; Expect = $true }
        @{ Name = 'start_at: the person''s choice is carried into the job (desktop / console)'
           Run = { "$((New-JobDocument -F $good -Desktop gnome -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r' -StartAt desktop).Job.intent.start_at)/$((New-JobDocument -F $good -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r' -StartAt console).Job.intent.start_at)/$((New-JobDocument -F $good -Desktop gnome -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r' -StartAt desktop).Job.intent.desktop)" }; Expect = 'desktop/console/gnome' }
        @{ Name = 'no erase sentence: nothing changes - a plain job carries no erase consent'
           Run = { $null -eq (New-JobDocument -F $good -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Job.erase_consent }; Expect = $true }
        @{ Name = 'locale: en-US + 0409 + Eastern -> en_US.UTF-8 / us / America/New_York'
           Run = { $l = (New-JobDocument -F $good -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Job.intent.locale; "$($l.lang)/$($l.keymap)/$($l.timezone)" }; Expect = 'en_US.UTF-8/us/America/New_York' }
        @{ Name = 'licence (R30): Windows 11 from the build although the registry says Windows 10; activated OEM:DM with a firmware key'
           Run = { $l = ConvertTo-JobLicenseFromFacts $good.License; "$($l.result):$($l.windows_version):$($l.edition_id):$($l.activated):$($l.license_status):$($l.channel):$($l.firmware_key_present):$($l.firmware_key_description)" }; Expect = 'read:11:Core:True:1:OEM:DM:True:[4.0] Core OEM:DM' }
        @{ Name = 'licence: build 19045 is Windows 10; not activated is recorded, not refused'
           Run = { $l = ConvertTo-JobLicense -Os @{ Build = '19045'; EditionId = 'Professional' } -Products @(@{ LicenseStatus = 5; Channel = 'Retail'; Addon = $false }) -Firmware @{ Present = $false; Description = '' } -NowUtc 'x'; "$($l.windows_version):$($l.activated):$($l.license_status):$($l.firmware_key_present):$($null -eq $l.firmware_key_description)" }; Expect = '10:False:5:False:True' }
        @{ Name = 'licence: an add-on licence (Windows 10 extended updates) is not taken for Windows itself'
           Run = { $l = ConvertTo-JobLicense -Os @{ Build = '19045' } -Products @(@{ LicenseStatus = 1; Channel = 'Retail'; Addon = $true }, @{ LicenseStatus = 0; Channel = 'OEM:DM'; Addon = $false }) -Firmware $null -NowUtc 'x'; "$($l.activated):$($l.channel)" }; Expect = 'False:OEM:DM' }
        @{ Name = 'licence: the licensed product is chosen when there are several'
           Run = { (ConvertTo-JobLicense -Os @{ Build = '26100' } -Products @(@{ LicenseStatus = 0; Channel = 'Volume:GVLK'; Addon = $false }, @{ LicenseStatus = 1; Channel = 'Retail'; Addon = $false }) -NowUtc 'x').channel }; Expect = 'Retail' }
        @{ Name = 'licence: a failed read is unreadable with its reason, and the job is still written'
           Run = { $r = New-JobDocument -F (With $good 'License' @{ Os = @{ Build = '26100' }; Products = @(); Firmware = $null; Error = 'Access denied'; NowUtc = '2026-09-27T12:00:00Z' }) -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r'; $l = $r.Job.harvest.windows_license; "$($r.Refusals.Count):$($l.result):$([bool]($l.reason -match 'Access denied')):$($l.windows_version)" }; Expect = '0:unreadable:True:11' }
        @{ Name = 'licence: no Windows licence reported is unreadable, not a guess'
           Run = { $l = ConvertTo-JobLicense -Os @{ Build = '26100' } -Products @() -NowUtc 'x'; "$($l.result):$($null -eq $l.activated)" }; Expect = 'unreadable:True' }
        @{ Name = 'licence (R13): a value shaped like a product key never reaches the job'
           Run = { $l = ConvertTo-JobLicense -Os @{ Build = '26100'; ProductName = 'ABCDE-FGHIJ-KLMNO-PQRST-UVWXY' } -Products @(@{ LicenseStatus = 1; Channel = 'Retail'; Addon = $false }) -Firmware @{ Present = $true; Description = '[4.0] Core OEM:DM ABCDE-12345-FGHIJ-67890-KLMNO' } -ReadError $null -NowUtc 'x'; $j = ConvertTo-JobJson $l; "$([bool]($j -match '[A-Z0-9]{5}-[A-Z0-9]{5}-[A-Z0-9]{5}')):$($null -eq $l.product_name):$($null -eq $l.firmware_key_description):$([bool]($l.reason -match 'left out'))" }; Expect = 'False:True:True:True' }
        @{ Name = 'licence: the job carries harvest.windows_license'
           Run = { $l = (New-JobDocument -F $good -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Job.harvest.windows_license; "$($l.result):$($l.windows_version):$($l.edition_id):$($l.activated)" }; Expect = 'read:11:Core:True' }
        @{ Name = 'keymap: German KLID maps to de; unknown maps to null'
           Run = { "$(ConvertTo-JobKeymap '0407:00000407')/$($null -eq (ConvertTo-JobKeymap '0000:0000FFFF'))" }; Expect = 'de/True' }
        @{ Name = 'linux name: spaces stripped, lowercased, 32 max'
           Run = { ConvertTo-JobLinuxName 'John Smith' }; Expect = 'johnsmith' }
        @{ Name = 'json: LF only, schema string first'
           Run = { $j = ConvertTo-JobJson (New-JobDocument -F $good -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Job; (-not $j.Contains("`r")) -and ($j -match '"schema":\s*"job/1"') }; Expect = $true }
        @{ Name = 'fsutil: NOT Dirty parses clean, localized parses unknown'
           Run = { "$(ConvertFrom-JobFsutilDirty @('Volume - C: is NOT Dirty'))/$(ConvertFrom-JobFsutilDirty @('Volume - C: ist NICHT fehlerhaft'))" }; Expect = 'clean/unknown' }
    )
    $failed = 0
    Write-Host ''; Write-Host "  upgrade_  job writer $JobWriterVersion  -  SELF-TEST" -ForegroundColor Cyan; Write-Host ''
    foreach ($c in $cases) {
        $got = & $c.Run
        if ("$got" -eq "$($c.Expect)") { Write-Host "    PASS  $($c.Name)" -ForegroundColor Green }
        else { Write-Host "    FAIL  $($c.Name)  (expected '$($c.Expect)', got '$got')" -ForegroundColor Red; $failed++ }
    }
    Write-Host ''
    if ($failed -gt 0) { Write-Host "  $failed check(s) failed" -ForegroundColor Red; exit 1 }
    Write-Host '  all checks passed' -ForegroundColor Green; Write-Host ''
}

# --- main -----------------------------------------------------------------------------

if ($SelfTest) { Invoke-SelfTest; return }
if ($PrintLinuxName) { ConvertTo-JobLinuxName $env:USERNAME; return }
if ($HarvestSettingsOut) {
    if (-not $OutDir) { throw 'give -OutDir <stick>\upgrade_ with -HarvestSettingsOut' }
    $c = Get-JobClockFacts; $iana = ConvertTo-JobIanaTimeZone -WindowsId $c.WindowsZone
    $ck = ConvertTo-JobClock -WindowsZone "$($c.WindowsZone)" -Iana "$iana" -RealTimeIsUniversal $c.RealTimeIsUniversal -DynamicDstDisabled $c.DynamicDstDisabled -OffsetMinutes ([int]$c.OffsetMinutes) -BaseOffsetMinutes ([int]$c.BaseOffsetMinutes) -DstActive ([bool]$c.DstActive) -NowUtc "$($c.NowUtc)"
    $why = @(); if (-not $iana) { $why += "Windows time zone '$($c.WindowsZone)' has no IANA mapping in this version" }; if ($ck.Refusal) { $why += $ck.Refusal }
    if ($why.Count -eq 0) { $wx = Export-JobWifi -OutDir $OutDir; if ($wx.Refusal) { $why += $wx.Refusal } }
    if ($why.Count -gt 0) { foreach ($x in $why) { Write-Host "  REFUSED: $x" -ForegroundColor Red }; exit 2 }
    [IO.File]::WriteAllText($HarvestSettingsOut, (ConvertTo-JobJson ([ordered]@{ job_writer = $JobWriterVersion; clock = $ck.Clock; wifi = $wx.Wifi; windows_license = (ConvertTo-JobLicenseFromFacts (Get-JobLicenseFacts)) })), (New-Object Text.UTF8Encoding($false)))
    Write-Host "  clock + Wi-Fi harvest written: $HarvestSettingsOut (Wi-Fi: $($wx.Wifi.result), $(@($wx.Wifi.profiles).Count) network(s))"
    return
}
if (-not $OutDir -or -not $StickDrive) { throw 'give -StickDrive X: -OutDir <stick>\upgrade_ -ScanDir <reports dir> (or -SelfTest)' }
if (-not (Test-JobAdmin)) { throw 'the job writer needs Administrator: the shrink measurement, the volume flag, BitLocker and the ESP are elevated-only reads' }

Write-Host ''; Write-Host "  upgrade_  job writer $JobWriterVersion" -ForegroundColor Cyan
Write-Host '  reads this machine; writes job.json; changes nothing' -ForegroundColor DarkGray
if (-not $VerifyOnly -and -not $PasswordHashFile) { Write-Host ''; Write-Host '  REFUSED - no job written: no password was chosen for the new account (a job that installs needs -PasswordHashFile; only a verify-only job may use the placeholder)' -ForegroundColor Red; exit 2 }
if (-not $StartAt) { Write-Host ''; Write-Host '  REFUSED - no job written: no choice was made of what the computer starts at (-StartAt desktop or console)' -ForegroundColor Red; exit 2 }
if ($PasswordHashFile) {
    if (-not (Test-Path -LiteralPath $PasswordHashFile)) { throw "no password hash at $PasswordHashFile" }
    $PasswordHash = (Get-Content -LiteralPath $PasswordHashFile -Raw).Trim()
}
$facts = Get-JobFacts -ScanDir $ScanDir -StickDrive $StickDrive -Materialize $Materialize.IsPresent
$reportRel = if ($facts.Report) { 'reports/' + (Split-Path $facts.Report -Leaf) } else { 'reports/none' }
$r = New-JobDocument -F $facts -Desktop $Desktop -PasswordHash $PasswordHash -IfCannotKeep $IfCannotKeep -ReportRel $reportRel -AcknowledgeDataLoss $AcknowledgeDataLoss -EraseEverything $EraseEverything -StartAt $StartAt
if ($r.Refusals.Count -gt 0) {
    Write-Host ''; Write-Host '  REFUSED - no job written:' -ForegroundColor Red
    foreach ($x in $r.Refusals) { Write-Host "    - $x" -ForegroundColor Red }
    Write-Host ''; exit 2
}
New-Item -ItemType Directory -Path $OutDir -Force | Out-Null
if (-not $VerifyOnly) {
    # The launcher showed the owner's approved Wi-Fi sentence before anything was typed (2026-09-27)
    $wx = Export-JobWifi -OutDir $OutDir
    if ($wx.Refusal) {
        Write-Host ''; Write-Host '  REFUSED - no job written:' -ForegroundColor Red
        Write-Host "    - $($wx.Refusal)" -ForegroundColor Red; Write-Host ''; exit 2
    }
    $r.Job.harvest.wifi = $wx.Wifi
}
$jobPath = Join-Path $OutDir 'job.json'
[IO.File]::WriteAllText($jobPath, (ConvertTo-JobJson $r.Job), (New-Object Text.UTF8Encoding($false)))
if ($r.Job.harvest.bitlocker.status -eq 'on') {
    $cred = Join-Path $OutDir 'artifacts\credentials'
    New-Item -ItemType Directory -Path $cred -Force | Out-Null
    [IO.File]::WriteAllText((Join-Path $cred 'bitlocker-C.txt'), "NOT HARVESTED - this job writer ($JobWriterVersion) does not extract the recovery key yet. The live-boot leg does not need it.`n", (New-Object Text.UTF8Encoding($false)))
}
$j = $r.Job
Write-Host ''
Write-Host "  job $($j.job_id)" -ForegroundColor Green
Write-Host "  $($j.identity.vendor) $($j.identity.model)   disk $($j.identity.system_disk.friendly_name) ($([math]::Round($j.identity.system_disk.size_bytes/1e9,1)) GB)   Secure Boot $($j.identity.secure_boot)   BitLocker $($j.harvest.bitlocker.status)"
Write-Host "  verdict $($j.scan.verdict)   disk health $($j.storage.physical_disk.health_status)   shrinkable $($j.storage.shrinkable_gb) GB   ESP free $([math]::Round($j.storage.esp.free_bytes/1MB,1)) MB   volume $($j.storage.volume_health.dirty)"
if ($j.storage.last_unmovable_file) { Write-Host "  Windows names the last unmovable file: $($j.storage.last_unmovable_file)$(if (Test-JobShrinkMitigable -LastUnmovable $j.storage.last_unmovable_file) { ' - the prologue turns it off and measures again' })" -ForegroundColor DarkGray }
if ($facts.RepairStale) { Write-Host "  $($facts.RepairStale) - not a queued repair" -ForegroundColor DarkGray }
if ($j.intent.path -eq 'keep-windows' -and ($null -eq $j.storage.shrinkable_gb -or $j.storage.shrinkable_gb -lt $LinuxMinGB)) { Write-Host "  not enough room measured yet: the prologue measures again before it changes anything, and if Linux still does not fit it $(if ($j.fork.if_cannot_keep -eq 'stop') { 'stops, as you chose' } else { 'takes the clean slate you chose' })" -ForegroundColor DarkGray }
Write-Host "  path $($j.intent.path) ($($j.intent.path_reason))   desktop $($j.intent.desktop), starts at the $($j.intent.start_at)   locale $($j.intent.locale.lang) $($j.intent.locale.keymap) $($j.intent.locale.timezone)"
Write-Host "  stick $($j.stick.friendly_name) $([math]::Round($j.stick.size_bytes/1e9,1)) GB '$($j.stick.label)'"
if ($j.risk_acknowledgement) { Write-Host "  DATA LOSS ACCEPTED: the RED verdict was acknowledged; lifted: $($j.risk_acknowledgement.overrides -join ', ')" -ForegroundColor Red }
Write-Host "  written: $jobPath" -ForegroundColor Cyan
if (-not $VerifyOnly) { Write-Host ''; Write-Host "  Your Fedora sign-in:  user  $($j.intent.account.linux_name)   password  the one you just chose" -ForegroundColor Green }
Write-Host "  software inventory: $($j.harvest.software.desktop.Count) desktop programs, $($j.harvest.software.store.Count) Store apps (names only; stays on the stick)" -ForegroundColor DarkGray
if ($j.erase_consent) {
    Write-Host ''
    Write-Host '  ERASE AND INSTALL: everything on these drives will be deleted, nothing is kept' -ForegroundColor Red
    foreach ($d in @($j.erase_consent.disks)) { Write-Host ("    {0,-7} {1}  {2:N1} GB  serial {3}  ({4})" -f $d.role, $d.friendly_name, ($d.size_bytes / 1e9), $d.serial_number, $(if ($d.role -eq 'system') { 'Fedora system' } else { 'your home folder' })) -ForegroundColor Red }
    Write-Host '  In the installer a 2-minute countdown comes first: press any key there to cancel and go back to Windows.' -ForegroundColor Yellow
}
Write-Host $(if ($j.erase_consent) { '  your folders now (all of these will be DELETED):' } else { '  your folders (settle-in copies these; a clean slate stages them to the stick):' })
foreach ($fo in @($j.harvest.folders)) {
    if (-not $fo.exists) { Write-Host ("    {0,-10} not found" -f $fo.name) -ForegroundColor DarkGray; continue }
    Write-Host ("    {0,-10} {1,8:N2} GB  {2,7} files{3}  {4}" -f $fo.name, ($fo.bytes / 1GB), $fo.files, $(if ($fo.is_onedrive) { '  (OneDrive)' } else { '' }), $fo.path) -ForegroundColor DarkGray
}
$sf = $j.harvest.stick_fit
Write-Host ("  on the stick they would need {0:N2} GB; it has {1:N2} GB free ({2}) - {3}" -f ($sf.needed_bytes / 1GB), ($sf.free_bytes / 1GB), $sf.filesystem, $(if ($sf.fits) { 'they fit' } else { "they do not fit: $($facts.Harvest.StickFit.Reason)" })) -ForegroundColor DarkGray
if ($j.harvest.cloud_files.result -eq 'left-in-cloud') { Write-Host "  OneDrive: $($j.harvest.cloud_files.placeholders_found) online-only file(s) stay in OneDrive; they are not copied - after the conversion, sign in to OneDrive to reach them" -ForegroundColor DarkGray }
if ($j.harvest.cloud_files.result -eq 'materialized') { Write-Host "  OneDrive: $($j.harvest.cloud_files.materialized) online-only file(s) downloaded and kept on this device" -ForegroundColor DarkGray }
$op = @($facts.Harvest.Owner.OtherProfiles)
if ($op.Count -gt 0) { Write-Host "  other accounts on this computer: $(@($op | ForEach-Object { $_.Path }) -join ', ') - their files are not in this job (RISKS R5)" -ForegroundColor Yellow }
$wf = $j.harvest.wifi
if ($wf.result -eq 'exported') {
    $ok = @($wf.profiles | Where-Object { $_.supported }); $no = @($wf.profiles | Where-Object { -not $_.supported })
    Write-Host "  Wi-Fi: $($ok.Count) saved network(s) Fedora will join by itself; their passwords are on the stick until the install ends" -ForegroundColor DarkGray
    foreach ($x in $no) { Write-Host "    not set up: $($x.ssid) - $($x.why_not)" -ForegroundColor Yellow }
} elseif ($wf.result -eq 'no-wireless') { Write-Host '  Wi-Fi: this computer has no Wi-Fi' -ForegroundColor DarkGray }
elseif ($wf.result -eq 'none-saved') { Write-Host '  Wi-Fi: no saved networks' -ForegroundColor DarkGray }
$ck = $j.harvest.clock
Write-Host "  clock: $($ck.iana), hardware clock in $(if ($ck.rtc_is_local) { 'local time (settle-in turns it to UTC on first startup)' } else { 'UTC' })" -ForegroundColor DarkGray
$wl = $j.harvest.windows_license
if ($wl.result -eq 'read') { Write-Host "  Windows: $(if ($wl.windows_version) { "Windows $($wl.windows_version)" } else { 'version unknown' }) $($wl.edition_id), $(if ($wl.activated) { 'activated' } else { "NOT activated (status $($wl.license_status))" }), channel $($wl.channel), $(if ($wl.firmware_key_present) { "a key in the firmware ($($wl.firmware_key_description))" } else { 'no key in the firmware' }) - kept for the way back to Windows; no key is copied" -ForegroundColor DarkGray }
else { Write-Host "  Windows licence: not read ($($wl.reason)) - the way back to Windows will know less" -ForegroundColor Yellow }
Write-Host '  not in this job: browsers, the BitLocker key' -ForegroundColor DarkGray
Write-Host ''
