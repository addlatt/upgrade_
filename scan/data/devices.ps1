# =============================================================================
#  upgrade_ / device knowledge base
# =============================================================================
#  Community-maintained. Adding a device is a small, self-contained PR:
#  find the PCI ID, add one line, cite how you know.
#
#  Fields:
#    Driver     in-tree kernel module (or 'proprietary' / 'none')
#    MinKernel  earliest kernel with usable support. '' = long-supported
#    Status     ok   - works out of the box
#               warn - works with a caveat (firmware, recent kernel, config)
#               fail - does not work / needs hardware replacement
#    Note       one line, shown to the user verbatim
#
#  PCI IDs are 'VVVV:DDDD' lowercase hex. Find yours in the report's
#  "unmatched devices" section and open a PR.
# =============================================================================

function Get-UpgWifiDatabase {
    # Exact-match table, consulted before the vendor fallback below.
    @{
        # --- Intel : iwlwifi, the reliable case -------------------------------
        '8086:24f3' = @{ Name='Intel 8260';        Driver='iwlwifi'; MinKernel='';     Status='ok' }
        '8086:24fd' = @{ Name='Intel 8265';        Driver='iwlwifi'; MinKernel='';     Status='ok' }
        '8086:2723' = @{ Name='Intel AX200';       Driver='iwlwifi'; MinKernel='5.1';  Status='ok' }
        '8086:2725' = @{ Name='Intel AX210';       Driver='iwlwifi'; MinKernel='5.10'; Status='ok' }
        '8086:51f0' = @{ Name='Intel AX211';       Driver='iwlwifi'; MinKernel='5.16'; Status='ok' }
        '8086:51f1' = @{ Name='Intel AX211';       Driver='iwlwifi'; MinKernel='5.16'; Status='ok' }
        '8086:54f0' = @{ Name='Intel AX211';       Driver='iwlwifi'; MinKernel='5.16'; Status='ok' }
        '8086:272b' = @{ Name='Intel BE200';       Driver='iwlwifi'; MinKernel='6.7';  Status='warn'
                         Note='Wi-Fi 7 card. Needs a recent kernel; some vendors ship it on boards that only whitelist it under Windows.' }

        # --- MediaTek : good modern support, but kernel-version sensitive -----
        '14c3:7961' = @{ Name='MediaTek MT7921';   Driver='mt7921e'; MinKernel='5.15'; Status='ok' }
        '14c3:0616' = @{ Name='MediaTek MT7922';   Driver='mt7921e'; MinKernel='5.19'; Status='ok' }
        '14c3:7922' = @{ Name='MediaTek MT7922';   Driver='mt7921e'; MinKernel='5.19'; Status='ok' }
        '14c3:7925' = @{ Name='MediaTek MT7925';   Driver='mt7925e'; MinKernel='6.7';  Status='warn'
                         Note='Wi-Fi 7 card, supported from kernel 6.7. On older kernels there is no driver at all - you will boot with no wireless.' }

        # --- Realtek : works, historically rough --------------------------
        '10ec:8821' = @{ Name='Realtek RTL8821CE'; Driver='rtw88';   MinKernel='5.8';  Status='warn'
                         Note='Known for unstable throughput and suspend/resume drops on some models.' }
        '10ec:c821' = @{ Name='Realtek RTL8821CE'; Driver='rtw88';   MinKernel='5.8';  Status='warn'
                         Note='Known for unstable throughput and suspend/resume drops on some models.' }
        '10ec:8852' = @{ Name='Realtek RTL8852AE'; Driver='rtw89';   MinKernel='5.16'; Status='ok' }
        '10ec:b852' = @{ Name='Realtek RTL8852BE'; Driver='rtw89';   MinKernel='6.1';  Status='ok' }
        '10ec:c852' = @{ Name='Realtek RTL8852CE'; Driver='rtw89';   MinKernel='6.1';  Status='ok' }

        # --- Qualcomm/Atheros ------------------------------------------------
        '168c:0030' = @{ Name='Atheros AR9300';    Driver='ath9k';   MinKernel='';     Status='ok' }
        '168c:003e' = @{ Name='Qualcomm QCA6174';  Driver='ath10k';  MinKernel='';     Status='ok' }
        '17cb:1101' = @{ Name='Qualcomm QCA6390';  Driver='ath11k';  MinKernel='5.13'; Status='warn'
                         Note='Firmware is fussy; Bluetooth coexistence bugs are common. Usually works, occasionally does not.' }
        '17cb:1103' = @{ Name='Qualcomm WCN6855';  Driver='ath11k';  MinKernel='5.16'; Status='warn'
                         Note='Needs vendor firmware blobs that some distros ship late.' }

        # --- Broadcom : the classic problem ----------------------------------
        '14e4:43a3' = @{ Name='Broadcom BCM4350';  Driver='broadcom-wl'; MinKernel=''; Status='fail'
                         Note='Broadcom. No working in-tree driver. Needs the out-of-tree wl module, which must be compiled - and you cannot download it without a network connection you do not yet have. Bring a USB Ethernet adapter or a phone for USB tethering.' }
        '14e4:43b1' = @{ Name='Broadcom BCM4352';  Driver='broadcom-wl'; MinKernel=''; Status='fail'
                         Note='Broadcom. Same chicken-and-egg problem: the driver needs a network connection to install. Bring USB Ethernet or a tethered phone.' }
        '14e4:4365' = @{ Name='Broadcom BCM43142'; Driver='broadcom-wl'; MinKernel=''; Status='fail'
                         Note='Broadcom. Poorly supported even with the out-of-tree driver. Replacing the M.2 card with an Intel AX200 (~$20) is the standard fix.' }
    }
}

