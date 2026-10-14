<#
.SYNOPSIS
    Records tests/stick-golden.json: what Write-UpgradeStick.ps1 answers and
    does for the made-up disks in tests/stick-cases.json.

.DESCRIPTION
    The script's functions are loaded as they are (everything between
    $ErrorActionPreference and the main block, through Invoke-Expression),
    and its live helpers are shadowed by functions answering from the case:
    Get-UpgAttachedDisks, Get-CimInstance, Test-Elevated, Read-Host,
    Test-Path (for the source's SHA256SUMS), Invoke-UpgStickWrite (the
    destructive half: its result or its error, canned), Resolve-ResultsCsv
    (a temporary file), Get-Date and Write-Host. The "fn" cases call the
    pure functions and record the return; the "flow" cases run the script's
    own main block for a phase and record every line it printed, the
    prompt it asked, the write it asked for, the evidence row it appended,
    the stick.json it wrote and its exit code. Nothing on this computer is
    read, and no disk is touched.

    Ran once, 2026-10-14, while the script was still in the tree; it stays
    as the record of how stick-golden.json was made.

    Run from WSL:
      powershell.exe -NoProfile -ExecutionPolicy Bypass -File tests/stick-golden.ps1 `
        -Cases <win path>\tests\stick-cases.json -Script <win path>\evaluate\windows\Write-UpgradeStick.ps1 -Out <win path>\tests\stick-golden.json
#>
param([string]$Cases, [string]$Script, [string]$Out)
$ErrorActionPreference = 'Stop'
$caseList = @((Get-Content -Raw -LiteralPath $Cases | ConvertFrom-Json) | ForEach-Object { $_ })

$scriptLines = (Get-Content -Raw -LiteralPath $Script) -split "`r?`n"
$startIdx = [array]::FindIndex($scriptLines, [Predicate[string]]{ param($l) $l.StartsWith('$ErrorActionPreference') })
$mainIdx = [array]::FindIndex($scriptLines, [Predicate[string]]{ param($l) $l.StartsWith('if ($SelfTest)') })
if ($startIdx -lt 0 -or $mainIdx -lt 0) { throw 'the script does not look like Write-UpgradeStick.ps1' }
Invoke-Expression ($scriptLines[$startIdx..($mainIdx - 1)] -join "`n")
$script:MainText = (($scriptLines[($mainIdx + 1)..($scriptLines.Count - 1)] -join "`n").Replace("`$PSCmdlet.ParameterSetName -eq 'List'", '$List') -replace 'exit 2', '$script:ExitCode = 2; return') -replace 'exit 1', '$script:ExitCode = 1; return'

function NormObj($o) {
    if ($null -eq $o) { return $null }
    if ($o -is [string]) { return $o }
    if ($o -is [System.Collections.IDictionary]) { $h = [ordered]@{}; foreach ($k in $o.Keys) { $h[$k] = NormObj $o[$k] }; return $h }
    if ($o -is [Array]) { return , @($o | ForEach-Object { NormObj $_ }) }
    if ($o -is [PSCustomObject]) { $h = [ordered]@{}; foreach ($p in $o.PSObject.Properties) { $h[$p.Name] = NormObj $p.Value }; return $h }
    $o
}
function Add-Event($e) { [void]$script:Events.Add($e) }
function ParseO([string]$t) { [datetime]::ParseExact($t, 'o', [cultureinfo]::InvariantCulture, [Globalization.DateTimeStyles]::RoundtripKind) }
function AsDisk($d) { $o = [pscustomobject]@{}; foreach ($p in $d.PSObject.Properties) { $v = $p.Value; if ($p.Name -eq 'Volumes') { $v = @($v | ForEach-Object { $_ }) }; if ($p.Name -eq 'SizeBytes') { $v = [long]$v }; $o | Add-Member -NotePropertyName $p.Name -NotePropertyValue $v }; $o }

# ---- the shadows
function Write-Host { param([Parameter(Position = 0)]$Object, $ForegroundColor) Add-Event ([ordered]@{ host = "$Object".Replace($script:CsvPath, '<csv>') }) }
function Get-Date { (ParseO $script:Case.now).ToLocalTime() }
function Test-Elevated { [bool]$script:Case.elevated }
function Get-UpgAttachedDisks { @($script:Case.disks | ForEach-Object { AsDisk $_ }) }
function Get-CimInstance { [CmdletBinding()] param([Parameter(Position = 0)]$ClassName)
    $m = $script:Case.machine
    if ($ClassName -eq 'Win32_ComputerSystem') { [pscustomobject]@{ Manufacturer = $m.Manufacturer; Model = $m.Model } } else { [pscustomobject]@{ Caption = $m.Caption; BuildNumber = $m.BuildNumber } } }
