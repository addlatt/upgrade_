# Pull arm (V3, V8; R8, R19, R20) - Windows side (guest, PowerShell Direct,
# ELEVATED). Windows PowerShell 5.1. Rig tooling, not product.
#
# Plants a test tree in the signed-in rig account's own Documents and
# Pictures, with every kind of entry settle-in's pull must sort right:
# ordinary files (odd names, an empty one, a 40 MB one, nested folders), a
# hidden desktop.ini, a junction to C:\Windows, a file symlink, and under a
# Cloud Files sync root (the API OneDrive is built on; the provider is the
# one VALIDATION V8's cfapi leg used, evaluate/windows/Test-Materialize.ps1,
# git 20d9a10^) two dehydrated placeholders and one placeholder downloaded
# by reading it (its cloud tag stays, its recall bit goes). Then a Chrome
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
foreach ($d in @($plant, $pplant)) { if (Test-Path -LiteralPath $d) { cmd /c "rmdir /s /q `"$d`"" | Out-Null } }
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

# --- the Cloud Files placeholders (the V8 provider's code, unchanged)
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
Add-Type -TypeDefinition $CfSource -Language CSharp
$root = Join-Path $plant 'Cloud'
New-Item -ItemType Directory -Force -Path $root | Out-Null
$prov = New-Object UpgV8.Provider
$cloudBytes = @{ 1 = (Bytes 70000); 2 = (Bytes 5000); 3 = (Bytes 123456) }
$names = New-Object 'System.Collections.Generic.Dictionary[int,string]'
$names[1] = 'only-in-cloud.docx'; $names[2] = 'Sub\also-in-cloud.xlsx'; $names[3] = 'downloaded.pdf'
foreach ($k in $cloudBytes.Keys) { $prov.Files[$k] = $cloudBytes[$k] }
$prov.Register($root)
$prov.Connect()
$prov.CreateDirectoryPlaceholder('Sub')
$prov.CreatePlaceholders($names) | Out-Null
# one downloaded by reading it through the filter: its bytes are now on the disk
$hydrated = Join-Path $root 'downloaded.pdf'
# read in another process, as the V8 leg did: the filter calls this process back for the bytes
$hsha = (powershell.exe -NoProfile -Command "(Get-FileHash -Algorithm SHA256 -LiteralPath '$hydrated').Hash").Trim().ToLower()
if ($hsha -ne ([BitConverter]::ToString((New-Object Security.Cryptography.SHA256Managed).ComputeHash($cloudBytes[3])) -replace '-', '').ToLower()) { throw 'the downloaded placeholder does not hash to its bytes' }
$expect += [ordered]@{ folder = 'Documents'; rel = 'pull-test/Cloud/downloaded.pdf'; size = 123456; sha256 = $hsha }
# disconnect, never unregister: the two others stay real placeholders, as OneDrive leaves them
$prov.Log | ForEach-Object { Log "  cf: $_" }
$attrs = @{}
foreach ($n in @('only-in-cloud.docx', 'Sub\also-in-cloud.xlsx', 'downloaded.pdf')) { $attrs[$n] = ('0x{0:X}' -f [int](Get-Item -LiteralPath (Join-Path $root $n) -Force).Attributes.value__) }
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