function Get-UpgWifiVendorFallback {
    # Used when the exact device is unknown. Vendor reputation is a real signal.
    @{
        '8086' = @{ Vendor='Intel';    Driver='iwlwifi'; Status='ok'
                    Note='Unrecognised Intel card. Intel wireless is in-tree and near-universally supported; very likely fine.' }
        '14c3' = @{ Vendor='MediaTek'; Driver='mt76';    Status='warn'
                    Note='Unrecognised MediaTek card. Recent MediaTek parts are supported but often only on very new kernels - prefer a current-release distro.' }
        '10ec' = @{ Vendor='Realtek';  Driver='rtw88/rtw89'; Status='warn'
                    Note='Unrecognised Realtek card. Support ranges from fine to flaky depending on the exact part.' }
        '17cb' = @{ Vendor='Qualcomm'; Driver='ath11k/ath12k'; Status='warn'
                    Note='Unrecognised Qualcomm card. Newer ath12k parts need very recent kernels and firmware.' }
        '168c' = @{ Vendor='Atheros';  Driver='ath9k/ath10k'; Status='ok'
                    Note='Unrecognised Atheros card. This family is generally well supported.' }
        '14e4' = @{ Vendor='Broadcom'; Driver='broadcom-wl'; Status='fail'
                    Note='Unrecognised Broadcom card. Broadcom wireless is the single most common cause of "no internet after installing Linux". Bring USB Ethernet or a tethered phone.' }
    }
}

function Get-UpgGpuDatabase {
    @{
        # AMD integrated - amdgpu is in-tree and excellent, but new silicon
        # genuinely does not work until the kernel that added it.
        '1002:150e' = @{ Name='AMD Radeon 890M (Strix Point)'; Driver='amdgpu'; MinKernel='6.10'; Status='warn'
                         Note='Strix Point graphics need kernel 6.10 or newer. On an older kernel you get software rendering or a black screen.' }
        '1002:1900' = @{ Name='AMD Radeon 780M (Phoenix)';     Driver='amdgpu'; MinKernel='6.4';  Status='ok' }
        '1002:164e' = @{ Name='AMD Raphael iGPU';              Driver='amdgpu'; MinKernel='6.1';  Status='ok' }
    }
}

function Get-UpgGpuVendorRules {
    @{
        '8086' = @{ Vendor='Intel'; Driver='i915 / xe'; Status='ok'
                    Note='Intel graphics are in-tree and generally work with zero configuration.' }
        '1002' = @{ Vendor='AMD';   Driver='amdgpu';    Status='ok'
                    Note='AMD graphics are in-tree and generally work with zero configuration. Very recent APUs need a matching recent kernel.' }
        '10de' = @{ Vendor='NVIDIA'; Driver='nvidia (proprietary)'; Status='warn'
                    Note='NVIDIA needs the proprietary driver for usable performance. Choose a distro that installs it for you - Linux Mint, Pop!_OS, and Fedora all do. Avoid distros that make you do it by hand.' }
    }
}

