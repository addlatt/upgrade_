Write-Host "=== {fwbootmgr} order ===" -ForegroundColor Cyan
bcdedit /enum "{fwbootmgr}" | Select-String "displayorder|bootsequence|^identifier" 
Write-Host "=== all firmware boot entries (description = position) ===" -ForegroundColor Cyan
$out = bcdedit /enum firmware
$out | Select-String "^identifier|^description" | ForEach-Object { $_.Line.Trim() }
