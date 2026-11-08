# The reader's test disk (settle-in/src/ntfsread.rs; RISKS R43) - Windows side
# (guest, PowerShell Direct, ELEVATED). Windows PowerShell 5.1. Rig tooling,
# not product.
#
# Makes a small NTFS volume the way Windows makes one, in a fixed VHD of its
# own (C: is not touched beyond the VHD file, deleted at the end), and puts in
# it every kind of entry the reader must tell apart, under
# X:\Users\Ann\Documents:
#   plain files (a small one that lives in its MFT record, a 50 KB one, a
#   name with spaces and accents, an empty one), one with a short MS-DOS
#   name beside its long one, a hard link (two names, one file), a junction,
#   a file Windows compressed (compact /c), and a Cloud Files sync root (the
#   test provider, guest/cfprovider.cs, with PARTIAL hydration) holding a
#   downloaded file, one only in the cloud, one partly downloaded (the
#   provider sends 64 KB and refuses the rest), a subfolder placeholder with
#   a downloaded file and a cloud-only one in it, and a compressed file.
# Then it writes, from Windows' own view, every entry's attributes, kind,
# size and (for what is wholly on the disk) sha256, and copies the VHD and
# that list to OEMDRV:\v3\fixture\. The host cuts the volume out of the VHD
# (rig/hyperv/ntfs-fixture.py) into settle-in/tests/ntfs-fixture/.
$ErrorActionPreference = 'Stop'
function Log($m) { Write-Host ('{0:HH:mm:ss} fixture: {1}' -f (Get-Date), $m) }

$vol = Get-Volume | Where-Object { $_.FileSystemLabel -eq 'OEMDRV' } | Select-Object -First 1
if (-not $vol) { throw 'OEMDRV volume not found' }
$out = "$($vol.DriveLetter):\v3\fixture"
New-Item -ItemType Directory -Force -Path $out | Out-Null