function Get-UpgAudioQuirks {
    # Matched against ACPI / HDAUDIO hardware IDs, substring, case-insensitive.
    # These are the "everything works except the speakers" cases - the single
    # most common post-install complaint on 2022+ laptops.
    @(
        @{ Match='CSC3551'; Name='Cirrus Logic CS35L41 smart amplifier'; MinKernel='6.7'; Status='warn'
           Note='Laptop speakers are driven by Cirrus smart amps. Headphones work immediately; internal speakers stay silent until kernel 6.7+ with the right firmware. Very common on ASUS, Dell and Lenovo 2023+ models.' }
        @{ Match='CSC3556'; Name='Cirrus Logic CS35L56 smart amplifier'; MinKernel='6.7'; Status='warn'
           Note='Laptop speakers are driven by Cirrus smart amps. Headphones work immediately; internal speakers may stay silent until kernel 6.7+ with vendor firmware extracted from Windows.' }
        @{ Match='TXNW2781'; Name='TI TAS2781 smart amplifier';          MinKernel='6.6'; Status='warn'
           Note='TI smart amps, common on Lenovo. Internal speakers need kernel 6.6+ and firmware; headphones are unaffected.' }
        @{ Match='INTC10'; Name='Intel SST / SOF DSP audio';             MinKernel='6.0'; Status='warn'
           Note='DSP-based audio. Needs the SOF firmware package, which most current distros ship - but audio may be silent on older or minimal installs.' }
    )
}

function Get-UpgVmdDeviceIds {
    # Intel VMD / RST controllers. If one of these is present and active, the
    # SSD is invisible to every Linux installer until BIOS is switched to AHCI.
    @( '8086:9a0b','8086:a77f','8086:467f','8086:7d0b','8086:e0b0',
       '8086:09ab','8086:201d','8086:2010','8086:7ec0','8086:ad0b' )
}

function Get-UpgVendorQuirks {
    # Matched as substring against system manufacturer + model.
    @(
        @{ Match='Microsoft Corporation|Surface'; Status='warn'
           Note='Surface hardware needs the linux-surface kernel and firmware for touch, cameras, sleep and sometimes Wi-Fi. Plan on following the linux-surface project setup, not a plain install.' }
        @{ Match='Apple Inc\.|MacBook|iMac|Macmini'; Status='warn'
           Note='Apple hardware. Intel Macs with a T2 security chip need special installation media for the keyboard, trackpad and SSD. Apple Silicon Macs need Asahi Linux, which is a different process entirely.' }
        @{ Match='Framework'; Status='ok'
           Note='Framework laptops are explicitly Linux-supported by the manufacturer. This is about as smooth as it gets.' }
    )
}

function Get-UpgAppRiskDatabase {
    # Matched as regex against installed-program display names.
    # Severity: blocker = no realistic Linux path; friction = workable, different.
    @(
        @{ Match='Adobe (Photoshop|Illustrator|InDesign|Premiere|After Effects|Lightroom)'
           Severity='blocker'
           Note='Adobe Creative Cloud does not run on Linux and Adobe has stated it will not. Alternatives (GIMP, Krita, Inkscape, DaVinci Resolve, Kdenlive) are capable but are not drop-in replacements, and none open .psd/.ai files with full fidelity. If you are paid to use these, do not convert this machine.' }
        @{ Match='Microsoft (Office|365)|Microsoft Word|Excel|PowerPoint'
           Severity='friction'
           Note='Desktop Office does not run on Linux. Office on the web works fine in a browser, and LibreOffice or OnlyOffice handle most documents - but complex Excel macros and heavily formatted Word files will break.' }
        @{ Match='QuickBooks|Sage 50|TurboTax|H&R Block'
           Severity='blocker'
           Note='Accounting and tax software is Windows-only and rarely has a usable Linux equivalent. If you run a business on this, keep a Windows machine or a Windows VM.' }
        @{ Match='AutoCAD|SolidWorks|Fusion 360|Revit|Altium|ArcGIS'
           Severity='blocker'
           Note='Professional CAD/EDA/GIS software is Windows-only. FreeCAD, KiCad and QGIS exist and are good, but file compatibility with commercial formats is imperfect.' }
        @{ Match='Valorant|Riot (Client|Vanguard)|Fortnite|Easy ?Anti ?Cheat|BattlEye|Call of Duty|Destiny 2|Apex Legends'
           Severity='blocker'
           Note='Kernel-level anti-cheat blocks Linux by design. These specific games will not run, no matter what you do. Most other games work well through Steam Proton - check protondb.com for your library.' }
        @{ Match='Steam'
           Severity='info'
           Note='Steam runs natively on Linux and Proton makes most Windows games work. Check protondb.com for your specific library before committing.' }
        @{ Match='iTunes|Apple Music'
           Severity='friction'
           Note='No Linux iTunes. iPhone sync and backup is limited; Apple Music works in a browser.' }
        @{ Match='OneDrive'
           Severity='friction'
           Note='No official Linux OneDrive client. Third-party sync clients exist and work, but setup is manual.' }
        @{ Match='(?<!Microsoft )Visual Studio (?!Code)'
           Severity='friction'
           Note='Visual Studio (the full IDE) is Windows-only. VS Code, JetBrains Rider and the .NET SDK all run natively on Linux.' }
    )
}
