# One click test, the file copy (V3; R8, R19, R20) - Windows side. Windows
# PowerShell 5.1. Rig tooling, not product. Run by the test manager
# (rig/oneclick, stage plant-files) as the account the conversion is for,
# BEFORE the job is written, so the product's own harvest counts these files.
#
# Plants a test tree in that account's own Documents and Pictures with the
# kinds of entry settle-in's pull must sort right: ordinary files (odd names,
# an empty one, a 40 MB one, nested folders), a hidden system desktop.ini, a
# junction to C:\Windows and a file symlink (never followed, never copied),
# then a Chrome profile's Bookmarks and, when -Firefox names one, a Firefox
# bookmark backup. It is rig/hyperv/guest/pull-plant.ps1 without the Cloud
# Files placeholders: files that live only in OneDrive are NOT planted here
# (R43's residue, a signed-in OneDrive on a real disk, stays owed).
#
# Writes -Out\expected.json: what must arrive (sha256, size, path) and what
# must not. An earlier plant is removed first, so the stage can run again.
param([string]$Out = 'C:\upgrade_oneclick\plant', [string]$Firefox = '')
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$docs = [Environment]::GetFolderPath('MyDocuments')
$pics = [Environment]::GetFolderPath('MyPictures')
if (-not $docs -or -not $pics) { throw 'this account has no Documents or Pictures folder' }
$plant = Join-Path $docs 'pull-test'
$pplant = Join-Path $pics 'pull-test'
foreach ($d in @($plant, $pplant)) { if (Test-Path -LiteralPath $d) { cmd /c "rmdir /s /q `"$d`"" | Out-Null } }
foreach ($d in @($plant, $pplant)) { if (Test-Path -LiteralPath $d) { throw "the earlier plant $d could not be removed" } }
New-Item -ItemType Directory -Force -Path (Join-Path $plant 'Taxes\2025'), (Join-Path $plant 'Fotos de la playa'), $pplant | Out-Null

$rng = New-Object System.Random 20261118
function Bytes([int]$n) { $b = New-Object byte[] $n; $rng.NextBytes($b); return ,$b }
$files = [ordered]@{
    'pull-test\letter.txt'                         = [Text.Encoding]::UTF8.GetBytes("Dear friend,`r`nthe files came home.`r`n")
    'pull-test\empty.txt'                          = (New-Object byte[] 0)
    'pull-test\Taxes\2025\return.pdf'              = (Bytes 250000)
    ('pull-test\Fotos de la playa\caf' + [char]0xE9 + ' ' + [char]0x00F1 + '.jpg') = (Bytes 3000000)
    'pull-test\big.bin'                            = (Bytes 40000000)
    'pull-test\name with spaces & (brackets).docx' = (Bytes 12345)
}
foreach ($k in $files.Keys) { [IO.File]::WriteAllBytes((Join-Path $docs $k), $files[$k]) }
[IO.File]::WriteAllBytes((Join-Path $pplant 'sunset.png'), (Bytes 1000000))
# Windows' own folder settings: hidden and system, never copied
Set-Content -LiteralPath (Join-Path $plant 'desktop.ini') -Value '[.ShellClassInfo]' -Encoding ASCII
attrib +h +s (Join-Path $plant 'desktop.ini')
# links: never followed
cmd /c "mklink /J `"$(Join-Path $plant 'LinkToWindows')`" C:\Windows" | Out-Null
$links = @('pull-test/desktop.ini', 'pull-test/LinkToWindows')
try {
    New-Item -ItemType SymbolicLink -Path (Join-Path $plant 'letter-link.txt') -Target (Join-Path $plant 'letter.txt') | Out-Null
    $links += 'pull-test/letter-link.txt'
} catch { Write-Output "NOTE=no file symlink was planted ($($_.Exception.Message))" }

function Sha($p) { (Get-FileHash -Algorithm SHA256 -LiteralPath $p).Hash.ToLower() }
$expect = @()
foreach ($k in $files.Keys) { $p = Join-Path $docs $k; $expect += [ordered]@{ folder = 'Documents'; rel = ($k -replace '\\', '/'); size = (Get-Item -LiteralPath $p).Length; sha256 = (Sha $p) } }
$expect += [ordered]@{ folder = 'Pictures'; rel = 'pull-test/sunset.png'; size = 1000000; sha256 = (Sha (Join-Path $pplant 'sunset.png')) }

# bookmarks: a Chrome profile (only if this account has none of its own), a Firefox backup
$marks = [ordered]@{}
$cd = Join-Path $env:LOCALAPPDATA 'Google\Chrome\User Data\Default'
if (-not (Test-Path -LiteralPath (Join-Path $cd 'Bookmarks')) -or (Test-Path -LiteralPath (Join-Path $cd 'upgrade_-planted'))) {
    New-Item -ItemType Directory -Force -Path $cd | Out-Null
    $chrome = '{"roots":{"bookmark_bar":{"type":"folder","name":"Bookmarks bar","children":[{"type":"url","name":"Bank","url":"https://bank.example/"},{"type":"url","name":"Mail","url":"https://mail.example/"},{"type":"folder","name":"Recipes","children":[{"type":"url","name":"Soup","url":"https://soup.example/"},{"type":"url","name":"Bread","url":"https://bread.example/"}]}]},"other":{"type":"folder","name":"Other bookmarks","children":[{"type":"url","name":"News","url":"https://news.example/"}]},"synced":{"type":"folder","name":"Mobile bookmarks","children":[]}},"version":1}'
    [IO.File]::WriteAllText((Join-Path $cd 'Bookmarks'), $chrome)
    Set-Content -LiteralPath (Join-Path $cd 'upgrade_-planted') -Value 'the one click test planted this Bookmarks file' -Encoding ASCII
    $marks['Chrome'] = 5
} else { Write-Output 'NOTE=this account has its own Chrome bookmarks; none were planted' }
if ($Firefox -and (Test-Path -LiteralPath $Firefox)) {
    $fd = Join-Path $env:APPDATA 'Mozilla\Firefox\Profiles\pulltest.default-release\bookmarkbackups'
    New-Item -ItemType Directory -Force -Path $fd | Out-Null
    Copy-Item -LiteralPath $Firefox -Destination (Join-Path $fd 'bookmarks-2026-11-18_3_pulltest.jsonlz4') -Force
    $marks['Firefox'] = 3
}

$doc = [ordered]@{
    planted_utc   = (Get-Date).ToUniversalTime().ToString('o')
    windows_build = [Environment]::OSVersion.Version.ToString()
    copy          = $expect
    not_copied    = $links
    bookmarks     = $marks
    cloud_only    = 'none planted'
}
$ej = Join-Path $Out 'expected.json'
[IO.File]::WriteAllText($ej, ($doc | ConvertTo-Json -Depth 6), (New-Object Text.UTF8Encoding $false))
Write-Output "PLANTED=$($expect.Count)"
Write-Output "EXPECTED=$ej"
