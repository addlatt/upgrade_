<#
.SYNOPSIS
    upgrade_ converter, phase A step 1 - harvest machine state.

.DESCRIPTION
    Read-only. Collects everything the Linux side needs to reproduce this
    user's setup: identity, locale, user folders, Wi-Fi networks, browser
    profiles, and the backup capacity required.

    Writes state.json. Touches nothing else. Makes no changes to this machine.

    Wi-Fi passwords are NOT collected unless -IncludeWifiSecrets is passed,
    because doing so turns the output file into a credential store.

.PARAMETER OutDir
    Where to write state.json. Defaults to a folder on your Desktop.

.PARAMETER IncludeWifiSecrets
    Also export Wi-Fi passwords in cleartext. Requires Administrator.
    The resulting file can connect anyone to the user's network.

.PARAMETER SkipSizes
    Skip recursive folder sizing. Much faster; the backup estimate becomes
    unavailable.

.PARAMETER Materialize
    Force every cloud-only placeholder in the user folders to real bytes on
    disk (OneDrive "free up space" files) before the folder map is written.
    This is the one thing the harvest does that is not a read: a placeholder
    copied from Linux later arrives EMPTY, and no later stage can fill it
    (RISKS R8 / V8). Every placeholder is pinned ("always keep on this
    device") and read through, then verified to hold its bytes. A file that
    cannot be made local is a refusal, not a warning.

.PARAMETER MaterializePath
    Run only the materialization step against one directory and write the
    per-file result as JSON to -MaterializeResult. This is the seam the V8
    harness (Test-Materialize.ps1) drives, in a separate process from the
    sync provider - exactly the shape of the real thing.

.EXAMPLE
    .\Harvest-UpgradeState.ps1

.EXAMPLE
    .\Harvest-UpgradeState.ps1 -IncludeWifiSecrets -OutDir E:\upgrade
#>
[CmdletBinding()]
param(
    [string]$OutDir,
    [switch]$IncludeWifiSecrets,
    [switch]$SkipSizes,
    [switch]$Materialize,
    [string]$MaterializePath,
    [string]$MaterializeResult,
    [int]$MaterializeTimeoutSec = 600,
    [switch]$SelfTest
)

$ErrorActionPreference = 'Stop'
$HarvestVersion = '0.2.0'

# Windows sets this on files that live in the cloud and have not been
# downloaded. Copying one gives you an empty file, silently.
$FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS = 0x400000
$FILE_ATTRIBUTE_OFFLINE               = 0x1000

function Test-HarvestAdmin {
    $id = [Security.Principal.WindowsIdentity]::GetCurrent()
    (New-Object Security.Principal.WindowsPrincipal($id)).IsInRole(
        [Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Write-Step {
    param([string]$Text)
    Write-Host "  $Text" -ForegroundColor DarkGray
}

function Write-Warn {
    param([string]$Text)
    Write-Host "  ! $Text" -ForegroundColor Yellow
}

# =============================================================================
#  identity - phase B re-checks this before it partitions anything
# =============================================================================

function Get-HarvestIdentity {
    $cs   = Get-CimInstance Win32_ComputerSystem
    $os   = Get-CimInstance Win32_OperatingSystem
    $bios = Get-CimInstance Win32_BIOS
    $sys  = Get-CimInstance Win32_ComputerSystemProduct

    # The system disk is the one holding C:. Its serial is how the live
    # environment confirms the USB has not been moved to another machine.
    $sysDisk = $null
    try {
        $part = Get-Partition -DriveLetter C -ErrorAction Stop
        $sysDisk = Get-Disk -Number $part.DiskNumber -ErrorAction Stop
    } catch { }

    [pscustomobject]@{
        Vendor        = $cs.Manufacturer
        Model         = $cs.Model
        SystemUuid    = $sys.UUID
        BiosVersion   = $bios.SMBIOSBIOSVersion
        BiosSerial    = $bios.SerialNumber
        FirmwareMode  = $env:firmware_type
        OsCaption     = $os.Caption
        OsBuild       = [int]$os.BuildNumber
        SystemDisk    = if ($sysDisk) {
            [pscustomobject]@{
                Number         = $sysDisk.Number
                SerialNumber   = ($sysDisk.SerialNumber -replace '\s','')
                UniqueId       = $sysDisk.UniqueId
                FriendlyName   = $sysDisk.FriendlyName
                SizeBytes      = $sysDisk.Size
                PartitionStyle = $sysDisk.PartitionStyle
            }
        } else { $null }
    }
}

# =============================================================================
#  locale - becomes kickstart lang/timezone/keyboard
# =============================================================================

function ConvertTo-HarvestIanaTimeZone {
    # Windows time zone IDs are not IANA IDs. Only a handful matter in
    # practice; anything unmapped returns $null - the installer asks.
    param([string]$WindowsTimeZoneId)
    $tzMap = @{
        'Eastern Standard Time'  = 'America/New_York'
        'Central Standard Time'  = 'America/Chicago'
        'Mountain Standard Time' = 'America/Denver'
        'Pacific Standard Time'  = 'America/Los_Angeles'
        'Alaskan Standard Time'  = 'America/Anchorage'
        'Hawaiian Standard Time' = 'Pacific/Honolulu'
        'GMT Standard Time'      = 'Europe/London'
        'W. Europe Standard Time'= 'Europe/Berlin'
        'Romance Standard Time'  = 'Europe/Paris'
        'Central Europe Standard Time' = 'Europe/Budapest'
        'AUS Eastern Standard Time'    = 'Australia/Sydney'
        'Tokyo Standard Time'    = 'Asia/Tokyo'
        'India Standard Time'    = 'Asia/Kolkata'
        'UTC'                    = 'UTC'
    }
    $tzMap[$WindowsTimeZoneId]
}

function Get-HarvestLocale {
    $tz = Get-TimeZone
    $locale = Get-WinSystemLocale
    $ianaTz = ConvertTo-HarvestIanaTimeZone -WindowsTimeZoneId $tz.Id

    $layout = $null
    try { $layout = (Get-WinUserLanguageList)[0].InputMethodTips[0] } catch { }

    [pscustomobject]@{
        WindowsTimeZone = $tz.Id
        IanaTimeZone    = $ianaTz
        TimeZoneMapped  = [bool]$ianaTz
        LocaleName      = $locale.Name
        KickstartLang   = ($locale.Name -replace '-', '_') + '.UTF-8'
        InputMethodTip  = $layout
    }
}

function ConvertTo-HarvestLinuxName {
    # Windows usernames allow characters Linux logins do not. Pure mapping,
    # exercised by -SelfTest with the names the dev machine can't produce.
    param([string]$WindowsName)
    $linuxName = ($WindowsName.ToLower() -replace '[^a-z0-9_-]', '')
    if ($linuxName -eq '' -or $linuxName -match '^[0-9]') { $linuxName = 'user' }
    $linuxName
}

function Get-HarvestAccount {
    $name = $env:USERNAME
    $full = $null
    try {
        $full = (Get-CimInstance Win32_UserAccount -Filter "Name='$name' AND LocalAccount=True" |
                 Select-Object -First 1).FullName
    } catch { }

    $linuxName = ConvertTo-HarvestLinuxName -WindowsName $name

    [pscustomobject]@{
        WindowsName  = $name
        FullName     = $full
        LinuxName    = $linuxName
        NameWasSafe  = ($linuxName -eq $name.ToLower())
    }
}

# =============================================================================
#  user folders
# =============================================================================

function Get-HarvestFolderStats {
    param([string]$Path, [int]$MaxFiles = 250000)

    $bytes = 0; $files = 0; $cloudOnly = 0

    # Do NOT use `break` inside ForEach-Object here. With no enclosing loop,
    # PowerShell unwinds past the function and terminates the entire script -
    # silently, with exit code 0. Select-Object -First is the supported way to
    # stop a pipeline early, and it bounds the work on pathological trees.
    try {
        Get-ChildItem -LiteralPath $Path -Recurse -File -Force -ErrorAction SilentlyContinue |
            Select-Object -First $MaxFiles |
            ForEach-Object {
                $files++
                $bytes += $_.Length
                $a = [int]$_.Attributes
                if (($a -band $FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS) -or
                    ($a -band $FILE_ATTRIBUTE_OFFLINE)) { $cloudOnly++ }
            }
    } catch { }

    [pscustomobject]@{
        Files          = $files
        Bytes          = $bytes
        CloudOnlyFiles = $cloudOnly
        Truncated      = ($files -ge $MaxFiles)
    }
}

function Get-HarvestUserFolders {
    param([switch]$SkipSizes)

    # GetFolderPath resolves OneDrive redirection correctly, which the plain
    # %USERPROFILE%\Documents guess does not.
    $targets = @(
        @{ Name='Desktop';   Path=[Environment]::GetFolderPath('Desktop') },
        @{ Name='Documents'; Path=[Environment]::GetFolderPath('MyDocuments') },
        @{ Name='Pictures';  Path=[Environment]::GetFolderPath('MyPictures') },
        @{ Name='Music';     Path=[Environment]::GetFolderPath('MyMusic') },
        @{ Name='Videos';    Path=[Environment]::GetFolderPath('MyVideos') }
    )

    # Downloads has no Environment enum member; it lives in the known-folder
    # registry under its GUID.
    try {
        $dl = (Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders' `
               -Name '{374DE290-123F-4565-9164-39C4925E467B}' -ErrorAction Stop).'{374DE290-123F-4565-9164-39C4925E467B}'
        $targets += @{ Name='Downloads'; Path=[Environment]::ExpandEnvironmentVariables($dl) }
    } catch { }

    $results = @()
    foreach ($t in $targets) {
        if (-not $t.Path -or -not (Test-Path -LiteralPath $t.Path)) {
            $results += [pscustomobject]@{
                Name = $t.Name; Path = $t.Path; Exists = $false
                IsOneDrive = $false; Files = 0; Bytes = 0
                CloudOnlyFiles = 0; Truncated = $false
            }
            continue
        }

        if (-not $SkipSizes) { Write-Step "sizing $($t.Name)..." }
        $stats = if ($SkipSizes) {
            [pscustomobject]@{ Files=0; Bytes=0; CloudOnlyFiles=0; Truncated=$false }
        } else {
            Get-HarvestFolderStats -Path $t.Path
        }

        $results += [pscustomobject]@{
            Name           = $t.Name
            Path           = $t.Path
            Exists         = $true
            IsOneDrive     = ($t.Path -match 'OneDrive')
            Files          = $stats.Files
            Bytes          = $stats.Bytes
            CloudOnlyFiles = $stats.CloudOnlyFiles
            Truncated      = $stats.Truncated
        }
    }
    $results
}

# =============================================================================
#  cloud placeholders - materialize, or refuse (RISKS R8 / V8)
# =============================================================================
#  A OneDrive "free up space" file is a placeholder: the directory entry
#  carries the full size, the data lives in the cloud, and Windows' cloud
#  files filter (cldflt) fetches it on first read. Read from Linux there is
#  no filter and no OneDrive - the file copies over empty. So while Windows
#  is alive we pin every placeholder and read it through, which makes the
#  filter fetch the bytes onto the NTFS volume where settle-in will find them.
#  Then we check, per file, that the placeholder attributes are gone and the
#  file has allocated bytes on disk. Anything short of that is a failure the
#  caller must refuse on - "probably fine" is how someone's photos arrive as
#  0-byte files.

if (-not ('Upg.NativeFile' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
namespace Upg {
    public static class NativeFile {
        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        static extern uint GetCompressedFileSizeW(string lpFileName, out uint lpFileSizeHigh);
        // Bytes the file actually occupies on the volume. A dehydrated
        // placeholder allocates nothing; a hydrated one allocates its size
        // (rounded up to the cluster). Sparse or compressed files also
        // report less than their length, which is why this is read together
        // with the attributes, not alone.
        public static long AllocatedBytes(string path) {
            uint high;
            uint low = GetCompressedFileSizeW(@"\\?\" + path, out high);
            if (low == 0xFFFFFFFF) {
                int err = Marshal.GetLastWin32Error();
                if (err != 0) throw new System.ComponentModel.Win32Exception(err);
            }
            return ((long)high << 32) | low;
        }
    }
}
'@
}

function Test-HarvestPlaceholderAttributes {
    # Pure: do these attribute bits mark a cloud-only placeholder?
    param([int]$Attributes)
    [bool](($Attributes -band $FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS) -or ($Attributes -band $FILE_ATTRIBUTE_OFFLINE))
}

function Get-HarvestPlaceholders {
    # Every cloud-only placeholder under a directory. Enumeration reads the
    # directory entries only - it does not hydrate anything.
    param([string]$Path, [int]$MaxFiles = 250000)
    $found = @()
    try {
        Get-ChildItem -LiteralPath $Path -Recurse -File -Force -ErrorAction SilentlyContinue |
            Select-Object -First $MaxFiles |
            ForEach-Object {
                if (Test-HarvestPlaceholderAttributes -Attributes ([int]$_.Attributes)) {
                    $found += [pscustomobject]@{ Path = $_.FullName; Length = $_.Length; Attributes = [int]$_.Attributes }
                }
            }
    } catch { }
    $found
}

function Test-HarvestMaterialized {
    # Pure judgment on one file's post-read facts: placeholder bits cleared,
    # every byte readable, and bytes allocated on disk (unless the file is
    # empty). All three, or it is not materialized.
    param([int]$Attributes, [long]$Length, [long]$BytesRead, [long]$AllocatedBytes)
    if (Test-HarvestPlaceholderAttributes -Attributes $Attributes) { return $false }
    if ($BytesRead -ne $Length) { return $false }
    if ($Length -gt 0 -and $AllocatedBytes -le 0) { return $false }
    $true
}

function Invoke-HarvestMaterializeFile {
    # One placeholder: pin it (attrib +P -U - "always keep on this device",
    # so the sync client will not dehydrate it again before settle-in pulls
    # it), read it through with a timeout so a stalled download cannot hang
    # the harvest, then re-read the facts and judge. Returns the per-file
    # record; never throws.
    param([string]$Path, [long]$Length, [int]$TimeoutSec = 600)
    $r = [pscustomobject]@{
        Path = $Path; Length = $Length; Pinned = $false; BytesRead = 0
        AttributesAfter = $null; AllocatedBytes = $null; Materialized = $false; Error = $null; Seconds = 0.0
    }
    $sw = [Diagnostics.Stopwatch]::StartNew()
    try {
        $null = & attrib.exe +P -U ("`"$Path`"") 2>&1
        $r.Pinned = ($LASTEXITCODE -eq 0)
    } catch { }
    $cts = New-Object System.Threading.CancellationTokenSource
    $fs = $null
    try {
        $fs = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
        $counter = New-Object Upg.CountingStream
        $task = $fs.CopyToAsync($counter, 1048576, $cts.Token)
        # Task.Wait throws an AggregateException when the read fails; the
        # inner exception carries what the filter said (a refused fetch on
        # the rig surfaced as "The cloud operation was unsuccessful").
        $done = $false
        try { $done = $task.Wait($TimeoutSec * 1000) }
        catch [System.AggregateException] {
            $inner = $_.Exception.InnerException
            if ($inner -and $inner.InnerException) { $inner = $inner.InnerException }
            $r.Error = ($inner.Message -replace '\s+', ' ').Trim()
            $done = $true
        }
        if (-not $done) {
            $cts.Cancel()
            $r.Error = "timed out after $TimeoutSec s with $($counter.Total) of $Length bytes"
        }
        $r.BytesRead = $counter.Total
    } catch {
        $e = $_.Exception
        while ($e.InnerException) { $e = $e.InnerException }
        $r.Error = ($e.Message -replace '\s+', ' ').Trim()
    } finally {
        if ($fs) { try { $fs.Dispose() } catch { } }
        $cts.Dispose()
    }
    try {
        $r.AttributesAfter = [int][IO.File]::GetAttributes($Path)
        $r.AllocatedBytes  = [Upg.NativeFile]::AllocatedBytes($Path)
        $r.Materialized = Test-HarvestMaterialized -Attributes $r.AttributesAfter -Length $Length `
                              -BytesRead $r.BytesRead -AllocatedBytes $r.AllocatedBytes
    } catch {
        if (-not $r.Error) { $r.Error = ($_.Exception.Message -replace '\s+', ' ').Trim() }
    }
    $r.Seconds = [math]::Round($sw.Elapsed.TotalSeconds, 2)
    $r
}

if (-not ('Upg.CountingStream' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.IO;
namespace Upg {
    // A sink that counts what was written to it. Reading a placeholder to
    // the end is what makes the filter fetch every byte; the count is the
    // proof that every byte arrived.
    public class CountingStream : Stream {
        public long Total;
        public override bool CanRead { get { return false; } }
        public override bool CanSeek { get { return false; } }
        public override bool CanWrite { get { return true; } }
        public override long Length { get { return Total; } }
        public override long Position { get { return Total; } set { throw new NotSupportedException(); } }
        public override void Flush() { }
        public override int Read(byte[] b, int o, int c) { throw new NotSupportedException(); }
        public override long Seek(long o, SeekOrigin s) { throw new NotSupportedException(); }
        public override void SetLength(long v) { throw new NotSupportedException(); }
        public override void Write(byte[] b, int o, int c) { Total += c; }
    }
}
'@
}

function Invoke-HarvestMaterialize {
    # Materialize every placeholder under the given directories. The summary
    # is what job.json's harvest.cloud_files carries; Failed > 0 means the
    # caller refuses to write a job at all.
    param([string[]]$Paths, [int]$TimeoutSec = 600, [int]$MaxFiles = 250000)
    $placeholders = @()
    foreach ($p in @($Paths)) {
        if ($p -and (Test-Path -LiteralPath $p)) { $placeholders += @(Get-HarvestPlaceholders -Path $p -MaxFiles $MaxFiles) }
    }
    $files = @()
    $i = 0
    foreach ($ph in $placeholders) {
        $i++
        Write-Step ("materializing {0}/{1}: {2} ({3:N0} bytes)..." -f $i, $placeholders.Count, (Split-Path $ph.Path -Leaf), $ph.Length)
        $files += Invoke-HarvestMaterializeFile -Path $ph.Path -Length $ph.Length -TimeoutSec $TimeoutSec
    }
    $ok = @($files | Where-Object { $_.Materialized })
    $bad = @($files | Where-Object { -not $_.Materialized })
    [pscustomobject]@{
        PlaceholdersFound = $placeholders.Count
        Materialized      = $ok.Count
        Failed            = $bad.Count
        Bytes             = [int64](($ok | Measure-Object -Property Length -Sum).Sum)
        Result            = if ($placeholders.Count -eq 0) { 'none-found' } elseif ($bad.Count -eq 0) { 'materialized' } else { 'refused' }
        Files             = $files
    }
}

# =============================================================================
#  wi-fi
# =============================================================================

function ConvertFrom-HarvestWlanProfileXml {
    # Parse half of the Wi-Fi harvest: one exported WLAN profile XML in, one
    # profile object out. Split from the netsh call so -SelfTest can feed it
    # fixture files - including SSIDs the dev machine's networks never cover.
    param([string]$XmlPath, [bool]$IncludeSecrets)

    $authMap = @{
        'open'      = 'none'
        'WPAPSK'    = 'wpa-psk'
        'WPA2PSK'   = 'wpa-psk'
        'WPA3SAE'   = 'sae'
        'WPA3ENT'   = 'UNSUPPORTED'
        'WPA2'      = 'UNSUPPORTED'
        'WPA'       = 'UNSUPPORTED'
    }

    # Load() honours the XML declaration's encoding. Get-Content -Raw
    # reads as ANSI on PS 5.1 and mangles any SSID with a curly quote,
    # accent or emoji - producing a profile that never connects.
    $x = New-Object System.Xml.XmlDocument
    $x.Load($XmlPath)
    $p = $x.WLANProfile
    $auth = $p.MSM.security.authEncryption.authentication
    $key  = $p.MSM.security.sharedKey.keyMaterial
    $protected = $p.MSM.security.sharedKey.protected

    $nmKeyMgmt = $authMap[$auth]
    if (-not $nmKeyMgmt) { $nmKeyMgmt = 'UNSUPPORTED' }

    [pscustomobject]@{
        Ssid            = $p.name
        Authentication  = $auth
        NmKeyMgmt       = $nmKeyMgmt
        Supported       = ($nmKeyMgmt -ne 'UNSUPPORTED')
        AutoConnect     = ($p.connectionMode -eq 'auto')
        HasSecret       = [bool]$key -and ($protected -ne 'true')
        Secret          = if ($IncludeSecrets) { $key } else { $null }
    }
}

function Get-HarvestWifi {
    param([bool]$IncludeSecrets, [bool]$IsAdmin, [string]$WorkDir)

    if ($IncludeSecrets -and -not $IsAdmin) {
        Write-Warn 'Wi-Fi passwords need Administrator. Collecting network names only.'
        $IncludeSecrets = $false
    }

    $exportDir = Join-Path $WorkDir 'wlan'
    New-Item -ItemType Directory -Path $exportDir -Force | Out-Null

    # Export as XML rather than parsing `netsh` console output, which is
    # localised - field names differ on a German or French Windows.
    # NOT $args - that is an automatic variable inside a function and splatting
    # it silently exports nothing.
    $netshArgs = @('wlan','export','profile',('folder=' + $exportDir))
    if ($IncludeSecrets) { $netshArgs += 'key=clear' }

    try {
        $null = & netsh @netshArgs 2>&1
    } catch {
        Write-Warn "netsh wlan export failed: $($_.Exception.Message)"
        return @()
    }

    $profiles = @()
    foreach ($f in (Get-ChildItem -Path $exportDir -Filter '*.xml' -ErrorAction SilentlyContinue)) {
        try {
            $profiles += ConvertFrom-HarvestWlanProfileXml -XmlPath $f.FullName -IncludeSecrets $IncludeSecrets
        } catch { }
    }

    if (-not $IncludeSecrets) {
        Remove-Item -Path $exportDir -Recurse -Force -ErrorAction SilentlyContinue
    }

    $profiles
}

# =============================================================================
#  browsers
# =============================================================================

function Get-HarvestBrowsers {
    param([switch]$SkipSizes)

    $candidates = @(
        @{ Name='Firefox'; Path=(Join-Path $env:APPDATA 'Mozilla\Firefox')
           LinuxTarget='~/.mozilla/firefox'; PasswordsPort=$true
           Note='Bookmarks, history, extensions and saved passwords all transfer. The Firefox password store is cross-platform.' }
        @{ Name='Chrome'; Path=(Join-Path $env:LOCALAPPDATA 'Google\Chrome\User Data')
           LinuxTarget='~/.config/google-chrome'; PasswordsPort=$false
           Note='Bookmarks, history and extensions transfer. Saved passwords do NOT - they are encrypted with Windows DPAPI, which has no Linux equivalent. Sign into Chrome sync or export passwords to CSV BEFORE converting.' }
        @{ Name='Edge'; Path=(Join-Path $env:LOCALAPPDATA 'Microsoft\Edge\User Data')
           LinuxTarget='~/.config/microsoft-edge'; PasswordsPort=$false
           Note='Bookmarks, history and extensions transfer. Saved passwords do NOT - Windows DPAPI encryption. Export them or enable sync BEFORE converting.' }
    )

    $found = @()
    foreach ($c in $candidates) {
        if (-not (Test-Path -LiteralPath $c.Path)) { continue }
        if (-not $SkipSizes) { Write-Step "measuring $($c.Name) profile..." }
        $stats = if ($SkipSizes) {
            [pscustomobject]@{ Files=0; Bytes=0; CloudOnlyFiles=0; Truncated=$false }
        } else {
            Get-HarvestFolderStats -Path $c.Path
        }
        $found += [pscustomobject]@{
            Name          = $c.Name
            Path          = $c.Path
            LinuxTarget   = $c.LinuxTarget
            PasswordsPort = $c.PasswordsPort
            Note          = $c.Note
            Bytes         = $stats.Bytes
            Files         = $stats.Files
        }
    }
    $found
}

# =============================================================================
#  capacity
# =============================================================================

function Get-HarvestExternalDrives {
    $drives = @()
    try {
        foreach ($d in (Get-Disk | Where-Object { $_.BusType -eq 'USB' })) {
            foreach ($p in (Get-Partition -DiskNumber $d.Number -ErrorAction SilentlyContinue)) {
                if (-not $p.DriveLetter) { continue }
                $v = Get-Volume -DriveLetter $p.DriveLetter -ErrorAction SilentlyContinue
                if (-not $v) { continue }
                $drives += [pscustomobject]@{
                    DriveLetter = $p.DriveLetter
                    Label       = $v.FileSystemLabel
                    FileSystem  = $v.FileSystemType
                    SizeBytes   = $v.Size
                    FreeBytes   = $v.SizeRemaining
                    DiskModel   = $d.FriendlyName
                }
            }
        }
    } catch { }
    $drives
}

function Get-HarvestCapacity {
    # $WindowsUsedBytes injectable for -SelfTest; omitted = read C: live.
    param($UserFolders, $Browsers, $ExternalDrives, $WindowsUsedBytes)

    $windowsUsed = $WindowsUsedBytes
    if ($null -eq $windowsUsed) {
        $c = Get-Volume -DriveLetter C -ErrorAction SilentlyContinue
        $windowsUsed = if ($c) { $c.Size - $c.SizeRemaining } else { 0 }
    }
    $userData = ($UserFolders | Measure-Object -Property Bytes -Sum).Sum
    $browserData = ($Browsers | Measure-Object -Property Bytes -Sum).Sum
    if (-not $userData) { $userData = 0 }
    if (-not $browserData) { $browserData = 0 }

    # The Windows image is the rollback path; the staged copy is what gets
    # restored into Linux. Both live on the external drive at the same time.
    $needed = [int64](($windowsUsed * 1.1) + $userData + $browserData)

    $best = $ExternalDrives | Sort-Object FreeBytes -Descending | Select-Object -First 1

    [pscustomobject]@{
        WindowsUsedBytes  = [int64]$windowsUsed
        UserDataBytes     = [int64]$userData
        BrowserDataBytes  = [int64]$browserData
        BackupNeededBytes = $needed
        BackupNeededGB    = [math]::Round($needed / 1GB, 1)
        ExternalPresent   = ($ExternalDrives.Count -gt 0)
        ExternalSufficient= if ($best) { $best.FreeBytes -ge $needed } else { $false }
        BestExternal      = $best
    }
}

# =============================================================================
#  self-test
# =============================================================================
#  Exercises the pure halves of the harvest - folder stats against a real
#  generated tree (including a genuine FILE_ATTRIBUTE_OFFLINE placeholder),
#  WLAN XML parsing against fixture profiles, the name/timezone mappings, and
#  the capacity arithmetic - none of which need this machine's real state.
#  Special characters are built from char codes so the test survives any
#  script-file encoding.

function Invoke-HarvestSelfTest {
    $t = @{ Failed = 0 }

    function Assert-H {
        param([string]$Name, [bool]$Condition, [string]$Detail = '')
        if ($Condition) {
            Write-Host ("    PASS  " + $Name) -ForegroundColor Green
        } else {
            $t.Failed++
            Write-Host ("    FAIL  " + $Name) -ForegroundColor Red
            if ($Detail) { Write-Host "          $Detail" -ForegroundColor Red }
        }
    }

    function Write-WlanFixture {
        param([string]$Path, [string]$Ssid, [string]$Auth, [string]$Key,
              [string]$Protected = 'false', [string]$Mode = 'auto')
        $esc = [Security.SecurityElement]::Escape($Ssid)
        $sec = ''
        if ($Key) {
            $sec = "<sharedKey><keyType>passPhrase</keyType><protected>$Protected</protected><keyMaterial>$([Security.SecurityElement]::Escape($Key))</keyMaterial></sharedKey>"
        }
        $xml = @"
<?xml version="1.0" encoding="UTF-8"?>
<WLANProfile xmlns="http://www.microsoft.com/networking/WLAN/profile/v1">
    <name>$esc</name>
    <SSIDConfig><SSID><name>$esc</name></SSID></SSIDConfig>
    <connectionType>ESS</connectionType>
    <connectionMode>$Mode</connectionMode>
    <MSM><security>
        <authEncryption><authentication>$Auth</authentication><encryption>AES</encryption><useOneX>false</useOneX></authEncryption>
        $sec
    </security></MSM>
</WLANProfile>
"@
        [IO.File]::WriteAllText($Path, $xml, (New-Object Text.UTF8Encoding $true))
    }

    Write-Host ''
    Write-Host '  upgrade_ harvest self-test' -ForegroundColor Cyan
    Write-Host ''

    $work = Join-Path $env:TEMP ('upgrade-harvest-selftest-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $work -Force | Out-Null
    try {
        # --- folder stats: a real generated tree --------------------------
        $tree = Join-Path $work 'tree'
        New-Item -ItemType Directory -Path (Join-Path $tree 'sub') -Force | Out-Null
        [IO.File]::WriteAllText((Join-Path $tree 'a.txt'),     ('a' * 10))
        [IO.File]::WriteAllText((Join-Path $tree 'b.txt'),     ('b' * 20))
        [IO.File]::WriteAllText((Join-Path $tree 'sub\c.txt'), ('c' * 30))

        $s = Get-HarvestFolderStats -Path $tree
        Assert-H 'folder stats: counts files and bytes recursively' `
            ($s.Files -eq 3 -and $s.Bytes -eq 60 -and -not $s.Truncated -and $s.CloudOnlyFiles -eq 0) `
            "got Files=$($s.Files) Bytes=$($s.Bytes) Truncated=$($s.Truncated) CloudOnly=$($s.CloudOnlyFiles)"

        # F1/R6 regression: the cap must stop the pipeline, flag Truncated,
        # and above all not kill the script.
        $s = Get-HarvestFolderStats -Path $tree -MaxFiles 2
        Assert-H 'folder stats: file cap truncates and says so (F1/R6)' `
            ($s.Files -eq 2 -and $s.Truncated) `
            "got Files=$($s.Files) Truncated=$($s.Truncated)"

        # R8 detection arm: FILE_ATTRIBUTE_OFFLINE set on a real NTFS file -
        # the genuine attribute, not a fabricated object. (The cloud filter's
        # RECALL_ON_DATA_ACCESS twin still needs a machine with real
        # placeholders; that residue stays open in R8.)
        $ph = Join-Path $tree 'placeholder.txt'
        [IO.File]::WriteAllText($ph, 'x')
        [IO.File]::SetAttributes($ph, ([IO.File]::GetAttributes($ph) -bor [IO.FileAttributes]::Offline))
        $s = Get-HarvestFolderStats -Path $tree
        Assert-H 'folder stats: offline-attribute file counts as cloud-only (R8)' `
            ($s.CloudOnlyFiles -eq 1) `
            "got CloudOnly=$($s.CloudOnlyFiles)"

        # --- materialization judgment (R8 / V8): the pure half ------------
        $RECALL = 0x400000; $OFFLINE = 0x1000; $ARCHIVE = 0x20; $PINNED = 0x80000
        Assert-H 'materialize: recall-on-data-access bit is a placeholder' `
            (Test-HarvestPlaceholderAttributes -Attributes ($ARCHIVE -bor $RECALL))
        Assert-H 'materialize: offline bit is a placeholder' `
            (Test-HarvestPlaceholderAttributes -Attributes ($ARCHIVE -bor $OFFLINE))
        Assert-H 'materialize: pinned alone is not a placeholder (V8: pin bits are not detection)' `
            (-not (Test-HarvestPlaceholderAttributes -Attributes ($ARCHIVE -bor $PINNED)))
        Assert-H 'materialize: bits cleared + every byte read + allocated is materialized' `
            (Test-HarvestMaterialized -Attributes ($ARCHIVE -bor $PINNED) -Length 1000 -BytesRead 1000 -AllocatedBytes 4096)
        Assert-H 'materialize: recall bit still set is NOT materialized, whatever was read' `
            (-not (Test-HarvestMaterialized -Attributes ($ARCHIVE -bor $RECALL) -Length 1000 -BytesRead 1000 -AllocatedBytes 4096))
        Assert-H 'materialize: a short read is NOT materialized' `
            (-not (Test-HarvestMaterialized -Attributes $ARCHIVE -Length 1000 -BytesRead 999 -AllocatedBytes 4096))
        Assert-H 'materialize: zero bytes allocated for a non-empty file is NOT materialized' `
            (-not (Test-HarvestMaterialized -Attributes $ARCHIVE -Length 1000 -BytesRead 1000 -AllocatedBytes 0))
        Assert-H 'materialize: an empty file needs no allocation' `
            (Test-HarvestMaterialized -Attributes $ARCHIVE -Length 0 -BytesRead 0 -AllocatedBytes 0)

        # allocated-bytes read against real files: a plain file allocates,
        # and the read goes through the same P/Invoke the judgment uses
        $alloc = [Upg.NativeFile]::AllocatedBytes((Join-Path $tree 'a.txt'))
        Assert-H 'materialize: a real 10-byte file reports allocated bytes > 0' ($alloc -gt 0) "got $alloc"

        # the per-file materializer on an ordinary (non-placeholder) file:
        # reads it through, judges it materialized, touches nothing else
        $mf = Invoke-HarvestMaterializeFile -Path (Join-Path $tree 'b.txt') -Length 20 -TimeoutSec 30
        Assert-H 'materialize: an ordinary file passes through as materialized' `
            ($mf.Materialized -and $mf.BytesRead -eq 20 -and -not $mf.Error) "got read=$($mf.BytesRead) err=$($mf.Error)"
        $mf = Invoke-HarvestMaterializeFile -Path (Join-Path $tree 'missing.txt') -Length 5 -TimeoutSec 30
        Assert-H 'materialize: a vanished file is a failure with the reason, not a crash' `
            (-not $mf.Materialized -and $mf.Error) "got err=$($mf.Error)"
        # the OFFLINE attribute set by hand (not by a filter) is a placeholder
        # the read cannot clear: the run must report it as NOT materialized.
        # This is the refuse arm - a stub nothing will fill must stop the job.
        $mf = Invoke-HarvestMaterializeFile -Path $ph -Length 1 -TimeoutSec 30
        Assert-H 'materialize: a stub no provider will fill is reported failed (refuse arm)' `
            (-not $mf.Materialized) "got materialized=$($mf.Materialized) attrs=$($mf.AttributesAfter)"
        $ms = Invoke-HarvestMaterialize -Paths @($tree) -TimeoutSec 30
        Assert-H 'materialize: summary counts the stub as found and failed, result refused' `
            ($ms.PlaceholdersFound -eq 1 -and $ms.Failed -eq 1 -and $ms.Result -eq 'refused') "got found=$($ms.PlaceholdersFound) failed=$($ms.Failed) result=$($ms.Result)"

        # --- WLAN profile XML parsing -------------------------------------
        $curly = 'Addison' + [char]0x2019 + 's iPhone'           # curly apostrophe (F3)
        $emoji = 'Caf' + [char]0xE9 + ' ' + [char]::ConvertFromUtf32(0x2615) + ' 5G'

        $f = Join-Path $work 'p1.xml'
        Write-WlanFixture -Path $f -Ssid $curly -Auth 'WPA2PSK' -Key 'hunter2'
        $r = ConvertFrom-HarvestWlanProfileXml -XmlPath $f -IncludeSecrets $false
        Assert-H 'wlan: curly-quote SSID survives byte-for-byte (F3)' `
            ($r.Ssid -ceq $curly) "got '$($r.Ssid)'"
        Assert-H 'wlan: WPA2PSK maps to wpa-psk, supported, autoconnect' `
            ($r.NmKeyMgmt -eq 'wpa-psk' -and $r.Supported -and $r.AutoConnect -and $r.HasSecret) `
            "got NmKeyMgmt=$($r.NmKeyMgmt) Supported=$($r.Supported)"
        Assert-H 'wlan: secrets stay out of the output unless asked for' `
            ($null -eq $r.Secret)
        $r = ConvertFrom-HarvestWlanProfileXml -XmlPath $f -IncludeSecrets $true
        Assert-H 'wlan: -IncludeWifiSecrets carries the key through' `
            ($r.Secret -ceq 'hunter2')

        $f = Join-Path $work 'p2.xml'
        Write-WlanFixture -Path $f -Ssid $emoji -Auth 'WPA3SAE' -Key 'espresso'
        $r = ConvertFrom-HarvestWlanProfileXml -XmlPath $f -IncludeSecrets $false
        Assert-H 'wlan: accented + emoji SSID survives, WPA3 maps to sae' `
            ($r.Ssid -ceq $emoji -and $r.NmKeyMgmt -eq 'sae' -and $r.Supported) `
            "got '$($r.Ssid)' NmKeyMgmt=$($r.NmKeyMgmt)"

        $f = Join-Path $work 'p3.xml'
        Write-WlanFixture -Path $f -Ssid 'CoffeeShopOpen' -Auth 'open' -Key '' -Mode 'manual'
        $r = ConvertFrom-HarvestWlanProfileXml -XmlPath $f -IncludeSecrets $false
        Assert-H 'wlan: open network - no secret, manual connect, supported' `
            ($r.NmKeyMgmt -eq 'none' -and $r.Supported -and -not $r.HasSecret -and -not $r.AutoConnect)

        $f = Join-Path $work 'p4.xml'
        Write-WlanFixture -Path $f -Ssid 'CorpNet' -Auth 'WPA2' -Key ''
        $r = ConvertFrom-HarvestWlanProfileXml -XmlPath $f -IncludeSecrets $false
        Assert-H 'wlan: enterprise 802.1X is flagged unsupported, not guessed' `
            ($r.NmKeyMgmt -eq 'UNSUPPORTED' -and -not $r.Supported)

        $f = Join-Path $work 'p5.xml'
        Write-WlanFixture -Path $f -Ssid 'HomeNet' -Auth 'WPA2PSK' -Key 'AQAAANCM...' -Protected 'true'
        $r = ConvertFrom-HarvestWlanProfileXml -XmlPath $f -IncludeSecrets $false
        Assert-H 'wlan: DPAPI-protected key does not count as a usable secret' `
            (-not $r.HasSecret)

        # --- linux login name mapping -------------------------------------
        Assert-H 'account: simple name lowercases and stays safe' `
            ((ConvertTo-HarvestLinuxName 'Addison') -ceq 'addison')
        Assert-H 'account: space in name is stripped' `
            ((ConvertTo-HarvestLinuxName 'John Smith') -ceq 'johnsmith')
        Assert-H 'account: leading digit falls back to user' `
            ((ConvertTo-HarvestLinuxName '3cool') -ceq 'user')
        Assert-H 'account: fully non-latin name falls back to user' `
            ((ConvertTo-HarvestLinuxName ([string][char]0x7B2C + [char]0x4E94)) -ceq 'user')
        Assert-H 'account: dash and underscore survive' `
            ((ConvertTo-HarvestLinuxName 'a-b_c') -ceq 'a-b_c')

        # --- timezone mapping ---------------------------------------------
        Assert-H 'locale: known Windows timezone maps to IANA' `
            ((ConvertTo-HarvestIanaTimeZone 'Eastern Standard Time') -eq 'America/New_York')
        Assert-H 'locale: unknown timezone returns nothing rather than a guess' `
            ($null -eq (ConvertTo-HarvestIanaTimeZone 'Nepal Standard Time'))

        # --- capacity arithmetic ------------------------------------------
        $gb = 1073741824
        $folders  = @([pscustomobject]@{ Bytes = 10 * $gb })
        $browsers = @([pscustomobject]@{ Bytes = 2 * $gb })
        $bigDrive   = [pscustomobject]@{ DriveLetter='E'; FreeBytes = 200 * $gb }
        $smallDrive = [pscustomobject]@{ DriveLetter='F'; FreeBytes = 50 * $gb }

        $c = Get-HarvestCapacity -UserFolders $folders -Browsers $browsers `
                                 -ExternalDrives @($smallDrive, $bigDrive) -WindowsUsedBytes (100 * $gb)
        Assert-H 'capacity: needed = 1.1x windows + user + browser data' `
            ($c.BackupNeededGB -eq 122) "got $($c.BackupNeededGB) GB"
        Assert-H 'capacity: picks the largest drive and calls it sufficient' `
            ($c.ExternalSufficient -and $c.BestExternal.DriveLetter -eq 'E')

        $c = Get-HarvestCapacity -UserFolders $folders -Browsers $browsers `
                                 -ExternalDrives @($smallDrive) -WindowsUsedBytes (100 * $gb)
        Assert-H 'capacity: a too-small drive is not sufficient' `
            ($c.ExternalPresent -and -not $c.ExternalSufficient)

        $c = Get-HarvestCapacity -UserFolders $folders -Browsers $browsers `
                                 -ExternalDrives @() -WindowsUsedBytes (100 * $gb)
        Assert-H 'capacity: no external drive is reported plainly' `
            (-not $c.ExternalPresent -and -not $c.ExternalSufficient)
    } finally {
        Remove-Item -Path $work -Recurse -Force -ErrorAction SilentlyContinue
    }

    Write-Host ''
    if ($t.Failed -eq 0) {
        Write-Host '  all checks passed' -ForegroundColor Green
        Write-Host ''
        exit 0
    }
    Write-Host "  $($t.Failed) failed" -ForegroundColor Red
    Write-Host ''
    exit 1
}

# =============================================================================
#  main
# =============================================================================

if ($SelfTest) { Invoke-HarvestSelfTest }

if ($MaterializePath) {
    # The V8 seam: materialize one directory, report per file, exit non-zero
    # on any failure. Run by Test-Materialize.ps1 in its own process.
    if (-not $MaterializeResult) { throw '-MaterializePath needs -MaterializeResult <file.json>' }
    $m = Invoke-HarvestMaterialize -Paths @($MaterializePath) -TimeoutSec $MaterializeTimeoutSec
    $m | ConvertTo-Json -Depth 5 | Out-File -FilePath $MaterializeResult -Encoding UTF8
    Write-Host ("  placeholders {0}, materialized {1}, failed {2} -> {3}" -f $m.PlaceholdersFound, $m.Materialized, $m.Failed, $m.Result)
    if ($m.Failed -gt 0) { exit 3 }
    exit 0
}

$isAdmin = Test-HarvestAdmin

if (-not $OutDir) {
    $OutDir = Join-Path ([Environment]::GetFolderPath('Desktop')) 'upgrade-state'
}
if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Path $OutDir -Force | Out-Null }

Write-Host ''
Write-Host '  upgrade_  harvest' -ForegroundColor Cyan
Write-Host '  read-only: this collects information and changes nothing' -ForegroundColor DarkGray
Write-Host ''

Write-Step 'identity...'
$identity = Get-HarvestIdentity

Write-Step 'locale...'
$locale = Get-HarvestLocale
$account = Get-HarvestAccount

$userFolders = Get-HarvestUserFolders -SkipSizes:$SkipSizes
$browsers    = Get-HarvestBrowsers -SkipSizes:$SkipSizes

# Materialize cloud placeholders (RISKS R8 / V8), then size again: the folder
# map that reaches the stick must describe files that are actually on disk.
$cloudFiles = [pscustomobject]@{ PlaceholdersFound = (@($userFolders | Measure-Object -Property CloudOnlyFiles -Sum).Sum)
                                 Materialized = 0; Failed = 0; Bytes = 0; Result = 'not-attempted'; Files = @() }
if (-not $cloudFiles.PlaceholdersFound) { $cloudFiles.PlaceholdersFound = 0 }
if ($Materialize) {
    Write-Step 'cloud placeholders...'
    $cloudFiles = Invoke-HarvestMaterialize -Paths @($userFolders | Where-Object { $_.Exists } | ForEach-Object { $_.Path }) `
                                            -TimeoutSec $MaterializeTimeoutSec
    if ($cloudFiles.PlaceholdersFound -gt 0) { $userFolders = Get-HarvestUserFolders -SkipSizes:$SkipSizes }
} elseif ($cloudFiles.PlaceholdersFound -eq 0) {
    $cloudFiles.Result = 'none-found'
}

Write-Step 'wi-fi profiles...'
$wifi = Get-HarvestWifi -IncludeSecrets:$IncludeWifiSecrets.IsPresent -IsAdmin $isAdmin -WorkDir $OutDir

Write-Step 'external drives...'
$external = Get-HarvestExternalDrives
$capacity = Get-HarvestCapacity -UserFolders $userFolders -Browsers $browsers -ExternalDrives $external

$state = [pscustomobject]@{
    HarvestVersion  = $HarvestVersion
    HarvestedUtc    = (Get-Date).ToUniversalTime().ToString('o')
    RanAsAdmin      = $isAdmin
    SecretsIncluded = ($IncludeWifiSecrets.IsPresent -and $isAdmin)
    Identity        = $identity
    Locale          = $locale
    Account         = $account
    UserFolders     = $userFolders
    CloudFiles      = [pscustomobject]@{
        PlaceholdersFound = $cloudFiles.PlaceholdersFound
        Materialized      = $cloudFiles.Materialized
        Failed            = $cloudFiles.Failed
        Bytes             = $cloudFiles.Bytes
        Result            = $cloudFiles.Result
        FailedFiles       = @($cloudFiles.Files | Where-Object { -not $_.Materialized } | ForEach-Object { [pscustomobject]@{ Path = $_.Path; Error = $_.Error } })
    }
    Browsers        = $browsers
    WifiProfiles    = $wifi
    ExternalDrives  = $external
    Capacity        = $capacity
}

$statePath = Join-Path $OutDir 'state.json'
$state | ConvertTo-Json -Depth 8 | Out-File -FilePath $statePath -Encoding UTF8

# --- summary ------------------------------------------------------------

Write-Host ''
Write-Host '  ---------------------------------------------------------------' -ForegroundColor DarkGray
Write-Host "  $($identity.Vendor) $($identity.Model)"
Write-Host "  user: $($account.WindowsName) -> linux login '$($account.LinuxName)'"
Write-Host "  timezone: $($locale.WindowsTimeZone) -> $(if($locale.IanaTimeZone){$locale.IanaTimeZone}else{'UNMAPPED'})"
Write-Host ''

foreach ($f in $userFolders) {
    if (-not $f.Exists) { continue }
    $gb = [math]::Round($f.Bytes / 1GB, 2)
    $line = "  {0,-10} {1,8} GB  {2,6} files" -f $f.Name, $gb, $f.Files
    Write-Host $line
    if ($f.CloudOnlyFiles -gt 0) {
        Write-Warn "$($f.CloudOnlyFiles) files in $($f.Name) are cloud-only placeholders - they would copy as EMPTY. Re-run with -Materialize to download them, or make them available offline first."
    }
    if ($f.Truncated) {
        Write-Warn "$($f.Name) was too large to size within the time limit; the backup estimate is low."
    }
}

if ($cloudFiles.Result -eq 'materialized') {
    Write-Host ("  cloud files: {0} placeholders made local ({1:N1} GB)" -f $cloudFiles.Materialized, ($cloudFiles.Bytes / 1GB)) -ForegroundColor Green
} elseif ($cloudFiles.Result -eq 'refused') {
    Write-Warn ("REFUSED: {0} of {1} cloud-only files could not be made local. They would arrive EMPTY on Linux, so no conversion can be written from this harvest. Check that OneDrive is running and signed in, then re-run." -f $cloudFiles.Failed, $cloudFiles.PlaceholdersFound)
    foreach ($ff in ($cloudFiles.Files | Where-Object { -not $_.Materialized } | Select-Object -First 10)) {
        Write-Host ("      {0}: {1}" -f $ff.Path, $ff.Error) -ForegroundColor Yellow
    }
}

Write-Host ''
foreach ($b in $browsers) {
    Write-Host ("  {0,-10} {1,8} GB" -f $b.Name, [math]::Round($b.Bytes/1GB,2))
    if (-not $b.PasswordsPort) {
        Write-Warn "$($b.Name) saved passwords will NOT transfer. Export them or turn on sync before converting."
    }
}

Write-Host ''
$supported = @($wifi | Where-Object { $_.Supported })
$unsupported = @($wifi | Where-Object { -not $_.Supported })
Write-Host "  wi-fi: $($supported.Count) networks transferable"
if ($unsupported.Count -gt 0) {
    Write-Warn "$($unsupported.Count) enterprise/802.1X networks cannot be migrated automatically."
}
if (-not $state.SecretsIncluded) {
    Write-Host '         (names only - re-run as Administrator with -IncludeWifiSecrets' -ForegroundColor DarkGray
    Write-Host '          to capture passwords)' -ForegroundColor DarkGray
}

Write-Host ''
Write-Host "  backup needed: $($capacity.BackupNeededGB) GB" -ForegroundColor Cyan
if (-not $capacity.ExternalPresent) {
    Write-Warn 'No external USB drive detected. The converter requires one and will refuse to run without it.'
} elseif (-not $capacity.ExternalSufficient) {
    Write-Warn "Largest external drive has $([math]::Round($capacity.BestExternal.FreeBytes/1GB,1)) GB free - not enough."
} else {
    Write-Host "  external drive $($capacity.BestExternal.DriveLetter): is large enough." -ForegroundColor Green
}

Write-Host ''
Write-Host "  state written: $statePath" -ForegroundColor Cyan
if ($state.SecretsIncluded) {
    Write-Host ''
    Write-Warn 'This file contains Wi-Fi passwords in cleartext. Anyone holding it can join your network. It is deleted automatically at the end of a conversion.'
}
Write-Host ''
