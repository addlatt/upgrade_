<#
.SYNOPSIS
    Records tests/storage-mode-golden.json: what Test-StorageMode.ps1 does
    for the made-up inputs in tests/storage-mode-cases.json.

.DESCRIPTION
    The harness has a judging half (the pure functions, the texts, the
    phases' decisions) and a live half (its helpers: the facts, the Safe
    Mode key, the context, the stick, the scanner, bcdedit and shutdown,
    the task, the registry, the popups). This recorder loads the script's
    functions as they are (everything between Set-StrictMode and the main
    block, through Invoke-Expression), then defines a function of the same
    name for every live helper, answering from the case: a function defined
    later wins. The script's own main block then runs for the case's phase,
    and everything it does through those helpers is recorded in order (log
    lines, popups, tool calls, saves, registry and task calls), with what it
    left in the state directory and on the stick, both of which are
    temporary folders here. Nothing on this computer is read or changed.

    The "fn" cases call the pure functions directly and record the return.

    Ran once, 2026-10-14, from the commit that still carried the script (the
    parent of the one that deleted it). It cannot run again without it; it
    stays as the record of how storage-mode-golden.json was made.

    Run from WSL:
      powershell.exe -NoProfile -ExecutionPolicy Bypass -File tests/storage-mode-golden.ps1 `
        -Cases <win path>\tests\storage-mode-cases.json -Script <win path>\evaluate\windows\Test-StorageMode.ps1 -Out <win path>\tests\storage-mode-golden.json
#>
param([string]$Cases, [string]$Script, [string]$Out)
$ErrorActionPreference = 'Stop'
# PS 5.1 hands a JSON array over as one object; the pipe unrolls it
$caseList = @((Get-Content -Raw -LiteralPath $Cases | ConvertFrom-Json) | ForEach-Object { $_ })

# ---- the script's functions, as they are
$scriptLines = (Get-Content -Raw -LiteralPath $Script) -split "`r?`n"
$startIdx = [array]::FindIndex($scriptLines, [Predicate[string]]{ param($l) $l.StartsWith('Set-StrictMode') })
$mainIdx = [array]::FindIndex($scriptLines, [Predicate[string]]{ param($l) $l.StartsWith('if ($SelfTest)') })
if ($startIdx -lt 0 -or $mainIdx -lt 0) { throw 'the script does not look like Test-StorageMode.ps1' }
$script:FakeScriptRoot = (Join-Path $env:TEMP 'upgrade-storage-mode-golden-stick'); $script:FakeCommandPath = Join-Path $script:FakeScriptRoot 'Test-StorageMode.ps1'
Invoke-Expression (($scriptLines[$startIdx..($mainIdx - 1)] -join "`n").Replace('$PSScriptRoot', '$script:FakeScriptRoot').Replace('$PSCommandPath', '$script:FakeCommandPath'))
$script:MainText = ($scriptLines[($mainIdx + 1)..($scriptLines.Count - 1)] -join "`n") -replace 'exit 1', '$script:ExitCode = 1'

# ---- the fixed places (the script sees temporary folders; the record sees these)
$STATE_ROOT = 'C:\ProgramData\upgrade_'
$STICK_ROOT = 'E:\'
$tmp = Join-Path $env:TEMP ('upgrade-storage-mode-golden-' + [guid]::NewGuid().ToString('n'))
$script:TmpStateRoot = Join-Path $tmp 'ProgramData\upgrade_'
$script:StateTmp = Join-Path $script:TmpStateRoot 'storage-mode'
$script:StickTmp = (Join-Path $tmp 'stick') + '\'
$SafeMarkerPath = Join-Path $script:TmpStateRoot 'storage-mode-safeboot.txt'
$StateDir = $script:StateTmp

function Norm([string]$s) {
    $s = $s.Replace($script:TmpStateRoot, $STATE_ROOT).Replace($script:StickTmp, $STICK_ROOT)
    $s -replace 'scanning \(.*?\)\.\.\.', 'scanning (<scanner>)...'
}
function NormObj($o) {
    if ($null -eq $o) { return $null }
    if ($o -is [string]) { return (Norm $o) }
    if ($o -is [System.Collections.IDictionary]) { $h = [ordered]@{}; foreach ($k in $o.Keys) { $h[$k] = NormObj $o[$k] }; return $h }
    if ($o -is [Array]) { return , @($o | ForEach-Object { NormObj $_ }) }
    if ($o -is [PSCustomObject]) { $h = [ordered]@{}; foreach ($p in $o.PSObject.Properties) { $h[$p.Name] = NormObj $p.Value }; return $h }
    $o
}
function Add-Event($e) { [void]$script:Events.Add($e) }
function ParseO([string]$t) { [datetime]::ParseExact($t, 'o', [cultureinfo]::InvariantCulture, [Globalization.DateTimeStyles]::RoundtripKind) }

