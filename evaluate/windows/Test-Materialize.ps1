<#
.SYNOPSIS
    upgrade_ / V8 validation harness - OneDrive placeholders are materialized
    at evaluate, or the harvest refuses.

.DESCRIPTION
    RISKS R8 / VALIDATION V8: a "free up space" file is a placeholder - full
    size in the directory, no bytes on disk, fetched by Windows' cloud files
    filter (cldflt) on first read. Pulled from Linux later, it arrives EMPTY.
    So evaluate must force every placeholder local while Windows is alive,
    and verify it did. This harness proves that end to end, against the real
    filter, with ground truth:

      1. registers a temporary sync root through the Cloud Files API
         (CfRegisterSyncRoot - the same API OneDrive is built on) with this
         process as the sync provider, in a scratch directory;
      2. creates dehydrated placeholders there (CfCreatePlaceholders) for a
         set of files whose bytes only this process knows - and confirms
         each carries FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS and allocates
         nothing on disk;
      3. runs the harvester's materialization seam in a SEPARATE process
         (Harvest-UpgradeState.ps1 -MaterializePath), which pins and reads
         each file; the filter asks this process for the bytes;
      4. checks, per file, that the bytes on the NTFS volume now hash to the
         ground truth, that the placeholder attribute is gone, and that the
         file allocates; and that the one file this provider REFUSES to
         serve is reported as failed by the harvester, with a non-zero exit -
         the refuse arm of "materialize, or refuse";
      5. tears the sync root down and appends one row to
         docs/validation-results/v8-materialize.csv.

    A pass here is `pass-plumbing`: the filter, the attribute, the read path
    and the judgment are real; the provider is ours. The residue (CLAUDE.md
    rule #5) is OneDrive itself as the provider - a signed-in account with
    "Files On-Demand" - which -OneDrive runs against, and which is what a
    plain `pass` requires.

.PARAMETER OneDrive
    Run against the signed-in OneDrive instead of the test provider: create
    a small test folder inside the OneDrive root, wait for the client to
    upload it, ask the client to dehydrate it (attrib +U -P, "free up
    space"), wait until it is a real placeholder, then materialize it
    through the harvester and compare hashes. Uploads a few MB to the
    account and removes the folder afterwards. Needs OneDrive running and
    signed in; never run unattended on a machine that is not yours.

.PARAMETER SelfTest
    The harness's own logic tests (result classification) - no filter, no
    files, no elevation.

.PARAMETER ResultsCsv
    Where the evidence row goes. Default: the repo's
    docs/validation-results/v8-materialize.csv if found.
#>
[CmdletBinding()]
param(
    [switch]$OneDrive,
    [switch]$SelfTest,
    [string]$ResultsCsv,
    [string]$WorkDir,
    [int]$TimeoutSec = 300,
    [switch]$KeepWork
)

$ErrorActionPreference = 'Stop'
$HarnessVersion = '0.1.0'
$Harvester = Join-Path $PSScriptRoot 'Harvest-UpgradeState.ps1'
$CsvHeader = 'timestamp,harness,os_build,provider,files,bytes,placeholders_confirmed,materialized,bytes_verified,refused_expected,refused_reported,harvest_exit,result,notes'
$FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS = 0x400000
$FILE_ATTRIBUTE_OFFLINE               = 0x1000

function New-Line { param([string]$s = '', [string]$c = 'Gray') Write-Host $s -ForegroundColor $c }

# =============================================================================
#  the test sync provider - Cloud Files API via P/Invoke
# =============================================================================
#  Struct layouts follow cfapi.h on x64. Offsets that matter and were checked
#  against the header: CF_CALLBACK_PARAMETERS keeps ParamSize at 0 and the
#  union at 8; CF_OPERATION_PARAMETERS the same; CF_PLACEHOLDER_CREATE_INFO
#  is 88 bytes with FileSize at 48 and CreateUsn at 80.

$CfSource = @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;

namespace UpgV8 {
    public static class Cf {
        [StructLayout(LayoutKind.Sequential)]
        public struct SYNC_REGISTRATION {
            public uint StructSize;
            [MarshalAs(UnmanagedType.LPWStr)] public string ProviderName;
            [MarshalAs(UnmanagedType.LPWStr)] public string ProviderVersion;
            public IntPtr SyncRootIdentity; public uint SyncRootIdentityLength;
            public IntPtr FileIdentity;     public uint FileIdentityLength;
            public Guid ProviderId;
        }
        [StructLayout(LayoutKind.Sequential)]
        public struct SYNC_POLICIES {
            public uint StructSize;
            public ushort HydrationPrimary; public ushort HydrationModifier;
            public ushort PopulationPrimary; public ushort PopulationModifier;
            public uint InSync; public uint HardLink; public uint PlaceholderManagement;
        }
        [StructLayout(LayoutKind.Sequential)]
        public struct CALLBACK_REGISTRATION { public uint Type; public IntPtr Callback; }
        [StructLayout(LayoutKind.Sequential)]
        public struct CALLBACK_INFO {
            public uint StructSize; public long ConnectionKey; public IntPtr CallbackContext;
            public IntPtr VolumeGuidName; public IntPtr VolumeDosName; public uint VolumeSerialNumber;
            public long SyncRootFileId; public IntPtr SyncRootIdentity; public uint SyncRootIdentityLength;
            public long FileId; public long FileSize; public IntPtr FileIdentity; public uint FileIdentityLength;
            public IntPtr NormalizedPath; public long TransferKey; public byte PriorityHint;
            public IntPtr CorrelationVector; public IntPtr ProcessInfo; public long RequestKey;
        }
        [StructLayout(LayoutKind.Sequential)]
        public struct FETCH_DATA_PARAMS {
            public uint ParamSize; public uint Pad0; public uint Flags; public uint Pad1;
            public long RequiredFileOffset; public long RequiredLength;
            public long OptionalFileOffset; public long OptionalLength;
            public long LastDehydrationTime; public uint LastDehydrationReason;
        }
        [StructLayout(LayoutKind.Sequential)]
        public struct OPERATION_INFO {
            public uint StructSize; public uint Type; public long ConnectionKey; public long TransferKey;
            public IntPtr CorrelationVector; public IntPtr SyncStatus; public long RequestKey;
        }
        [StructLayout(LayoutKind.Sequential)]
        public struct TRANSFER_DATA_PARAMS {
            public uint ParamSize; public uint Pad0; public uint Flags; public int CompletionStatus;
            public IntPtr Buffer; public long Offset; public long Length;
        }
        [StructLayout(LayoutKind.Sequential)]
        public struct PLACEHOLDER_CREATE_INFO {
            [MarshalAs(UnmanagedType.LPWStr)] public string RelativeFileName;
            public long CreationTime; public long LastAccessTime; public long LastWriteTime; public long ChangeTime;
            public uint FileAttributes; public uint Pad0; public long FileSize;
            public IntPtr FileIdentity; public uint FileIdentityLength; public uint Flags; public int Result; public uint Pad1;
            public long CreateUsn;
        }
        [UnmanagedFunctionPointer(CallingConvention.StdCall)]
        public delegate void CfCallback(IntPtr info, IntPtr parameters);

        [DllImport("cldapi.dll", CharSet = CharSet.Unicode)] public static extern int CfRegisterSyncRoot(string path, ref SYNC_REGISTRATION reg, ref SYNC_POLICIES pol, uint flags);
        [DllImport("cldapi.dll", CharSet = CharSet.Unicode)] public static extern int CfUnregisterSyncRoot(string path);
        [DllImport("cldapi.dll", CharSet = CharSet.Unicode)] public static extern int CfConnectSyncRoot(string path, CALLBACK_REGISTRATION[] table, IntPtr ctx, uint flags, out long key);
        [DllImport("cldapi.dll")] public static extern int CfDisconnectSyncRoot(long key);
        [DllImport("cldapi.dll", CharSet = CharSet.Unicode)] public static extern int CfCreatePlaceholders(string baseDir, [In, Out] PLACEHOLDER_CREATE_INFO[] arr, uint count, uint flags, out uint processed);
        [DllImport("cldapi.dll")] public static extern int CfExecute(ref OPERATION_INFO info, ref TRANSFER_DATA_PARAMS p);
    }

    // The provider: holds the ground truth, answers FETCH_DATA with it - or
    // refuses, for the files listed in Refuse - and logs every request.
    public class Provider {
        public Dictionary<int, byte[]> Files = new Dictionary<int, byte[]>();
        public HashSet<int> Refuse = new HashSet<int>();
        public List<string> Log = new List<string>();
        public int Fetches = 0;
        string root; long key; bool connected; Cf.CfCallback cb; Cf.CALLBACK_REGISTRATION[] table;
        IntPtr identity = IntPtr.Zero;

        public void Register(string rootPath) {
            root = rootPath;
            identity = Marshal.AllocHGlobal(16);
            for (int i = 0; i < 16; i++) Marshal.WriteByte(identity, i, (byte)(0x40 + i));
            var reg = new Cf.SYNC_REGISTRATION();
            reg.StructSize = (uint)Marshal.SizeOf(typeof(Cf.SYNC_REGISTRATION));
            reg.ProviderName = "upgrade_ V8 test provider";
            reg.ProviderVersion = "0.1";
            reg.SyncRootIdentity = identity; reg.SyncRootIdentityLength = 16;
            reg.FileIdentity = identity; reg.FileIdentityLength = 16;
            reg.ProviderId = new Guid("7b6c0c1e-5d2a-4f8e-9a3b-2c1d0e9f8a7b");
            var pol = new Cf.SYNC_POLICIES();
            pol.StructSize = (uint)Marshal.SizeOf(typeof(Cf.SYNC_POLICIES));
            pol.HydrationPrimary = 2;   // CF_HYDRATION_POLICY_FULL
            pol.PopulationPrimary = 3;  // CF_POPULATION_POLICY_ALWAYS_FULL - we pre-create every placeholder
            // CF_REGISTER_FLAG_DISABLE_ON_DEMAND_POPULATION_ON_ROOT (2) | MARK_IN_SYNC_ON_ROOT (4)
            int hr = Cf.CfRegisterSyncRoot(root, ref reg, ref pol, 6);
            Log.Add("CfRegisterSyncRoot hr=0x" + hr.ToString("X8"));
            if (hr < 0) throw new Exception("CfRegisterSyncRoot failed: 0x" + hr.ToString("X8"));
        }

        public void Connect() {
            cb = new Cf.CfCallback(OnCallback);
            table = new Cf.CALLBACK_REGISTRATION[2];
            table[0].Type = 0; // CF_CALLBACK_TYPE_FETCH_DATA
            table[0].Callback = Marshal.GetFunctionPointerForDelegate(cb);
            table[1].Type = 0xFFFFFFFF; table[1].Callback = IntPtr.Zero; // CF_CALLBACK_REGISTRATION_END
            int hr = Cf.CfConnectSyncRoot(root, table, IntPtr.Zero, 4 /* REQUIRE_FULL_FILE_PATH */, out key);
            Log.Add("CfConnectSyncRoot hr=0x" + hr.ToString("X8") + " key=" + key);
            if (hr < 0) throw new Exception("CfConnectSyncRoot failed: 0x" + hr.ToString("X8"));
            connected = true;
        }

        // A directory placeholder: parents of placeholders must themselves be
        // placeholders (a plain directory under the root gets 0x8007017C from
        // CfCreatePlaceholders - learned on the rig, 2026-09-08). 0x10 =
        // FILE_ATTRIBUTE_DIRECTORY; flags 3 = DISABLE_ON_DEMAND_POPULATION |
        // MARK_IN_SYNC, so the filter never asks us to enumerate it.
        public void CreateDirectoryPlaceholder(string relName) {
            var arr = new Cf.PLACEHOLDER_CREATE_INFO[1];
            IntPtr idp = Marshal.AllocHGlobal(4); Marshal.WriteInt32(idp, -100);
            long now = DateTime.UtcNow.ToFileTimeUtc();
            arr[0].RelativeFileName = relName;
            arr[0].CreationTime = now; arr[0].LastAccessTime = now; arr[0].LastWriteTime = now; arr[0].ChangeTime = now;
            arr[0].FileAttributes = 0x10; arr[0].FileSize = 0;
            arr[0].FileIdentity = idp; arr[0].FileIdentityLength = 4; arr[0].Flags = 3;
            uint processed;
            int hr = Cf.CfCreatePlaceholders(root, arr, 1, 1, out processed);
            Marshal.FreeHGlobal(idp);
            Log.Add("CfCreatePlaceholders(dir " + relName + ") hr=0x" + hr.ToString("X8") + " result=0x" + arr[0].Result.ToString("X8"));
            if (hr < 0) throw new Exception("directory placeholder " + relName + " failed: 0x" + hr.ToString("X8"));
        }

        // Creates one dehydrated placeholder per entry of Files, named by id.
        // One CfCreatePlaceholders call per file, with the file's own
        // directory as the base: RelativeFileName must be a bare name (a
        // name with a separator gets 0x8007017C - learned on the rig,
        // 2026-09-08; the CloudMirror sample creates per directory too).
        public string CreatePlaceholders(Dictionary<int, string> names) {
            var sb = new System.Text.StringBuilder();
            long now = DateTime.UtcNow.ToFileTimeUtc();
            int failures = 0;
            foreach (var kv in Files) {
                string rel = names[kv.Key];
                string baseDir = root; string leaf = rel;
                int cut = rel.LastIndexOf('\\');
                if (cut >= 0) { baseDir = System.IO.Path.Combine(root, rel.Substring(0, cut)); leaf = rel.Substring(cut + 1); }
                var arr = new Cf.PLACEHOLDER_CREATE_INFO[1];
                IntPtr idp = Marshal.AllocHGlobal(4); Marshal.WriteInt32(idp, kv.Key);
                arr[0].RelativeFileName = leaf;
                arr[0].CreationTime = now; arr[0].LastAccessTime = now; arr[0].LastWriteTime = now; arr[0].ChangeTime = now;
                arr[0].FileAttributes = 0x80; // FILE_ATTRIBUTE_NORMAL
                arr[0].FileSize = kv.Value.LongLength;
                arr[0].FileIdentity = idp; arr[0].FileIdentityLength = 4;
                arr[0].Flags = 2; // CF_PLACEHOLDER_CREATE_FLAG_MARK_IN_SYNC
                uint processed;
                int hr = Cf.CfCreatePlaceholders(baseDir, arr, 1, 1 /* STOP_ON_ERROR */, out processed);
                Marshal.FreeHGlobal(idp);
                sb.Append("[" + rel + " hr=0x" + hr.ToString("X8") + " result=0x" + arr[0].Result.ToString("X8") + "] ");
                if (hr < 0 || processed != 1) failures++;
            }
            Log.Add("CfCreatePlaceholders " + sb.ToString());
            if (failures > 0) throw new Exception("CfCreatePlaceholders: " + sb.ToString());
            return sb.ToString();
        }

        void OnCallback(IntPtr infoPtr, IntPtr paramsPtr) {
            try {
                var info = (Cf.CALLBACK_INFO)Marshal.PtrToStructure(infoPtr, typeof(Cf.CALLBACK_INFO));
                var p = (Cf.FETCH_DATA_PARAMS)Marshal.PtrToStructure(paramsPtr, typeof(Cf.FETCH_DATA_PARAMS));
                int id = -1;
                if (info.FileIdentity != IntPtr.Zero && info.FileIdentityLength >= 4) id = Marshal.ReadInt32(info.FileIdentity);
                string path = info.NormalizedPath != IntPtr.Zero ? Marshal.PtrToStringUni(info.NormalizedPath) : "?";
                Fetches++;
                var oi = new Cf.OPERATION_INFO();
                oi.StructSize = (uint)Marshal.SizeOf(typeof(Cf.OPERATION_INFO));
                oi.Type = 0; // CF_OPERATION_TYPE_TRANSFER_DATA
                oi.ConnectionKey = info.ConnectionKey; oi.TransferKey = info.TransferKey; oi.RequestKey = info.RequestKey;
                var tp = new Cf.TRANSFER_DATA_PARAMS();
                tp.ParamSize = (uint)Marshal.SizeOf(typeof(Cf.TRANSFER_DATA_PARAMS));
                int hr;
                if (id < 0 || !Files.ContainsKey(id) || Refuse.Contains(id)) {
                    tp.CompletionStatus = unchecked((int)0xC0000001); // STATUS_UNSUCCESSFUL - the provider cannot deliver
                    tp.Buffer = IntPtr.Zero; tp.Offset = p.RequiredFileOffset; tp.Length = p.RequiredLength;
                    hr = Cf.CfExecute(ref oi, ref tp);
                    Log.Add("FETCH_DATA id=" + id + " path=" + path + " off=" + p.RequiredFileOffset + " len=" + p.RequiredLength + " -> REFUSED hr=0x" + hr.ToString("X8"));
                    return;
                }
                byte[] data = Files[id];
                var h = GCHandle.Alloc(data, GCHandleType.Pinned);
                try {
                    tp.CompletionStatus = 0;
                    tp.Buffer = h.AddrOfPinnedObject(); tp.Offset = 0; tp.Length = data.LongLength;
                    hr = Cf.CfExecute(ref oi, ref tp);
                } finally { h.Free(); }
                Log.Add("FETCH_DATA id=" + id + " path=" + path + " off=" + p.RequiredFileOffset + " len=" + p.RequiredLength + " -> sent " + data.LongLength + " bytes hr=0x" + hr.ToString("X8"));
            } catch (Exception e) {
                Log.Add("callback exception: " + e.Message);
            }
        }

        public void Teardown() {
            if (connected) { int hr = Cf.CfDisconnectSyncRoot(key); Log.Add("CfDisconnectSyncRoot hr=0x" + hr.ToString("X8")); connected = false; }
            if (root != null) { int hr = Cf.CfUnregisterSyncRoot(root); Log.Add("CfUnregisterSyncRoot hr=0x" + hr.ToString("X8")); }
            if (identity != IntPtr.Zero) { Marshal.FreeHGlobal(identity); identity = IntPtr.Zero; }
        }
    }
}
'@

function Initialize-CfProvider {
    if (-not ('UpgV8.Provider' -as [type])) { Add-Type -TypeDefinition $CfSource }
}

# =============================================================================
#  pure helpers
# =============================================================================

function Get-Sha256Hex {
    param([byte[]]$Bytes)
    $h = [Security.Cryptography.SHA256]::Create()
    try { ([BitConverter]::ToString($h.ComputeHash($Bytes)) -replace '-', '').ToLower() } finally { $h.Dispose() }
}

function Get-FileSha256Hex {
    param([string]$Path)
    $fs = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
    try {
        $h = [Security.Cryptography.SHA256]::Create()
        try { ([BitConverter]::ToString($h.ComputeHash($fs)) -replace '-', '').ToLower() } finally { $h.Dispose() }
    } finally { $fs.Dispose() }
}

function New-TestBytes {
    # Deterministic pseudo-random content: seed per file, so the ground
    # truth can be re-derived and nothing about it is compressible.
    param([int]$Seed, [int]$Length)
    $r = New-Object System.Random($Seed)
    $b = New-Object byte[] $Length
    if ($Length -gt 0) { $r.NextBytes($b) }
    , $b
}

function Test-IsPlaceholder {
    param([int]$Attributes)
    [bool](($Attributes -band $FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS) -or ($Attributes -band $FILE_ATTRIBUTE_OFFLINE))
}

function Get-V8Result {
    # Pure classifier -> the CSV result vocabulary
    # (docs/validation-results/README.md). Inputs are counts the run
    # established.
    param(
        [int]$Files, [int]$PlaceholdersConfirmed,
        [int]$Materialized, [int]$BytesVerified,
        [int]$RefusedExpected, [int]$RefusedReported,
        [int]$HarvestExit, [bool]$WrongBytes, [bool]$RealProvider
    )
    $servable = $Files - $RefusedExpected
    if ($PlaceholdersConfirmed -ne $Files)          { return 'setup-failed' }
    if ($WrongBytes)                                 { return 'wrong-bytes' }
    if ($RefusedExpected -gt 0) {
        if ($RefusedReported -ne $RefusedExpected)   { return 'refusal-missed' }
        if ($HarvestExit -eq 0)                      { return 'refusal-missed' }
    } elseif ($HarvestExit -ne 0)                    { return 'not-materialized' }
    if ($Materialized -ne $servable)                 { return 'not-materialized' }
    if ($BytesVerified -ne $servable)                { return 'not-materialized' }
    if ($RealProvider) { 'pass' } else { 'pass-plumbing' }
}

function Resolve-ResultsCsv {
    if ($ResultsCsv) { return $ResultsCsv }
    $repo = Join-Path $PSScriptRoot '..\..\docs\validation-results\v8-materialize.csv'
    try { $repo = [IO.Path]::GetFullPath($repo) } catch { }
    if (Test-Path (Split-Path $repo -Parent)) { return $repo }
    Join-Path $env:ProgramData 'upgrade_\v8\v8-materialize.csv'
}

function Write-EvidenceRow {
    param([hashtable]$Row)
    $csv = Resolve-ResultsCsv
    $dir = Split-Path $csv -Parent
    if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Path $dir -Force | Out-Null }
    if (-not (Test-Path $csv)) {
        [IO.File]::WriteAllText($csv, $CsvHeader + "`r`n", (New-Object Text.UTF8Encoding($false)))
    }
    function Esc { param($v) '"' + (($v -as [string]) -replace '"', '""') + '"' }
    $line = @(
        Esc((Get-Date).ToUniversalTime().ToString('o'))
        Esc($HarnessVersion); Esc($Row.OsBuild); Esc($Row.Provider)
        Esc($Row.Files); Esc($Row.Bytes); Esc($Row.PlaceholdersConfirmed)
        Esc($Row.Materialized); Esc($Row.BytesVerified)
        Esc($Row.RefusedExpected); Esc($Row.RefusedReported); Esc($Row.HarvestExit)
        Esc($Row.Result); Esc($Row.Notes)
    ) -join ','
    Add-Content -Path $csv -Value $line -Encoding UTF8
    $csv
}

function Invoke-HarvesterSeam {
    # The materialization step, in its own process - as it will run for
    # real: evaluate is never the sync provider.
    param([string]$Dir, [string]$ResultJson, [int]$TimeoutSec)
    $args = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$Harvester`"",
              '-MaterializePath', "`"$Dir`"", '-MaterializeResult', "`"$ResultJson`"",
              '-MaterializeTimeoutSec', "$TimeoutSec")
    $out = Join-Path (Split-Path $ResultJson -Parent) 'harvester-stdout.txt'
    $p = Start-Process -FilePath 'powershell.exe' -ArgumentList ($args -join ' ') -Wait -PassThru -NoNewWindow `
            -RedirectStandardOutput $out
    [pscustomobject]@{ ExitCode = $p.ExitCode; Stdout = (Get-Content $out -Raw -ErrorAction SilentlyContinue) }
}

# =============================================================================
#  the cfapi leg
# =============================================================================

function Invoke-CfapiLeg {
    Initialize-CfProvider
    $os = Get-CimInstance Win32_OperatingSystem
    $work = if ($WorkDir) { $WorkDir } else { Join-Path $env:TEMP ('upgrade-v8-' + [guid]::NewGuid().ToString('N').Substring(0, 8)) }
    $root = Join-Path $work 'root'
    # our scratch, and a stale root from an aborted run would carry old placeholders
    if (Test-Path $root) { & cmd.exe /c "rd /s /q `"$root`"" 2>&1 | Out-Null }
    New-Item -ItemType Directory -Path $root -Force | Out-Null
    New-Line ''
    New-Line "  upgrade_  V8 materialization test  -  cfapi test provider  (harness $HarnessVersion)" 'Cyan'
    New-Line "  $($os.Caption) build $($os.BuildNumber)   sync root: $root" 'DarkGray'
    New-Line ''

    # Ground truth: sizes that straddle the 4 KiB boundaries the filter
    # transfers in, one big enough to need several, and one the provider
    # will refuse to serve.
    $spec = @(
        @{ Id = 1; Name = 'one-byte.bin';     Length = 1 }
        @{ Id = 2; Name = 'under-4k.bin';     Length = 4095 }
        @{ Id = 3; Name = 'exactly-4k.bin';   Length = 4096 }
        @{ Id = 4; Name = 'photo-ish.jpg';    Length = 1048593 }
        @{ Id = 5; Name = 'sub\nested.docx';  Length = 70001 }
        @{ Id = 6; Name = 'big.mp4';          Length = 5242880 }
        @{ Id = 7; Name = 'unservable.pdf';   Length = 12345; Refuse = $true }
    )
    $prov = New-Object UpgV8.Provider
    $truth = @{}
    $names = New-Object 'System.Collections.Generic.Dictionary[int,string]'
    foreach ($f in $spec) {
        $bytes = New-TestBytes -Seed (1000 + $f.Id) -Length $f.Length
        $prov.Files[$f.Id] = $bytes
        $names[$f.Id] = $f.Name
        if ($f.Refuse) { [void]$prov.Refuse.Add($f.Id) }
        $truth[$f.Id] = Get-Sha256Hex -Bytes $bytes
    }

    $row = @{ OsBuild = $os.BuildNumber; Provider = 'cfapi-test-provider'; Files = $spec.Count
              Bytes = (($spec | ForEach-Object { $_.Length } | Measure-Object -Sum).Sum)
              PlaceholdersConfirmed = 0; Materialized = 0; BytesVerified = 0
              RefusedExpected = @($spec | Where-Object { $_.Refuse }).Count; RefusedReported = 0
              HarvestExit = -1; Result = 'error'; Notes = '' }
    $notes = @()
    $wrongBytes = $false
    try {
        New-Line '  registering sync root and connecting the provider...' 'DarkGray'
        $prov.Register($root)
        $prov.Connect()
        New-Line '  creating dehydrated placeholders...' 'DarkGray'
        $prov.CreateDirectoryPlaceholder('sub')
        $prov.CreatePlaceholders($names) | Out-Null

        # --- step 2: confirm they are real placeholders -------------------
        New-Line ''
        foreach ($f in $spec) {
            $path = Join-Path $root $f.Name
            $attr = [int][IO.File]::GetAttributes($path)
            $len  = (Get-Item -LiteralPath $path -Force).Length
            $alloc = [Upg.NativeFile]::AllocatedBytes($path)
            $isPh = (Test-IsPlaceholder -Attributes $attr) -and ($len -eq $f.Length) -and ($alloc -eq 0)
            if ($isPh) { $row.PlaceholdersConfirmed++ }
            New-Line ("  {0} {1,-18} len={2,-9} alloc={3,-6} attrs=0x{4:X}" -f $(if ($isPh) { 'placeholder' } else { 'NOT-PLACEHOLDER' }), $f.Name, $len, $alloc, $attr) $(if ($isPh) { 'DarkGray' } else { 'Red' })
        }
        if ($row.PlaceholdersConfirmed -ne $spec.Count) {
            $notes += "setup: only $($row.PlaceholdersConfirmed)/$($spec.Count) files came out as dehydrated placeholders"
        } else {
            # --- step 3: the harvester, in its own process ------------------
            New-Line ''
            New-Line '  running the harvester materialization seam in a separate process...' 'DarkGray'
            $resultJson = Join-Path $work 'materialize-result.json'
            $h = Invoke-HarvesterSeam -Dir $root -ResultJson $resultJson -TimeoutSec $TimeoutSec
            $row.HarvestExit = $h.ExitCode
            $res = if (Test-Path $resultJson) { Get-Content $resultJson -Raw | ConvertFrom-Json } else { $null }
            if (-not $res) { $notes += "harvester wrote no result (exit $($h.ExitCode))" }

            # --- step 4: verify against the ground truth ---------------------
            New-Line ''
            foreach ($f in $spec) {
                $path = Join-Path $root $f.Name
                $rec = $null
                if ($res) { $rec = @($res.Files | Where-Object { $_.Path -eq $path }) | Select-Object -First 1 }
                $attr = [int][IO.File]::GetAttributes($path)
                $alloc = [Upg.NativeFile]::AllocatedBytes($path)
                $reported = if ($rec) { [bool]$rec.Materialized } else { $false }
                if ($f.Refuse) {
                    # The refuse arm: the harvester must say this one FAILED,
                    # and the file must still be a placeholder.
                    $stillPh = Test-IsPlaceholder -Attributes $attr
                    $ok = (-not $reported) -and $stillPh
                    if (-not $reported) { $row.RefusedReported++ }
                    New-Line ("  {0} {1,-18} reported={2} still-placeholder={3} alloc={4} error='{5}'" -f $(if ($ok) { 'refused-ok ' } else { 'REFUSAL-MISSED' }), $f.Name, $reported, $stillPh, $alloc, $(if ($rec) { $rec.Error } else { '' })) $(if ($ok) { 'Green' } else { 'Red' })
                    continue
                }
                $hash = $null
                try { $hash = Get-FileSha256Hex -Path $path } catch { $notes += "$($f.Name): hash read failed: $($_.Exception.Message)" }
                $bytesOk = ($hash -eq $truth[$f.Id])
                if ($hash -and -not $bytesOk) { $wrongBytes = $true }
                $onDisk = (-not (Test-IsPlaceholder -Attributes $attr)) -and ($f.Length -eq 0 -or $alloc -gt 0)
                if ($reported) { $row.Materialized++ }
                if ($bytesOk -and $onDisk) { $row.BytesVerified++ }
                $ok = $reported -and $bytesOk -and $onDisk
                New-Line ("  {0} {1,-18} reported={2} sha256-match={3} alloc={4} attrs=0x{5:X}" -f $(if ($ok) { 'materialized' } else { 'FAILED      ' }), $f.Name, $reported, $bytesOk, $alloc, $attr) $(if ($ok) { 'Green' } else { 'Red' })
            }
            $notes += "provider served $($prov.Fetches) FETCH_DATA requests"
            if ($h.Stdout) { $notes += ('harvester: ' + (($h.Stdout -split "`r?`n" | Where-Object { $_ -match 'placeholders' } | Select-Object -Last 1) -replace '^\s+', '')) }
        }
    } catch {
        $notes += "exception: $($_.Exception.Message)"
        New-Line "  ! $($_.Exception.Message)" 'Red'
    } finally {
        New-Line ''
        New-Line '  tearing down the sync root...' 'DarkGray'
        try { $prov.Teardown() } catch { $notes += "teardown: $($_.Exception.Message)" }
        $logPath = Join-Path $work 'provider.log'
        try { $prov.Log | Out-File -FilePath $logPath -Encoding UTF8 } catch { }
        if (-not $KeepWork) {
            try { Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction Stop } catch {
                # a placeholder that survived unregistration can refuse a plain delete
                & cmd.exe /c "rd /s /q `"$root`"" 2>&1 | Out-Null
                if (Test-Path $root) { $notes += "cleanup: $root could not be removed ($($_.Exception.Message))" }
            }
        }
    }

    $row.Result = Get-V8Result -Files $spec.Count -PlaceholdersConfirmed $row.PlaceholdersConfirmed `
        -Materialized $row.Materialized -BytesVerified $row.BytesVerified `
        -RefusedExpected $row.RefusedExpected -RefusedReported $row.RefusedReported `
        -HarvestExit $row.HarvestExit -WrongBytes $wrongBytes -RealProvider $false
    $row.Notes = "[harness: os=$($os.Caption) $($os.BuildNumber); provider=cfapi-test; hydration=FULL; sizes=$(($spec | ForEach-Object { $_.Length }) -join '/')] " + ($notes -join ' | ')

    New-Line ''
    New-Line "  RESULT: $($row.Result)" $(if ($row.Result -like 'pass*') { 'Green' } else { 'Red' })
    $csv = Write-EvidenceRow -Row $row
    New-Line "  logged to $csv" 'Cyan'
    New-Line "  provider log: $logPath" 'DarkGray'
    New-Line ''
    if ($row.Result -notlike 'pass*') { exit 1 }
}

# =============================================================================
#  the OneDrive leg (the residue - a real account, never unattended)
# =============================================================================

function Invoke-OneDriveLeg {
    $os = Get-CimInstance Win32_OperatingSystem
    $odRoot = $env:OneDrive
    if (-not $odRoot -or -not (Test-Path $odRoot)) { throw 'OneDrive is not set up for this user ($env:OneDrive is empty).' }
    if (-not (Get-Process OneDrive -ErrorAction SilentlyContinue)) { throw 'OneDrive.exe is not running; the client must be signed in and syncing.' }
    $dir = Join-Path $odRoot ('upgrade_-v8-test-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
    New-Item -ItemType Directory -Path $dir -Force | Out-Null
    New-Line ''
    New-Line "  upgrade_  V8 materialization test  -  OneDrive  (harness $HarnessVersion)" 'Cyan'
    New-Line "  $($os.Caption) build $($os.BuildNumber)   test folder: $dir" 'DarkGray'
    New-Line ''
    $spec = @(
        @{ Id = 1; Name = 'one-byte.bin';   Length = 1 }
        @{ Id = 2; Name = 'under-4k.bin';   Length = 4095 }
        @{ Id = 3; Name = 'photo-ish.jpg';  Length = 1048593 }
        @{ Id = 4; Name = 'big.mp4';        Length = 5242880 }
    )
    $truth = @{}
    foreach ($f in $spec) {
        $b = New-TestBytes -Seed (2000 + $f.Id) -Length $f.Length
        [IO.File]::WriteAllBytes((Join-Path $dir $f.Name), $b)
        $truth[$f.Id] = Get-Sha256Hex -Bytes $b
    }
    $row = @{ OsBuild = $os.BuildNumber; Provider = 'onedrive'; Files = $spec.Count
              Bytes = (($spec | ForEach-Object { $_.Length } | Measure-Object -Sum).Sum)
              PlaceholdersConfirmed = 0; Materialized = 0; BytesVerified = 0
              RefusedExpected = 0; RefusedReported = 0; HarvestExit = -1; Result = 'error'; Notes = '' }
    $notes = @(); $wrongBytes = $false
    try {
        # Ask the client to dehydrate ("free up space"); it can only do so
        # once the upload is complete, so this doubles as the sync wait.
        New-Line '  asking OneDrive to free up space on the test files (attrib +U -P)...' 'DarkGray'
        $deadline = (Get-Date).AddSeconds($TimeoutSec)
        do {
            foreach ($f in $spec) { & attrib.exe +U -P ("`"$(Join-Path $dir $f.Name)`"") 2>&1 | Out-Null }
            Start-Sleep -Seconds 5
            $row.PlaceholdersConfirmed = 0
            foreach ($f in $spec) {
                $path = Join-Path $dir $f.Name
                $attr = [int][IO.File]::GetAttributes($path)
                if ((Test-IsPlaceholder -Attributes $attr) -and ([Upg.NativeFile]::AllocatedBytes($path) -eq 0)) { $row.PlaceholdersConfirmed++ }
            }
            New-Line ("  {0}/{1} dehydrated..." -f $row.PlaceholdersConfirmed, $spec.Count) 'DarkGray'
        } while ($row.PlaceholdersConfirmed -lt $spec.Count -and (Get-Date) -lt $deadline)
        if ($row.PlaceholdersConfirmed -ne $spec.Count) {
            $notes += "setup: OneDrive dehydrated only $($row.PlaceholdersConfirmed)/$($spec.Count) files within $TimeoutSec s (upload not finished, or Files On-Demand off)"
        } else {
            $resultJson = Join-Path $env:TEMP ('upgrade-v8-onedrive-' + [guid]::NewGuid().ToString('N').Substring(0, 8) + '.json')
            New-Line '  running the harvester materialization seam in a separate process...' 'DarkGray'
            $h = Invoke-HarvesterSeam -Dir $dir -ResultJson $resultJson -TimeoutSec $TimeoutSec
            $row.HarvestExit = $h.ExitCode
            $res = if (Test-Path $resultJson) { Get-Content $resultJson -Raw | ConvertFrom-Json } else { $null }
            foreach ($f in $spec) {
                $path = Join-Path $dir $f.Name
                $rec = if ($res) { @($res.Files | Where-Object { $_.Path -eq $path }) | Select-Object -First 1 } else { $null }
                $reported = if ($rec) { [bool]$rec.Materialized } else { $false }
                $attr = [int][IO.File]::GetAttributes($path); $alloc = [Upg.NativeFile]::AllocatedBytes($path)
                $hash = $null; try { $hash = Get-FileSha256Hex -Path $path } catch { }
                $bytesOk = ($hash -eq $truth[$f.Id]); if ($hash -and -not $bytesOk) { $wrongBytes = $true }
                $onDisk = (-not (Test-IsPlaceholder -Attributes $attr)) -and ($alloc -gt 0)
                if ($reported) { $row.Materialized++ }
                if ($bytesOk -and $onDisk) { $row.BytesVerified++ }
                $ok = $reported -and $bytesOk -and $onDisk
                New-Line ("  {0} {1,-18} reported={2} sha256-match={3} alloc={4} attrs=0x{5:X}" -f $(if ($ok) { 'materialized' } else { 'FAILED      ' }), $f.Name, $reported, $bytesOk, $alloc, $attr) $(if ($ok) { 'Green' } else { 'Red' })
            }
            Remove-Item $resultJson -Force -ErrorAction SilentlyContinue
        }
    } catch {
        $notes += "exception: $($_.Exception.Message)"
    } finally {
        if (-not $KeepWork) { Remove-Item -LiteralPath $dir -Recurse -Force -ErrorAction SilentlyContinue }
    }
    $row.Result = Get-V8Result -Files $spec.Count -PlaceholdersConfirmed $row.PlaceholdersConfirmed `
        -Materialized $row.Materialized -BytesVerified $row.BytesVerified -RefusedExpected 0 -RefusedReported 0 `
        -HarvestExit $row.HarvestExit -WrongBytes $wrongBytes -RealProvider $true
    $odVer = try { (Get-Item (Get-Process OneDrive | Select-Object -First 1).Path).VersionInfo.ProductVersion } catch { 'unknown' }
    $row.Notes = "[harness: os=$($os.Caption) $($os.BuildNumber); provider=OneDrive $odVer; sizes=$(($spec | ForEach-Object { $_.Length }) -join '/')] " + ($notes -join ' | ')
    New-Line ''
    New-Line "  RESULT: $($row.Result)" $(if ($row.Result -like 'pass*') { 'Green' } else { 'Red' })
    $csv = Write-EvidenceRow -Row $row
    New-Line "  logged to $csv" 'Cyan'
    New-Line ''
    if ($row.Result -notlike 'pass*') { exit 1 }
}

# =============================================================================
#  self-test (logic only)
# =============================================================================

function Invoke-SelfTest {
    $cases = @(
        @{ Name = 'classify: everything served, verified, refused one reported, exit 3 is pass-plumbing'
           Run = { Get-V8Result -Files 7 -PlaceholdersConfirmed 7 -Materialized 6 -BytesVerified 6 -RefusedExpected 1 -RefusedReported 1 -HarvestExit 3 -WrongBytes $false -RealProvider $false }; Expect = 'pass-plumbing' }
        @{ Name = 'classify: the same against a real provider is pass'
           Run = { Get-V8Result -Files 4 -PlaceholdersConfirmed 4 -Materialized 4 -BytesVerified 4 -RefusedExpected 0 -RefusedReported 0 -HarvestExit 0 -WrongBytes $false -RealProvider $true }; Expect = 'pass' }
        @{ Name = 'classify: a placeholder that was never dehydrated is setup-failed'
           Run = { Get-V8Result -Files 7 -PlaceholdersConfirmed 6 -Materialized 6 -BytesVerified 6 -RefusedExpected 1 -RefusedReported 1 -HarvestExit 3 -WrongBytes $false -RealProvider $false }; Expect = 'setup-failed' }
        @{ Name = 'classify: a hash mismatch is wrong-bytes, above everything else'
           Run = { Get-V8Result -Files 7 -PlaceholdersConfirmed 7 -Materialized 6 -BytesVerified 5 -RefusedExpected 1 -RefusedReported 1 -HarvestExit 3 -WrongBytes $true -RealProvider $false }; Expect = 'wrong-bytes' }
        @{ Name = 'classify: the refused file reported as materialized is refusal-missed'
           Run = { Get-V8Result -Files 7 -PlaceholdersConfirmed 7 -Materialized 7 -BytesVerified 6 -RefusedExpected 1 -RefusedReported 0 -HarvestExit 3 -WrongBytes $false -RealProvider $false }; Expect = 'refusal-missed' }
        @{ Name = 'classify: a failure with exit code 0 is refusal-missed'
           Run = { Get-V8Result -Files 7 -PlaceholdersConfirmed 7 -Materialized 6 -BytesVerified 6 -RefusedExpected 1 -RefusedReported 1 -HarvestExit 0 -WrongBytes $false -RealProvider $false }; Expect = 'refusal-missed' }
        @{ Name = 'classify: a servable file left as a placeholder is not-materialized'
           Run = { Get-V8Result -Files 7 -PlaceholdersConfirmed 7 -Materialized 5 -BytesVerified 5 -RefusedExpected 1 -RefusedReported 1 -HarvestExit 3 -WrongBytes $false -RealProvider $false }; Expect = 'not-materialized' }
        @{ Name = 'classify: reported materialized but not on disk is not-materialized'
           Run = { Get-V8Result -Files 4 -PlaceholdersConfirmed 4 -Materialized 4 -BytesVerified 3 -RefusedExpected 0 -RefusedReported 0 -HarvestExit 0 -WrongBytes $false -RealProvider $true }; Expect = 'not-materialized' }
        @{ Name = 'placeholder: recall bit is a placeholder'
           Run = { Test-IsPlaceholder -Attributes 0x400020 }; Expect = $true }
        @{ Name = 'placeholder: pinned + archive is not'
           Run = { Test-IsPlaceholder -Attributes 0x80020 }; Expect = $false }
        @{ Name = 'bytes: the same seed gives the same bytes (ground truth is reproducible)'
           Run = { (Get-Sha256Hex (New-TestBytes -Seed 7 -Length 1000)) -eq (Get-Sha256Hex (New-TestBytes -Seed 7 -Length 1000)) }; Expect = $true }
        @{ Name = 'bytes: a different seed gives different bytes'
           Run = { (Get-Sha256Hex (New-TestBytes -Seed 7 -Length 1000)) -ne (Get-Sha256Hex (New-TestBytes -Seed 8 -Length 1000)) }; Expect = $true }
        @{ Name = 'struct: CF_PLACEHOLDER_CREATE_INFO marshals to 88 bytes (cfapi.h x64)'
           Run = { Initialize-CfProvider; [Runtime.InteropServices.Marshal]::SizeOf([type]'UpgV8.Cf+PLACEHOLDER_CREATE_INFO') }; Expect = 88 }
        @{ Name = 'struct: CF_CALLBACK_INFO marshals to 152 bytes'
           Run = { Initialize-CfProvider; [Runtime.InteropServices.Marshal]::SizeOf([type]'UpgV8.Cf+CALLBACK_INFO') }; Expect = 152 }
        @{ Name = 'struct: CF_OPERATION_INFO marshals to 48 bytes'
           Run = { Initialize-CfProvider; [Runtime.InteropServices.Marshal]::SizeOf([type]'UpgV8.Cf+OPERATION_INFO') }; Expect = 48 }
        @{ Name = 'struct: TRANSFER_DATA parameters marshal to 40 bytes'
           Run = { Initialize-CfProvider; [Runtime.InteropServices.Marshal]::SizeOf([type]'UpgV8.Cf+TRANSFER_DATA_PARAMS') }; Expect = 40 }
        @{ Name = 'struct: CF_SYNC_POLICIES marshals to 24 bytes'
           Run = { Initialize-CfProvider; [Runtime.InteropServices.Marshal]::SizeOf([type]'UpgV8.Cf+SYNC_POLICIES') }; Expect = 24 }
    )
    $failed = 0
    New-Line ''
    New-Line "  upgrade_  V8 materialization harness $HarnessVersion  -  SELF-TEST" 'Cyan'
    New-Line ''
    foreach ($c in $cases) {
        $got = & $c.Run
        if ("$got" -eq "$($c.Expect)") { New-Line "    PASS  $($c.Name)" 'Green' }
        else { New-Line "    FAIL  $($c.Name)  (expected '$($c.Expect)', got '$got')" 'Red'; $failed++ }
    }
    New-Line ''
    if ($failed -gt 0) { New-Line "  $failed check(s) failed" 'Red'; exit 1 }
    New-Line '  all checks passed' 'Green'
    New-Line ''
}

# =============================================================================
#  main
# =============================================================================

if (-not (Test-Path $Harvester)) { throw "harvester not found beside this script: $Harvester" }
# the harvester's NativeFile type (allocated-bytes read) is shared; load it
# without running the harvester by extracting only its Add-Type block
$hv = Get-Content $Harvester -Raw
if ($hv -match "(?s)(Add-Type -TypeDefinition @'\r?\n(?:.*?)public static class NativeFile.*?\r?\n'@)") {
    if (-not ('Upg.NativeFile' -as [type])) { Invoke-Expression $matches[1] }
} else { throw 'could not find the NativeFile type in the harvester' }

if ($SelfTest) { Invoke-SelfTest; return }
if ($OneDrive) { Invoke-OneDriveLeg; return }
Invoke-CfapiLeg