$vhd = Join-Path $env:TEMP 'upgrade-ntfs-fixture.vhd'
if (Test-Path $vhd) { Remove-Item -Force $vhd }
# a letter nobody uses
$letter = @('R', 'Q', 'P', 'O', 'N') | Where-Object { -not (Test-Path "$($_):\") } | Select-Object -First 1
$dp = Join-Path $env:TEMP 'upgrade-fixture-dp.txt'
@"
create vdisk file="$vhd" maximum=24 type=fixed
select vdisk file="$vhd"
attach vdisk
create partition primary
format fs=ntfs quick label=FIXTURE unit=4096
assign letter=$letter
"@ | Set-Content -Encoding ASCII $dp
diskpart /s $dp | Out-Null
if (-not (Test-Path "$($letter):\")) { throw "the fixture volume did not mount at $($letter):" }
$X = "$($letter):"
Log "volume $X in $vhd"
fsutil 8dot3name set $X 0 | Out-Null   # short names on, for this volume only

$rng = New-Object System.Random 20261107
function Bytes([int]$n) { $b = New-Object byte[] $n; $rng.NextBytes($b); return ,$b }
$docs = "$X\Users\Ann\Documents"
New-Item -ItemType Directory -Force -Path "$docs\Taxes" | Out-Null
$plain = [ordered]@{
    'small.txt'                              = [Text.Encoding]::UTF8.GetBytes("small enough to live in its MFT record`r`n")
    'empty.txt'                              = (New-Object byte[] 0)
    'Taxes\return.pdf'                       = (Bytes 50000)
    ('caf' + [char]0xE9 + ' with spaces.docx') = (Bytes 12345)
    'A long file name for its short name.txt' = (Bytes 5000)
}
foreach ($k in $plain.Keys) { [IO.File]::WriteAllBytes((Join-Path $docs $k), $plain[$k]) }
fsutil hardlink create "$docs\hard-link.pdf" "$docs\Taxes\return.pdf" | Out-Null
cmd /c "mklink /J `"$docs\Junction`" `"$X\Users`"" | Out-Null
$text = [Text.Encoding]::ASCII.GetBytes(('compress me ' * 8000))
[IO.File]::WriteAllBytes("$docs\compressed.txt", $text)
compact /c "$docs\compressed.txt" | Out-Null

Add-Type -TypeDefinition ([IO.File]::ReadAllText((Join-Path $PSScriptRoot 'cfprovider.cs'))) -Language CSharp
$root = "$docs\OneDrive"
New-Item -ItemType Directory -Force -Path $root | Out-Null
$prov = New-Object UpgV8.Provider
$prov.Hydration = 0   # CF_HYDRATION_POLICY_PARTIAL: a read fetches only what it needs
$cloud = @{ 1 = (Bytes 40000); 2 = (Bytes 70000); 3 = (Bytes 200000); 4 = (Bytes 9000); 5 = (Bytes 5000) }
$names = New-Object 'System.Collections.Generic.Dictionary[int,string]'
$names[1] = 'downloaded.pdf'; $names[2] = 'only-in-cloud.docx'; $names[3] = 'partly.bin'; $names[4] = 'Sub\downloaded-too.txt'; $names[5] = 'Sub\also-in-cloud.xlsx'
foreach ($k in $cloud.Keys) { $prov.Files[$k] = $cloud[$k] }
$prov.Partial.Add(3) | Out-Null
$prov.Register($root)
$prov.Connect()
$prov.CreateDirectoryPlaceholder('Sub')
$prov.CreatePlaceholders($names) | Out-Null
# downloaded: read whole in another process (the filter calls this one back)
foreach ($id in @(1, 4)) {
    $p = Join-Path $root $names[$id]
    powershell.exe -NoProfile -Command "[IO.File]::ReadAllBytes('$p') | Out-Null" | Out-Null
}
# partly downloaded: read its first 4 KB; the provider sends 64 KB, then refuses
$p = Join-Path $root $names[3]
powershell.exe -NoProfile -Command "`$f = [IO.File]::OpenRead('$p'); `$b = New-Object byte[] 4096; try { `$f.Read(`$b, 0, 4096) | Out-Null } catch {} finally { `$f.Close() }" | Out-Null
# a file compressed inside the OneDrive folder (made there as a plain file, then compressed)
[IO.File]::WriteAllBytes("$root\compressed-in-onedrive.txt", $text)
compact /c "$root\compressed-in-onedrive.txt" | Out-Null
$prov.Log | ForEach-Object { Log "  cf: $_" }

# --- Windows' own view of every entry
function Sha([byte[]]$b) { ([BitConverter]::ToString((New-Object Security.Cryptography.SHA256Managed).ComputeHash($b)) -replace '-', '').ToLower() }
$known = @{}
foreach ($k in $plain.Keys) { $known[($k -replace '\\', '/')] = $plain[$k] }
$known['hard-link.pdf'] = $plain['Taxes\return.pdf']
$known['compressed.txt'] = $text
foreach ($k in $cloud.Keys) { $known['OneDrive/' + ($names[$k] -replace '\\', '/')] = $cloud[$k] }
$known['OneDrive/compressed-in-onedrive.txt'] = $text
$entries = @()
# Get-ChildItem would open placeholders' parents through the filter only; it never reads a file
foreach ($i in (Get-ChildItem -LiteralPath $docs -Recurse -Force -Attributes !ReparsePoint, ReparsePoint)) {
    $rel = $i.FullName.Substring($docs.Length + 1) -replace '\\', '/'
    if ($rel -like 'Junction/*') { continue }
    $a = [int64]$i.Attributes.value__
    $e = [ordered]@{ rel = $rel; attributes = ('0x{0:X}' -f $a); dir = [bool]($a -band 0x10) }
    if (-not $e.dir) { $e.size = $i.Length }
    if ($known.ContainsKey($rel)) { $e.sha256 = Sha $known[$rel] }
    $entries += $e
}
$short = (cmd /c "dir /x `"$docs`"") -match 'ALONGF~1' | Select-Object -First 1
$doc = [ordered]@{
    made_utc = (Get-Date).ToUniversalTime().ToString('o')
    windows_build = [Environment]::OSVersion.Version.ToString()
    documents = 'Users/Ann/Documents'
    short_name_seen = [bool]$short
    entries = $entries
    provider_log = @($prov.Log)
}
[IO.File]::WriteAllText("$out\fixture.json", ($doc | ConvertTo-Json -Depth 6), (New-Object Text.UTF8Encoding $false))
Log ("{0} entries; attributes: {1}" -f $entries.Count, (($entries | ForEach-Object { "$($_.rel)=$($_.attributes)" }) -join ', '))

# disconnect, never unregister: the placeholders stay as they are on the volume
$prov.Disconnect()
@"
select vdisk file="$vhd"
detach vdisk
"@ | Set-Content -Encoding ASCII $dp
diskpart /s $dp | Out-Null
Copy-Item -LiteralPath $vhd -Destination "$out\ntfs-fixture.vhd" -Force
Remove-Item -Force $vhd, $dp
Log "copied to $out"
