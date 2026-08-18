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

.EXAMPLE
    .\Harvest-UpgradeState.ps1

.EXAMPLE
    .\Harvest-UpgradeState.ps1 -IncludeWifiSecrets -OutDir E:\upgrade
#>
[CmdletBinding()]
param(
    [string]$OutDir,
    [switch]$IncludeWifiSecrets,
    [switch]$SkipSizes
)

$ErrorActionPreference = 'Stop'
$HarvestVersion = '0.1.0'

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

function Get-HarvestLocale {
    $tz = Get-TimeZone

    # Windows time zone IDs are not IANA IDs. Only a handful matter in
    # practice; anything unmapped falls back and the installer asks.
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

    $locale = Get-WinSystemLocale
    $ianaTz = $tzMap[$tz.Id]

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

function Get-HarvestAccount {
    $name = $env:USERNAME
    $full = $null
    try {
        $full = (Get-CimInstance Win32_UserAccount -Filter "Name='$name' AND LocalAccount=True" |
                 Select-Object -First 1).FullName
    } catch { }

    # Windows usernames allow characters Linux logins do not.
    $linuxName = ($name.ToLower() -replace '[^a-z0-9_-]', '')
    if ($linuxName -eq '' -or $linuxName -match '^[0-9]') { $linuxName = 'user' }

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
#  wi-fi
# =============================================================================

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

    $authMap = @{
        'open'      = 'none'
        'WPAPSK'    = 'wpa-psk'
        'WPA2PSK'   = 'wpa-psk'
        'WPA3SAE'   = 'sae'
        'WPA3ENT'   = 'UNSUPPORTED'
        'WPA2'      = 'UNSUPPORTED'
        'WPA'       = 'UNSUPPORTED'
    }

    $profiles = @()
    foreach ($f in (Get-ChildItem -Path $exportDir -Filter '*.xml' -ErrorAction SilentlyContinue)) {
        try {
            # Load() honours the XML declaration's encoding. Get-Content -Raw
            # reads as ANSI on PS 5.1 and mangles any SSID with a curly quote,
            # accent or emoji - producing a profile that never connects.
            $x = New-Object System.Xml.XmlDocument
            $x.Load($f.FullName)
            $p = $x.WLANProfile
            $auth = $p.MSM.security.authEncryption.authentication
            $key  = $p.MSM.security.sharedKey.keyMaterial
            $protected = $p.MSM.security.sharedKey.protected

            $nmKeyMgmt = $authMap[$auth]
            if (-not $nmKeyMgmt) { $nmKeyMgmt = 'UNSUPPORTED' }

            $profiles += [pscustomobject]@{
                Ssid            = $p.name
                Authentication  = $auth
                NmKeyMgmt       = $nmKeyMgmt
                Supported       = ($nmKeyMgmt -ne 'UNSUPPORTED')
                AutoConnect     = ($p.connectionMode -eq 'auto')
                HasSecret       = [bool]$key -and ($protected -ne 'true')
                Secret          = if ($IncludeSecrets) { $key } else { $null }
            }
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
    param($UserFolders, $Browsers, $ExternalDrives)

    $c = Get-Volume -DriveLetter C -ErrorAction SilentlyContinue
    $windowsUsed = if ($c) { $c.Size - $c.SizeRemaining } else { 0 }
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
#  main
# =============================================================================

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
        Write-Warn "$($f.CloudOnlyFiles) files in $($f.Name) are cloud-only placeholders - they would copy as EMPTY. Make them available offline first."
    }
    if ($f.Truncated) {
        Write-Warn "$($f.Name) was too large to size within the time limit; the backup estimate is low."
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