# ---- the live helpers, answering from the case
function Write-Log {
    param([string]$Line, [string]$Color = 'Gray')
    Add-Event ([ordered]@{ log = (Norm $Line) })
    $stamp = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
    foreach ($p in @($script:LogFile, $script:StickLog)) { if ($p) { try { Add-Content -Path $p -Value "$stamp $Line" -Encoding UTF8 } catch { } } }
}
function Write-Host { param([Parameter(Position = 0)]$Object, $ForegroundColor) Add-Event ([ordered]@{ host = (Norm "$Object") }) }
function Get-Date { $script:NowLocal }
function Test-Elevated { [bool]$script:Case.elevated }
function Get-DriveRoot { param([string]$Letter) $script:StickTmp }
function Get-LiveSafeMode { if ($null -eq $script:Case.safe_mode) { return $null }; [int]$script:Case.safe_mode }
function Get-LiveContext {
    $c = $script:Case.context
    [ordered]@{ Utc = "$($c.Utc)"; RunAs = "$($c.RunAs)"; Interactive = [bool]$c.Interactive; SessionId = [int]$c.SessionId; ExplorerRunning = [bool]$c.ExplorerRunning; Unattended = [bool]$c.Unattended; UptimeSeconds = [int]$c.UptimeSeconds }
}
function Find-StickRoot { param($S) Add-Event ([ordered]@{ 'find-stick' = [bool]$script:Case.stick }); if ($script:Case.stick) { $script:StickTmp } else { $null } }
function Wait-Stick { param($S) Add-Event ([ordered]@{ 'wait-stick' = [bool]$script:Case.stick }); if ($script:Case.stick) { $script:StickTmp } else { $null } }
function Get-Facts {
    param([string]$Root)
    Add-Event ([ordered]@{ facts = (Norm $Root) })
    $f = $script:Case.facts
    if ($f.PSObject.Properties['error']) { throw "$($f.error)" }
    [ordered]@{ Vendor = "$($f.Vendor)"; Model = "$($f.Model)"; BiosVersion = "$($f.BiosVersion)"; OsCaption = "$($f.OsCaption)"; OsBuild = [int]$f.OsBuild
                SecureBoot = "$($f.SecureBoot)"; BitLocker = "$($f.BitLocker)"; StickVolumeId = "$($f.StickVolumeId)"; StickBus = "$($f.StickBus)" }
}
function Protect-StateDir { param([string]$State) Add-Event ([ordered]@{ protect = (Norm $State) }); if ($script:Case.protect_error) { throw "$($script:Case.protect_error)" } }
function Register-ResumeTask { param([string]$State) Add-Event ([ordered]@{ task = 'register' }); if ($script:Case.register_task_error) { throw "$($script:Case.register_task_error)" } }
function Unregister-ResumeTask { Add-Event ([ordered]@{ task = 'unregister'; present = $script:TaskPresent }); $r = $script:TaskPresent; $script:TaskPresent = $false; $r }
function Invoke-Native {
    param([string]$Command, [string[]]$Arguments)
    $exit = 0; $text = ''
    foreach ($n in @($script:Case.native)) {
        if ($n.program -ne $Command -or $n.first -ne $Arguments[0]) { continue }
        if ($n.PSObject.Properties['second'] -and $n.second -ne $Arguments[1]) { continue }
        $exit = [int]$n.exit; $text = "$($n.text)"; break
    }
    Add-Event ([ordered]@{ native = $Command; args = @($Arguments | ForEach-Object { Norm $_ }); exit = $exit })
    [ordered]@{ Exit = $exit; Text = $text }
}
function Show-Popup {
    param([string]$Text, [string]$Title, [int]$Seconds, [int]$Buttons = 0)
    $answer = -1
    if ($script:Popups.Count -gt 0) { $answer = [int]$script:Popups.Dequeue() }
    Add-Event ([ordered]@{ popup = $Title; text = (Norm $Text); seconds = $Seconds; buttons = $Buttons; answer = $answer })
    $answer
}
function Set-ItemProperty {
    [CmdletBinding()] param($Path, $Name, $Value)
    if ($script:RefuseName -and $Name -eq $script:RefuseName) { throw 'Requested registry access is not allowed.' }
    $script:Registry[$Name] = "$Value"
    Add-Event ([ordered]@{ 'reg-set' = $Name })
}
function Remove-ItemProperty {
    [CmdletBinding()] param($Path, $Name)
    Add-Event ([ordered]@{ 'reg-remove' = $Name; present = $script:Registry.ContainsKey($Name) })
    if ($script:Registry.ContainsKey($Name) -and $script:Registry[$Name] -ne 'sticky') { $script:Registry.Remove($Name) }
}
function Get-ItemProperty {
    [CmdletBinding()] param($Path, $Name)
    if ($script:Registry.ContainsKey($Name)) { $o = [ordered]@{}; $o[$Name] = $script:Registry[$Name]; return [pscustomobject]$o }
    $null
}
function Save-Json {
    param($Obj, [string]$Path)
    $json = $Obj | ConvertTo-Json -Depth 12
    [IO.File]::WriteAllText($Path, $json, (New-Object Text.UTF8Encoding($false)))
    Add-Event ([ordered]@{ save = (Norm $Path); doc = (NormObj ($json | ConvertFrom-Json)) })
}
function Copy-Item { [CmdletBinding()] param($Path, $Destination, [switch]$Force) if ("$Destination".EndsWith('Test-StorageMode.ps1')) { Add-Event ([ordered]@{ copy = 'program' }) } }
function Test-Path { [CmdletBinding()] param([Parameter(Position = 0)][string]$Path) if ($Path.EndsWith('upgrade-scan.ps1')) { return $true }; Microsoft.PowerShell.Management\Test-Path -Path $Path }
function powershell.exe {
    $a = @($args | ForEach-Object { "$_" })
    if ($a -contains '-Json') {
        $dir = $a[[array]::IndexOf($a, '-OutDir') + 1]
        $script:LegIndex++
        $leg = $script:LegList[$script:LegIndex - 1]
        Add-Event ([ordered]@{ scan = (Norm $dir) })
        if (-not $leg.PSObject.Properties['no_report']) {
            [IO.File]::WriteAllText((Join-Path $dir $leg.report_file), ($leg.report | ConvertTo-Json -Depth 12), (New-Object Text.UTF8Encoding($false)))
        }
        'scan output line 1'; 'scan output line 2'
    } elseif ($a -contains '-DumpMachine') {
        $path = $a[[array]::IndexOf($a, '-DumpMachine') + 1]
        $leg = $script:LegList[$script:LegIndex - 1]
        Add-Event ([ordered]@{ capture = (Norm $path) })
        if (-not $leg.PSObject.Properties['no_capture']) {
            [IO.File]::WriteAllText($path, ($leg.capture | ConvertTo-Json -Depth 12), (New-Object Text.UTF8Encoding($false)))
        }
    }
}

