# Pull arm (V3, V8; R8, R19, R20) - Windows side (guest, PowerShell Direct,
# ELEVATED). Windows PowerShell 5.1. Rig tooling, not product.
#
# Plants a test tree in the signed-in rig account's own Documents and
# Pictures, with every kind of entry settle-in's pull must sort right:
# ordinary files (odd names, an empty one, a 40 MB one, nested folders), a
# hidden desktop.ini, a junction to C:\Windows, a file symlink, and under a
# Cloud Files sync root (the API OneDrive is built on; the provider is the
# one VALIDATION V8's cfapi leg used, evaluate/windows/Test-Materialize.ps1,
# git 20d9a10^; guest/cfprovider.cs) two dehydrated placeholders and two downloaded (one in a subfolder)
# by reading them (the cloud tag stays, the recall bit goes). Then a Chrome
# profile's Bookmarks and a Firefox bookmark backup. Then it runs the real
# harvester (upgrade-harvest folder-map), so the counts the pull holds
# itself against are the product's own, and writes, to OEMDRV:\v3\pull\:
#   expected.json  what must be copied (sha256, size, path), what must not
#   folder-map.json  the harvester's answer
# then a FULL shutdown. Expects upgrade-harvest.exe and firefox.jsonlz4 in
# OEMDRV:\v3\ (put there by v3.sh pull-oemdrv).
param([switch]$NoShutdown)
$ErrorActionPreference = 'Stop'
function Log($m) { Write-Host ('{0:HH:mm:ss} {1}' -f (Get-Date), $m) }

$vol = Get-Volume | Where-Object { $_.FileSystemLabel -eq 'OEMDRV' } | Select-Object -First 1
if (-not $vol) { throw 'OEMDRV volume not found' }
if (-not $vol.DriveLetter) {
    $part = Get-Partition | Where-Object { $_.AccessPaths -contains $vol.Path } | Select-Object -First 1
    $part | Add-PartitionAccessPath -AssignDriveLetter | Out-Null; Start-Sleep -Seconds 2
    $vol = Get-Volume | Where-Object { $_.FileSystemLabel -eq 'OEMDRV' } | Select-Object -First 1
}
$oem = "$($vol.DriveLetter):\v3"
$out = Join-Path $oem 'pull'
New-Item -ItemType Directory -Force -Path $out | Out-Null

