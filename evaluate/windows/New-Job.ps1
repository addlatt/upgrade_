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

    What it does NOT yet do (said plainly so the job is read as what it is):
    it does not harvest folders, browsers or Wi-Fi (those blocks are empty),
    does not materialize cloud files, does not extract the BitLocker key
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
    kde (default) or gnome.
#>
[CmdletBinding()]
param(
    [string]$ScanDir,
    [string]$StickDrive,
    [string]$OutDir,
    [ValidateSet('kde', 'gnome')][string]$Desktop = 'kde',
    [string]$PasswordHash = '$6$upgradeV1$MkYfbaBe.FFp2fzSNrPiJ6RdPagcfI.crkepTcQpGsjGFMe8780OtkedouSyxvXdky5a6WiTWDy/.epwkWUk71',
    [ValidateSet('clean-slate', 'stop')][string]$IfCannotKeep = 'stop',
    [string]$AcknowledgeDataLoss,
    [switch]$SelfTest
)
$ErrorActionPreference = 'Stop'
$JobWriterVersion = '0.4.0'
$LinuxMinGB = 25
# The acknowledged-data-loss path (RISKS R23, decided 2026-09-13). The person
# types this sentence, verbatim, on the separate launcher; it lifts exactly
# the two refusals whose failure mode is losing THIS machine's files, and
# nothing else. Kept in one place so every module compares the same bytes.
$RiskStatement = 'I confirm that I understand the risks and could lose data'
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

function Get-JobPath {
    # Pure: the path decision (architecture.md, "The conversion path is not a
    # coin flip"): keep Windows whenever the disk can, else clean slate, forced.
    # A volume that carries the dirty flag cannot be measured at all (RISKS
    # R18): on a Healthy disk that is NOT "no room" - it is a keep-windows job
    # whose number the prologue measures after its disk check, branching on
    # fork.if_cannot_keep if it then does not fit (decided 2026-09-08; the
    # 0.1.0 writer forced clean slate here, which pre-empted the fork).
    param([string]$DiskHealth, [bool]$EspFits, $ShrinkableGB, [string]$Dirty = 'clean', [bool]$DiskHealthAcknowledged = $false)
    if (($DiskHealth -eq 'Healthy' -or $DiskHealthAcknowledged) -and $EspFits) {
        if ($null -ne $ShrinkableGB -and $ShrinkableGB -ge $LinuxMinGB) { return @{ Path = 'keep-windows'; Reason = 'default' } }
        if ($null -eq $ShrinkableGB -and $Dirty -eq 'dirty') { return @{ Path = 'keep-windows'; Reason = 'default' } }
    }
    @{ Path = 'clean-slate'; Reason = 'forced-no-room' }
}