function Run-Main { Invoke-Expression $script:MainText }

function Snapshot {
    $files = [ordered]@{}
    foreach ($root in @($script:TmpStateRoot, $script:StickTmp.TrimEnd('\'))) {
        if (-not (Microsoft.PowerShell.Management\Test-Path $root)) { continue }
        foreach ($f in @(Get-ChildItem -LiteralPath $root -Recurse -File | Sort-Object FullName)) {
            $rel = Norm $f.FullName
            $text = [IO.File]::ReadAllText($f.FullName, [Text.Encoding]::UTF8).TrimStart([char]0xFEFF)
            if ($f.Extension -eq '.json') { $files[$rel] = NormObj ($text | ConvertFrom-Json) }
            else { $lines = $text -split "`r?`n"; if ($lines.Count -gt 0 -and $lines[-1] -eq '') { $lines = $lines[0..($lines.Count - 2)] }; $files[$rel] = @($lines | ForEach-Object { Norm $_ }) }
        }
    }
    $files
}

function Invoke-FlowCase($case) {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Path $script:StateTmp -Force | Out-Null
    New-Item -ItemType Directory -Path $script:StickTmp -Force | Out-Null
    # the start phase makes the stick's folder; a later phase finds it there (the case says)
    if ($case.stick_folder) { New-Item -ItemType Directory -Path (Join-Path $script:StickTmp 'upgrade_\storage-mode') -Force | Out-Null }
    $script:Case = $case
    $script:Events = New-Object Collections.ArrayList
    $script:ExitCode = 0
    $script:LogFile = $null; $script:StickLog = $null
    $script:NowLocal = (ParseO $case.now).ToLocalTime()
    $script:Popups = New-Object Collections.Queue
    foreach ($p in @($case.popups)) { $script:Popups.Enqueue($p) }
    $script:LegIndex = 0; $script:LegList = @($case.legs | ForEach-Object { $_ })
    $script:Registry = @{}; $script:RefuseName = $null
    foreach ($p in $case.runonce.PSObject.Properties) { if ($p.Name -eq '__refuse__') { $script:RefuseName = $p.Value } else { $script:Registry[$p.Name] = "$($p.Value)" } }
    $script:TaskPresent = [bool]$case.task_present
    if ($null -ne $case.state) { [IO.File]::WriteAllText((Join-Path $script:StateTmp 'state.json'), ($case.state | ConvertTo-Json -Depth 12), (New-Object Text.UTF8Encoding($false))) }
    foreach ($p in $case.state_files.PSObject.Properties) { [IO.File]::WriteAllText((Join-Path $script:StateTmp $p.Name), ($p.Value | ConvertTo-Json -Depth 12), (New-Object Text.UTF8Encoding($false))) }
    if ($null -ne $case.marker) { [IO.File]::WriteAllLines($SafeMarkerPath, [string[]]@($case.marker)) }
    $script:Start = $false; $script:Resume = $false; $script:Notify = $false; $script:SafeModeSignIn = $false; $script:Abort = $false; $script:SelfTest = $false
    switch ($case.phase) { 'start' { $script:Start = $true } 'resume' { $script:Resume = $true } 'notify' { $script:Notify = $true } 'abort' { $script:Abort = $true } }
    $script:StickDrive = $case.args.stick; $script:Bench = [bool]$case.args.bench; $script:NoPrompt = [bool]$case.args.no_prompt
    Run-Main
    [ordered]@{ events = @($script:Events); files = (Snapshot); exit = $script:ExitCode }
}

function Invoke-FnCase($case) {
    $a = $case.args
    $r = switch ($case.fn) {
        'Get-SmPciId' { Get-SmPciId -DeviceId $a.DeviceId }
        'Get-SmClassCodes' { , @(Get-SmClassCodes -CompatibleIds $a.CompatibleIds) }
        'Get-SmStorageControllers' { , @(Get-SmStorageControllers -Pnp @($a.Pnp)) }
        'mode-of' { Get-SmModeWord @(Get-SmStorageControllers -Pnp @($a.Pnp)) }
        'Get-SmNextMode' { Get-SmNextMode -Current $a.Current }
        'Get-SmModeLabel' { Get-SmModeLabel -Mode $a.Mode }
        'Get-SmReportCheck' { Get-SmReportCheck -Report $a.Report }
        'ConvertFrom-SmBcdCopy' { ConvertFrom-SmBcdCopy -Text $a.Text }
        'Test-SmSafeMode' { Test-SmSafeMode -OptionValue $a.OptionValue }
        'Get-SmLegOutcome' { Get-SmLegOutcome -Leg $a.Leg -FirstMode $a.FirstMode -ThisMode $a.ThisMode }
        'ConvertFrom-SmSafeMarker' { , @(ConvertFrom-SmSafeMarker -Lines ([string[]]@($a.Lines))) }
        'Get-SmFlowResult' { Get-SmFlowResult -Stage $a.Stage -Legs $a.Legs -LastOutcome $a.LastOutcome }
        'ConvertTo-SmCsvLine' { ConvertTo-SmCsvLine -Fields ([object[]]@($a.Fields)) }
        default { throw "no such function in the recorder: $($case.fn)" }
    }
    [ordered]@{ result = (NormObj $r) }
}

$golden = [ordered]@{}
foreach ($case in $caseList) {
    $golden[$case.name] = if ($case.kind -eq 'fn') { Invoke-FnCase $case } else { Invoke-FlowCase $case }
    $n = if ($case.kind -eq 'fn') { 'fn' } else { "$($golden[$case.name].events.Count) events, exit $($golden[$case.name].exit)" }
    Microsoft.PowerShell.Utility\Write-Host ("  {0,-22} {1}" -f $n, $case.name)
}
Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
$json = $golden | ConvertTo-Json -Depth 20
[IO.File]::WriteAllText($Out, $json + "`n", (New-Object Text.UTF8Encoding($false)))
Microsoft.PowerShell.Utility\Write-Host "  written: $Out ($($caseList.Count) cases)"