$prof = $env:USERPROFILE
$docs = [Environment]::GetFolderPath('MyDocuments')
$pics = [Environment]::GetFolderPath('MyPictures')
Log "profile $prof; Documents $docs; Pictures $pics"
$plant = Join-Path $docs 'pull-test'
$pplant = Join-Path $pics 'pull-test'
# an earlier plant: its sync root unregistered first (the filter can block deletes under a registered root)
if (Test-Path -LiteralPath (Join-Path $plant 'Cloud')) {
    Add-Type -TypeDefinition 'using System.Runtime.InteropServices; public static class CfU { [DllImport("cldapi.dll", CharSet = CharSet.Unicode)] public static extern int CfUnregisterSyncRoot(string path); }'
    Log ('earlier plant: CfUnregisterSyncRoot hr=0x{0:X8}' -f [CfU]::CfUnregisterSyncRoot((Join-Path $plant 'Cloud')))
}
foreach ($d in @($plant, $pplant)) { if (Test-Path -LiteralPath $d) { cmd /c "rmdir /s /q `"$d`"" | Out-Null } }
foreach ($d in @($plant, $pplant)) { if (Test-Path -LiteralPath $d) { throw "the earlier plant $d could not be removed" } }
New-Item -ItemType Directory -Force -Path (Join-Path $plant 'Taxes\2025'), (Join-Path $plant 'Fotos de la playa') , $pplant | Out-Null

$rng = New-Object System.Random 20261107
function Bytes([int]$n) { $b = New-Object byte[] $n; $rng.NextBytes($b); return ,$b }
$files = [ordered]@{
    'pull-test\letter.txt'                        = [Text.Encoding]::UTF8.GetBytes("Dear friend,`r`nthe files came home.`r`n")
    'pull-test\empty.txt'                         = (New-Object byte[] 0)
    'pull-test\Taxes\2025\return.pdf'             = (Bytes 250000)
    ('pull-test\Fotos de la playa\caf' + [char]0xE9 + ' ' + [char]0x00F1 + '.jpg') = (Bytes 3000000)
    'pull-test\big.bin'                           = (Bytes 40000000)
    'pull-test\name with spaces & (brackets).docx'= (Bytes 12345)
}
foreach ($k in $files.Keys) { [IO.File]::WriteAllBytes((Join-Path $docs $k), $files[$k]) }
[IO.File]::WriteAllBytes((Join-Path $pplant 'sunset.png'), (Bytes 1000000))
# Windows' own folder settings: hidden and system, never copied
Set-Content -LiteralPath (Join-Path $plant 'desktop.ini') -Value "[.ShellClassInfo]" -Encoding ASCII
attrib +h +s (Join-Path $plant 'desktop.ini')
# links: never followed
cmd /c "mklink /J `"$(Join-Path $plant 'LinkToWindows')`" C:\Windows" | Out-Null
New-Item -ItemType SymbolicLink -Path (Join-Path $plant 'letter-link.txt') -Target (Join-Path $plant 'letter.txt') | Out-Null

# the expected copies, hashed BEFORE any placeholder exists (a placeholder is never read here)
function Sha($p) { (Get-FileHash -Algorithm SHA256 -LiteralPath $p).Hash.ToLower() }
$expect = @()
foreach ($k in $files.Keys) { $p = Join-Path $docs $k; $expect += [ordered]@{ folder = 'Documents'; rel = ($k -replace '\\', '/'); size = (Get-Item -LiteralPath $p).Length; sha256 = (Sha $p) } }
$expect += [ordered]@{ folder = 'Pictures'; rel = 'pull-test/sunset.png'; size = 1000000; sha256 = (Sha (Join-Path $pplant 'sunset.png')) }

# --- the Cloud Files placeholders (the V8 provider, guest/cfprovider.cs)
$CfSource = [IO.File]::ReadAllText((Join-Path $PSScriptRoot 'cfprovider.cs'))
Add-Type -TypeDefinition $CfSource -Language CSharp
$root = Join-Path $plant 'Cloud'
New-Item -ItemType Directory -Force -Path $root | Out-Null
$prov = New-Object UpgV8.Provider
$cloudBytes = @{ 1 = (Bytes 70000); 2 = (Bytes 5000); 3 = (Bytes 123456); 4 = (Bytes 9000) }
$names = New-Object 'System.Collections.Generic.Dictionary[int,string]'
$names[1] = 'only-in-cloud.docx'; $names[2] = 'Sub\also-in-cloud.xlsx'; $names[3] = 'downloaded.pdf'; $names[4] = 'Sub\downloaded-too.txt'
foreach ($k in $cloudBytes.Keys) { $prov.Files[$k] = $cloudBytes[$k] }
$prov.Register($root)
$prov.Connect()
$prov.CreateDirectoryPlaceholder('Sub')
$prov.CreatePlaceholders($names) | Out-Null
# two downloaded by reading them through the filter (one in a OneDrive subfolder): their bytes are now on the disk
foreach ($id in @(3, 4)) {
    $hydrated = Join-Path $root $names[$id]
    # read in another process, as the V8 leg did: the filter calls this process back for the bytes
    $hsha = (powershell.exe -NoProfile -Command "(Get-FileHash -Algorithm SHA256 -LiteralPath '$hydrated').Hash").Trim().ToLower()
    if ($hsha -ne ([BitConverter]::ToString((New-Object Security.Cryptography.SHA256Managed).ComputeHash($cloudBytes[$id])) -replace '-', '').ToLower()) { throw "the downloaded placeholder $($names[$id]) does not hash to its bytes" }
    $expect += [ordered]@{ folder = 'Documents'; rel = ('pull-test/Cloud/' + ($names[$id] -replace '\\', '/')); size = $cloudBytes[$id].Length; sha256 = $hsha }
}
# disconnect, never unregister: the two others stay real placeholders, as OneDrive leaves them
$prov.Log | ForEach-Object { Log "  cf: $_" }
$attrs = @{}
foreach ($n in @('only-in-cloud.docx', 'Sub\also-in-cloud.xlsx', 'downloaded.pdf', 'Sub\downloaded-too.txt')) { $attrs[$n] = ('0x{0:X}' -f [int](Get-Item -LiteralPath (Join-Path $root $n) -Force).Attributes.value__) }
Log ("attributes: " + (($attrs.GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" }) -join ', '))

# --- bookmarks: a Chrome profile, a Firefox backup (host-made, firefox.jsonlz4)
$cd = Join-Path $env:LOCALAPPDATA 'Google\Chrome\User Data\Default'
New-Item -ItemType Directory -Force -Path $cd | Out-Null
$chrome = '{"roots":{"bookmark_bar":{"type":"folder","name":"Bookmarks bar","children":[{"type":"url","name":"Bank","url":"https://bank.example/"},{"type":"url","name":"Mail","url":"https://mail.example/"},{"type":"folder","name":"Recipes","children":[{"type":"url","name":"Soup","url":"https://soup.example/"},{"type":"url","name":"Bread","url":"https://bread.example/"}]}]},"other":{"type":"folder","name":"Other bookmarks","children":[{"type":"url","name":"News","url":"https://news.example/"}]},"synced":{"type":"folder","name":"Mobile bookmarks","children":[]}},"version":1}'
[IO.File]::WriteAllText((Join-Path $cd 'Bookmarks'), $chrome)
$fd = Join-Path $env:APPDATA 'Mozilla\Firefox\Profiles\pulltest.default-release\bookmarkbackups'
New-Item -ItemType Directory -Force -Path $fd | Out-Null
Copy-Item -LiteralPath (Join-Path $oem 'firefox.jsonlz4') -Destination (Join-Path $fd 'bookmarks-2026-11-07_3_pulltest.jsonlz4') -Force

# --- the product's own count
$fm = Join-Path $out 'folder-map.json'
& (Join-Path $oem 'upgrade-harvest.exe') folder-map --out $fm
Log "harvester exit $LASTEXITCODE"
$bl = Get-CimInstance -Namespace root\cimv2\Security\MicrosoftVolumeEncryption -ClassName Win32_EncryptableVolume -Filter "DriveLetter='C:'"
$doc = [ordered]@{
    planted_utc = (Get-Date).ToUniversalTime().ToString('o')
    windows_build = [Environment]::OSVersion.Version.ToString()
    profile = $prof; documents = $docs; pictures = $pics
    copy = $expect
    placeholders = @('pull-test/Cloud/only-in-cloud.docx', 'pull-test/Cloud/Sub/also-in-cloud.xlsx')
    not_copied = @('pull-test/desktop.ini', 'pull-test/LinkToWindows', 'pull-test/letter-link.txt')
    attributes = $attrs
    bookmarks = @{ Chrome = 5; Firefox = 3 }
    bitlocker = @{ protection = $bl.ProtectionStatus; conversion = $bl.ConversionStatus }
}
[IO.File]::WriteAllText((Join-Path $out 'expected.json'), ($doc | ConvertTo-Json -Depth 6), (New-Object Text.UTF8Encoding $false))
Log "wrote $out"
if (-not $NoShutdown) { Log 'full shutdown'; shutdown /s /t 5 /f }
