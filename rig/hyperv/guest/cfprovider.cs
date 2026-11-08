// The Cloud Files test provider (VALIDATION V8's cfapi leg, evaluate/windows/Test-Materialize.ps1,
// git 20d9a10^), shared by pull-plant.ps1 and ntfs-fixture.ps1. Rig tooling, not product.
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
        // files sent only in part: the first PartialBytes bytes, then a refusal for the rest
        public HashSet<int> Partial = new HashSet<int>();
        public long PartialBytes = 65536;
        public ushort Hydration = 2; // CF_HYDRATION_POLICY_FULL; 0 = PARTIAL
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
            pol.HydrationPrimary = Hydration;   // 2 = CF_HYDRATION_POLICY_FULL (the default), 0 = PARTIAL
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
                long send = Partial.Contains(id) ? Math.Min(PartialBytes, data.LongLength) : data.LongLength;
                var h = GCHandle.Alloc(data, GCHandleType.Pinned);
                try {
                    tp.CompletionStatus = 0;
                    tp.Buffer = h.AddrOfPinnedObject(); tp.Offset = 0; tp.Length = send;
                    hr = Cf.CfExecute(ref oi, ref tp);
                    if (send < data.LongLength && p.RequiredFileOffset + p.RequiredLength > send) {
                        // the rest is refused: the read fails, the bytes sent stay on the disk
                        tp.CompletionStatus = unchecked((int)0xC0000001);
                        tp.Buffer = IntPtr.Zero; tp.Offset = send; tp.Length = p.RequiredFileOffset + p.RequiredLength - send;
                        Cf.CfExecute(ref oi, ref tp);
                    }
                } finally { h.Free(); }
                Log.Add("FETCH_DATA id=" + id + " path=" + path + " off=" + p.RequiredFileOffset + " len=" + p.RequiredLength + " -> sent " + send + " of " + data.LongLength + " bytes hr=0x" + hr.ToString("X8"));
            } catch (Exception e) {
                Log.Add("callback exception: " + e.Message);
            }
        }

        // let go of the callbacks, keep the registration (and every placeholder as it is)
        public void Disconnect() {
            if (connected) { int hr = Cf.CfDisconnectSyncRoot(key); Log.Add("CfDisconnectSyncRoot hr=0x" + hr.ToString("X8")); connected = false; }
        }

        public void Teardown() {
            if (connected) { int hr = Cf.CfDisconnectSyncRoot(key); Log.Add("CfDisconnectSyncRoot hr=0x" + hr.ToString("X8")); connected = false; }
            if (root != null) { int hr = Cf.CfUnregisterSyncRoot(root); Log.Add("CfUnregisterSyncRoot hr=0x" + hr.ToString("X8")); }
            if (identity != IntPtr.Zero) { Marshal.FreeHGlobal(identity); identity = IntPtr.Zero; }
        }
    }
}
