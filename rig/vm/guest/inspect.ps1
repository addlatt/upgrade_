# fs0: probe - where did the handoff marker land?
Write-Host "=== fs0: probe ===" -ForegroundColor Cyan
$stickFired = Test-Path 'D:\fired.txt'
Write-Host ("STICK D:\fired.txt  : {0}" -f $stickFired) -ForegroundColor (@('Red','Green')[[int]$stickFired])
Write-Host ("D:\ contents        : " + ((Get-ChildItem D:\ -Force -ErrorAction SilentlyContinue | Select-Object -Expand Name) -join ', '))

# mount the ESP and look there too
cmd /c "mountvol S: /S" 2>$null
$espFired = Test-Path 'S:\fired.txt'
Write-Host ("ESP   S:\fired.txt  : {0}" -f $espFired) -ForegroundColor (@('Green','Red')[[int]$espFired])
if (Test-Path 'S:\') { Write-Host ("ESP root            : " + ((Get-ChildItem S:\ -Force -ErrorAction SilentlyContinue | Select-Object -Expand Name) -join ', ')) }
cmd /c "mountvol S: /D" 2>$null

Write-Host ("handoff-state.json  : " + (Test-Path "$env:ProgramData\upgrade_\v0\handoff-state.json"))
if ($stickFired) { Write-Host "VERDICT: marker on the STICK - startup.nsh mapped fs0: correctly, handoff FIRED" -ForegroundColor Green }
elseif ($espFired) { Write-Host "VERDICT: marker on the ESP - fs0: misdirected; startup.nsh needs self-locating fix" -ForegroundColor Yellow }
else { Write-Host "VERDICT: no marker anywhere - handoff did NOT fire this boot" -ForegroundColor Red }