function Read-Host { param([Parameter(Position = 0)]$Prompt) Add-Event ([ordered]@{ 'read-host' = "$Prompt" }); "$($script:Case.answer)" }
function Test-Path { [CmdletBinding()] param([Parameter(Position = 0)][string]$Path, [string]$LiteralPath) if ($Path.EndsWith('SHA256SUMS')) { return [bool]$script:Case.source_has_sums }; Microsoft.PowerShell.Management\Test-Path -Path $Path }
function Resolve-ResultsCsv { $script:CsvPath }
# the real Join-Path refuses a drive that is not there (the stick's D: during a recording); the script's use is a plain join
function Join-Path { param([Parameter(Position = 0)]$Path, [Parameter(Position = 1)]$ChildPath) "$("$Path".TrimEnd('\'))\$ChildPath" }
function Invoke-UpgStickWrite {
    param($Disk, [string]$SourceDir, [string]$BootLabel, [int]$BootGB)
    Add-Event ([ordered]@{ write = $Disk.UniqueId; source = $SourceDir; label = $BootLabel; boot_gb = $BootGB })
    $w = $script:Case.write
    if ($w.PSObject.Properties['error']) { throw "$($w.error)" }
    [pscustomobject]@{ Written = [bool]$w.Written; Verified = [bool]$w.Verified; Files = [int]$w.Files; Failed = @($w.Failed | ForEach-Object { "$_" }); BootLetter = $w.BootLetter; DataLetter = $w.DataLetter; Notes = @($w.Notes | ForEach-Object { "$_" }); Stick = $w.Stick }
}
function Out-File { [CmdletBinding()] param([Parameter(ValueFromPipeline = $true)]$InputObject, $FilePath, $Encoding)
    begin { $script:OutParts = @() } process { $script:OutParts += "$InputObject" } end { Add-Event ([ordered]@{ 'out-file' = "$FilePath"; doc = ($script:OutParts -join "`n" | ConvertFrom-Json | ForEach-Object { NormObj $_ }) }) } }

function Run-Main { Invoke-Expression $script:MainText }

function Invoke-FlowCase($case) {
    $script:Case = $case
    $script:Events = New-Object Collections.ArrayList
    $script:ExitCode = 0
    $script:CsvPath = Join-Path $script:Tmp ("r16-" + [guid]::NewGuid().ToString('n') + ".csv")
    $script:List = ($case.phase -eq 'list'); $script:Plan = ($case.phase -eq 'plan'); $script:Write = ($case.phase -eq 'write'); $script:SelfTest = $false
    $script:Target = "$($case.target)"; [long]$script:ExpectedSizeBytes = [long]$case.expected_bytes; [double]$script:ExpectedSizeGB = [double]$case.expected_gb
    $script:Source = "$($case.source)"; $script:Label = "$($case.label)"; [int]$script:BootSizeGB = [int]$case.boot_size_gb; $script:ConfirmedLabel = "$($case.confirmed_label)"; $script:ResultsCsv = ''
    $err = $null
    try { Run-Main } catch { $err = $_.Exception.Message; $script:ExitCode = 1 }
    $csv = @()
    if (Microsoft.PowerShell.Management\Test-Path $script:CsvPath) { $csv = @([IO.File]::ReadAllLines($script:CsvPath, [Text.Encoding]::UTF8)) }
    [ordered]@{ events = @($script:Events); csv = $csv; exit = $script:ExitCode; error = $err }
}

function Invoke-FnCase($case) {
    $a = $case.args
    $r = switch ($case.fn) {
        'Resolve-UpgStickTarget' { Resolve-UpgStickTarget -Disks @($a.Disks | ForEach-Object { AsDisk $_ }) -Target "$($a.Target)" -ExpectedBytes ([long]$a.ExpectedBytes) -ExpectedGB ([double]$a.ExpectedGB) }
        'Test-UpgStickSize' { Test-UpgStickSize -SizeBytes ([long]$a.SizeBytes) -ExpectedBytes ([long]$a.ExpectedBytes) -ExpectedGB ([double]$a.ExpectedGB) }
        'Get-UpgDiskRefusals' { @(Get-UpgDiskRefusals -Disk (AsDisk $a.Disk) -Target "$($a.Target)" -ExpectedBytes ([long]$a.ExpectedBytes) -ExpectedGB ([double]$a.ExpectedGB)) }
        'Get-UpgConfirmWord' { Get-UpgConfirmWord -Disk (AsDisk $a.Disk) }
        'ConvertFrom-UpgSha256Sums' { ConvertFrom-UpgSha256Sums -Lines ([string[]]@($a.Lines)) }
        'Format-DiskLine' { Format-DiskLine -Disk (AsDisk $a.Disk) }
        default { throw "no such function in the recorder: $($case.fn)" }
    }
    [ordered]@{ result = (NormObj $r) }
}

$script:Tmp = Join-Path $env:TEMP ('upgrade-stick-golden-' + [guid]::NewGuid().ToString('n'))
New-Item -ItemType Directory -Path $script:Tmp -Force | Out-Null
$golden = [ordered]@{}
foreach ($case in $caseList) {
    $golden[$case.name] = if ($case.kind -eq 'fn') { Invoke-FnCase $case } else { Invoke-FlowCase $case }
    $n = if ($case.kind -eq 'fn') { 'fn' } else { "$($golden[$case.name].events.Count) events, exit $($golden[$case.name].exit)" }
    Microsoft.PowerShell.Utility\Write-Host ("  {0,-22} {1}" -f $n, $case.name)
}
Remove-Item -Recurse -Force $script:Tmp -ErrorAction SilentlyContinue
$json = $golden | ConvertTo-Json -Depth 12
[IO.File]::WriteAllText($Out, $json + "`n", (New-Object Text.UTF8Encoding($false)))
Microsoft.PowerShell.Utility\Write-Host "  written: $Out ($($caseList.Count) cases)"
