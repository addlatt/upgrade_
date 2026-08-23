# Guest-side V0 prep. Run from the elevated PowerShell:
#   powershell -ExecutionPolicy Bypass -File \\10.0.2.4\qemu\rig\vm\guest\prep.ps1
$ErrorActionPreference = 'Continue'
Write-Host "=== upgrade_ V0 guest prep ===" -ForegroundColor Cyan

# 1. Stick drive letter (the FAT32 UPGV0 volume on USB)
$stick = Get-Volume | Where-Object { $_.FileSystemLabel -eq 'UPGV0' }
if ($stick) {
    Write-Host ("STICK: {0}: (label UPGV0, {1})" -f $stick.DriveLetter, $stick.FileSystemType) -ForegroundColor Green
    $root = "$($stick.DriveLetter):\"
    Write-Host ("  contents: " + ((Get-ChildItem $root -Force | Select-Object -Expand Name) -join ', '))
    Write-Host ("  EFI\BOOT: " + ((Get-ChildItem "$root\EFI\BOOT" -Force -ErrorAction SilentlyContinue | Select-Object -Expand Name) -join ', '))
} else {
    Write-Host "STICK: NOT FOUND - no UPGV0 volume. Is --stick attached?" -ForegroundColor Red
}

# 2. Host share writable?
$share = '\\10.0.2.4\qemu'
try {
    $probe = Join-Path $share ('.rigprobe-' + [guid]::NewGuid().ToString('N') + '.tmp')
    Set-Content -Path $probe -Value 'ok' -ErrorAction Stop
    Remove-Item $probe -ErrorAction SilentlyContinue
    Write-Host "SHARE: $share is writable" -ForegroundColor Green
} catch {
    Write-Host "SHARE: $share NOT writable - $($_.Exception.Message)" -ForegroundColor Red
}

# 3. Local copy of the harness + scanner (from the share, excluding artifacts)
$dst = 'C:\upgrade_'
if (-not (Test-Path $dst)) {
    Write-Host "Copying harness + scanner to $dst ..." -ForegroundColor Cyan
    New-Item -ItemType Directory -Path $dst -Force | Out-Null
    foreach ($sub in @('upgrade_\windows','evaluate\windows','data','docs\validation-results')) {
        $src = Join-Path $share $sub
        $out = Join-Path $dst $sub
        New-Item -ItemType Directory -Path (Split-Path $out) -Force -ErrorAction SilentlyContinue | Out-Null
        Copy-Item $src $out -Recurse -Force -ErrorAction SilentlyContinue
    }
}
if (Test-Path "$dst\upgrade_\windows\Test-Handoff.ps1") {
    Write-Host "HARNESS: C:\upgrade_\upgrade_\windows\Test-Handoff.ps1 present" -ForegroundColor Green
} else {
    Write-Host "HARNESS: missing after copy" -ForegroundColor Red
}
Write-Host "=== prep done ===" -ForegroundColor Cyan