function ConvertFrom-JobFsutilDirty {
    param([string[]]$Lines)
    $t = (@($Lines) -join "`n")
    if ($t -match '(?i)\bis\s+NOT\s+Dirty\b') { return 'clean' }
    if ($t -match '(?i)\bis\s+Dirty\b') { return 'dirty' }
    'unknown'
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

function Get-JobFacts {
    param([string]$ScanDir, [string]$StickDrive)
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
    $f.Operational = if ($pd) { (@($pd.OperationalStatus) -join ',') } else { 'unknown' }
    $f.MediaType = if ($pd) { "$($pd.MediaType)" } else { '' }

    $f.ShrinkGB = $null; $f.ShrinkError = $null
    try { $s = Get-PartitionSupportedSize -DriveLetter C -ErrorAction Stop; $f.ShrinkGB = [math]::Round(($part.Size - $s.SizeMin) / 1GB, 1) }
    catch { $f.ShrinkError = ($_.Exception.Message -replace '\s+', ' ').Trim() }

    $f.Dirty = 'unknown'
    try { $f.Dirty = ConvertFrom-JobFsutilDirty -Lines @(& fsutil dirty query C: 2>&1 | ForEach-Object { "$_" }) } catch { }

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
    $f.InputTip = try { (Get-WinUserLanguageList)[0].InputMethodTips[0] } catch { '' }
    $f.Software = Get-JobSoftware
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

function New-JobDocument {
    param($F, [string]$Desktop, [string]$PasswordHash, [string]$IfCannotKeep, [string]$ReportRel, [string]$AcknowledgeDataLoss)
    $refusals = @()
    $ack = Get-JobAcknowledgement -Verdict "$($F.Verdict)" -FailedChecks @($F.FailedChecks) -WarnChecks @($F.WarnChecks) -Typed $AcknowledgeDataLoss
    if ($ack.Refusal) { $refusals += $ack.Refusal }
    elseif ($F.Verdict -notin @('GREEN', 'YELLOW', 'RED')) { $refusals += "no scanner verdict found (got '$($F.Verdict)') - run the scanner with -Json first" }
    if ($F.Firmware -ne 'UEFI') { $refusals += "firmware is '$($F.Firmware)', not UEFI - the boot handoff does not apply" }
    if ($F.BitLocker -notin @('on', 'off')) { $refusals += 'BitLocker state on C: could not be determined' }
    $iana = ConvertTo-JobIanaTimeZone -WindowsId $F.WindowsTz
    if (-not $iana) { $refusals += "Windows time zone '$($F.WindowsTz)' has no IANA mapping in this version" }
    $keymap = ConvertTo-JobKeymap -InputMethodTip $F.InputTip
    if (-not $keymap) { $refusals += "keyboard layout '$($F.InputTip)' has no mapping in this version" }
    if (-not $F.Stick) { $refusals += "the stick's identity could not be read ($($F.StickError))" }
    if ($F.Stick -and $F.Stick.Bus -ne 'USB') { $refusals += "the stick is on bus '$($F.Stick.Bus)', not USB" }
    if (-not $F.Disk.UniqueId) { $refusals += 'the system disk has no unique id' }
    if ($refusals.Count -gt 0) { return @{ Refusals = $refusals; Job = $null } }

    $espFits = ($F.EspFree -ge 32MB)
    $diskAck = [bool]($ack.Block -and ($ack.Block.overrides -contains 'disk-health'))
    $path = Get-JobPath -DiskHealth $F.Health -EspFits $espFits -ShrinkableGB $F.ShrinkGB -Dirty $F.Dirty -DiskHealthAcknowledged $diskAck
    $health = if ($F.Health -in @('Healthy', 'Warning', 'Unhealthy')) { $F.Health } else { 'Unknown' }
    $bl = $F.BitLocker
    $job = [ordered]@{
        schema = 'job/1'
        job_id = [guid]::NewGuid().ToString()
        created_utc = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
        evaluate = [ordered]@{ version = $JobWriterVersion; scanner_version = 'see report'; harvest_version = 'none'; ran_as_admin = $true }
        identity = [ordered]@{
            vendor = $F.Vendor; model = $F.Model; system_uuid = $F.Uuid; bios_serial = $F.BiosSerial; bios_version = $F.BiosVersion
            firmware_mode = 'UEFI'; secure_boot = $F.SecureBoot; os_caption = $F.OsCaption; os_build = [int]$F.OsBuild
            system_disk = [ordered]@{ number = [int]$F.Disk.Number; serial_number = "$($F.Disk.Serial)"; unique_id = $F.Disk.UniqueId
                                      friendly_name = $F.Disk.Name; size_bytes = [long]$F.Disk.Size; partition_style = $F.Disk.Style }
        }
        scan = [ordered]@{ verdict = $F.Verdict; required_kernel = $(if ($F.RequiredKernel) { "$($F.RequiredKernel)" } else { $null }); report = $ReportRel }
        intent = [ordered]@{
            path = $path.Path; path_reason = $path.Reason; desktop = $Desktop
            distro = [ordered]@{ name = 'fedora'; release = '42' }
            account = [ordered]@{ windows_name = $F.UserName; full_name = $(if ($F.FullName) { "$($F.FullName)" } else { $null })
                                  linux_name = (ConvertTo-JobLinuxName $F.UserName); password_hash = $PasswordHash }
            locale = [ordered]@{ lang = (($F.Locale -replace '-', '_') + '.UTF-8'); timezone = $iana; keymap = $keymap }
        }
        fork = [ordered]@{ if_cannot_keep = $IfCannotKeep; volume_check_consented = $true }
        storage = [ordered]@{
            shrinkable_gb = $F.ShrinkGB; shrink_source = $(if ($null -ne $F.ShrinkGB) { 'storage-api' } else { $null }); shrink_error = $F.ShrinkError
            linux_min_gb = $LinuxMinGB
            volume_health = [ordered]@{ dirty = $F.Dirty; scan = $null }
            physical_disk = [ordered]@{ health_status = $health; operational_status = "$($F.Operational)"; media_type = "$($F.MediaType)" }
            esp = [ordered]@{ size_bytes = [long]$F.EspSize; free_bytes = [long]$F.EspFree; fits_alongside_install = $espFits }
        }
        harvest = [ordered]@{
            folders = @()
            cloud_files = [ordered]@{ placeholders_found = 0; materialized = 0; failed = 0; result = 'none-found' }
            browsers = @()
            wifi = [ordered]@{ profiles = @(); secrets_file = $null }
            bitlocker = [ordered]@{ status = $bl; recovery_key_file = $(if ($bl -eq 'on') { 'artifacts/credentials/bitlocker-C.txt' } else { $null }) }
            firmware_artifacts = @()
            software = $(if ($F.Software) { $F.Software } else { [ordered]@{ desktop = @(); store = @(); truncated = $false } })
        }
        stick = [ordered]@{ unique_id = $F.Stick.UniqueId; serial_number = "$($F.Stick.Serial)"; size_bytes = [long]$F.Stick.Size
                            friendly_name = $F.Stick.Name; label = $(if ($F.Stick.Label) { $F.Stick.Label } else { 'UPGV0' }); manifest = 'SHA256SUMS' }
    }
    if ($ack.Block) { $job.risk_acknowledgement = $ack.Block }
    if ($path.Path -eq 'clean-slate') { $job.staged = [ordered]@{ files = 0; bytes = 0; manifest = 'staging/SHA256SUMS' } }
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
    $good = @{ Vendor = 'Acer'; Model = 'Aspire'; Uuid = 'u'; BiosSerial = 's'; BiosVersion = 'v'; OsCaption = 'Windows 10'; OsBuild = 19045
               Firmware = 'UEFI'; SecureBoot = 'on'; Disk = @{ Number = 0; Serial = 'S1'; UniqueId = 'eui.1'; Name = 'SSD'; Size = 250059350016; Style = 'GPT' }
               Health = 'Healthy'; Operational = 'OK'; MediaType = 'SSD'; ShrinkGB = 61.4; ShrinkError = $null; Dirty = 'clean'
               EspSize = 104857600; EspFree = 72219648; BitLocker = 'on'; Verdict = 'YELLOW'; RequiredKernel = '6.7'; Report = 'x'
               Stick = @{ UniqueId = 'USBSTOR\X'; Serial = ''; Size = 8053063680; Name = 'General UDisk'; Label = 'UPGV0'; Bus = 'USB' }
               WindowsTz = 'Eastern Standard Time'; Locale = 'en-US'; InputTip = '0409:00000409'; UserName = 'Addison'; FullName = 'Addison Example'
               FailedChecks = @(); WarnChecks = @() }
    function With { param($h, [string]$k, $v) $c = @{}; foreach ($e in $h.GetEnumerator()) { $c[$e.Key] = $e.Value }; $c[$k] = $v; $c }
    $ph = '$6$upgradeV1$MkYfbaBe.FFp2fzSNrPiJ6RdPagcfI.crkepTcQpGsjGFMe8780OtkedouSyxvXdky5a6WiTWDy/.epwkWUk71'
    $cases = @(
        @{ Name = 'a healthy machine with room gets a keep-windows job'
           Run = { $r = New-JobDocument -F $good -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r.txt'; "$($r.Refusals.Count):$($r.Job.intent.path):$($r.Job.intent.path_reason)" }; Expect = '0:keep-windows:default' }
        @{ Name = 'too little shrink room forces clean slate, with staged block'
           Run = { $r = New-JobDocument -F (With $good 'ShrinkGB' 10.0) -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r'; "$($r.Job.intent.path):$($r.Job.intent.path_reason):$([bool]$r.Job.staged)" }; Expect = 'clean-slate:forced-no-room:True' }
        @{ Name = 'an unmeasured shrink (null) on a clean volume forces clean slate'
           Run = { (New-JobDocument -F (With $good 'ShrinkGB' $null) -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Job.intent.path }; Expect = 'clean-slate' }
        @{ Name = 'an unmeasured shrink on a FLAGGED volume, Healthy disk, is a keep-windows job with the fork pending (R18)'
           Run = { $r = New-JobDocument -F (With (With $good 'ShrinkGB' $null) 'Dirty' 'dirty') -Desktop kde -PasswordHash $ph -IfCannotKeep 'clean-slate' -ReportRel 'r'; "$($r.Job.intent.path):$($r.Job.fork.if_cannot_keep):$($r.Job.fork.volume_check_consented):$($r.Job.storage.volume_health.dirty):$($null -eq $r.Job.storage.shrinkable_gb)" }; Expect = 'keep-windows:clean-slate:True:dirty:True' }
        @{ Name = 'a flagged volume on a Warning disk still forces clean slate'
           Run = { (New-JobDocument -F (With (With (With $good 'ShrinkGB' $null) 'Dirty' 'dirty') 'Health' 'Warning') -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Job.intent.path }; Expect = 'clean-slate' }
        @{ Name = 'a Warning disk forces clean slate'
           Run = { (New-JobDocument -F (With $good 'Health' 'Warning') -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Job.intent.path }; Expect = 'clean-slate' }
        @{ Name = 'a full ESP forces clean slate'
           Run = { (New-JobDocument -F (With $good 'EspFree' 1000000) -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Job.intent.path }; Expect = 'clean-slate' }
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
        @{ Name = 'locale: en-US + 0409 + Eastern -> en_US.UTF-8 / us / America/New_York'
           Run = { $l = (New-JobDocument -F $good -Desktop kde -PasswordHash $ph -IfCannotKeep stop -ReportRel 'r').Job.intent.locale; "$($l.lang)/$($l.keymap)/$($l.timezone)" }; Expect = 'en_US.UTF-8/us/America/New_York' }
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
if (-not $OutDir -or -not $StickDrive) { throw 'give -StickDrive X: -OutDir <stick>\upgrade_ -ScanDir <reports dir> (or -SelfTest)' }
if (-not (Test-JobAdmin)) { throw 'the job writer needs Administrator: the shrink measurement, the volume flag, BitLocker and the ESP are elevated-only reads' }

Write-Host ''; Write-Host "  upgrade_  job writer $JobWriterVersion" -ForegroundColor Cyan
Write-Host '  reads this machine; writes job.json; changes nothing' -ForegroundColor DarkGray
$facts = Get-JobFacts -ScanDir $ScanDir -StickDrive $StickDrive
$reportRel = if ($facts.Report) { 'reports/' + (Split-Path $facts.Report -Leaf) } else { 'reports/none' }
$r = New-JobDocument -F $facts -Desktop $Desktop -PasswordHash $PasswordHash -IfCannotKeep $IfCannotKeep -ReportRel $reportRel -AcknowledgeDataLoss $AcknowledgeDataLoss
if ($r.Refusals.Count -gt 0) {
    Write-Host ''; Write-Host '  REFUSED - no job written:' -ForegroundColor Red
    foreach ($x in $r.Refusals) { Write-Host "    - $x" -ForegroundColor Red }
    Write-Host ''; exit 2
}
New-Item -ItemType Directory -Path $OutDir -Force | Out-Null
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
Write-Host "  path $($j.intent.path) ($($j.intent.path_reason))   desktop $($j.intent.desktop)   locale $($j.intent.locale.lang) $($j.intent.locale.keymap) $($j.intent.locale.timezone)"
Write-Host "  stick $($j.stick.friendly_name) $([math]::Round($j.stick.size_bytes/1e9,1)) GB '$($j.stick.label)'"
if ($j.risk_acknowledgement) { Write-Host "  DATA LOSS ACCEPTED: the RED verdict was acknowledged; lifted: $($j.risk_acknowledgement.overrides -join ', ')" -ForegroundColor Red }
Write-Host "  written: $jobPath" -ForegroundColor Cyan
Write-Host "  software inventory: $($j.harvest.software.desktop.Count) desktop programs, $($j.harvest.software.store.Count) Store apps (names only; stays on the stick)" -ForegroundColor DarkGray
Write-Host '  not in this job: folders, browsers, Wi-Fi, cloud files, the BitLocker key, a chosen password' -ForegroundColor DarkGray
Write-Host ''
