# Device feasibility: what else could upgrade_ convert?

**Decided (2026-09-27, the owner):** smart TVs, and devices beyond Windows PCs
in general, go on the long-term plan. This page is the research that starts
it: a table of device families and how feasible each one is to bring into
this project.

**This is research, not evidence.** Every cell comes from web sources (listed
at the bottom of each section), not from a machine we touched. By rule #2, a
row here closes nothing. It tells us where to point a real machine next.
"unknown" means no source was found. Researched 2026-09-27; this goes stale
fast, so date any update.

## How a device is judged

A Linux build for the chip is necessary, but it isn't enough. A device is
only convertible when **all five doors** are open:

```text
1. a Linux build for this exact device       (the recipe exists)
2. firmware that will boot something else    (the door opens)
3. a way to start the switch from the old OS (you can reach the handle)
4. drivers for this device's parts           (screen, Wi-Fi, sound work)
5. a way back if it fails                    (the safety net)
```

Plus two questions about whether it's worth it:

- **Stranded audience:** are these devices losing support, so people need
  a way out?
- **For TVs and boxes:** do streaming apps still work afterwards? That's
  why people own them.

**Scope rule: only doors the maker opens on purpose.** Official boot from
USB or SD, official bootloader unlocks, documented developer modes. If the
only way in is breaking a security lock, the row says `blocked: needs
circumvention` and stops there. This project does not research or ship
exploits. That's a legal line (see "The rules" below) and a trust line
(rule #4: trust is spent once).

**Verdicts:**

```text
ready       all five doors open, a maintained Linux exists today
plausible   doors open, real gaps, worth a proper look
hard        possible, but breaks walk-away, safety or the stranded-audience fit
blocked     a door is shut (no build, or needs circumvention)
```

## The short answer

```text
BEST NEXT SOURCES (after Windows PCs)
  plausible  Intel Macs without T2 (pre-2018)   stranded, x86 UEFI, Fedora runs
  plausible  x86 handhelds (ROG Ally, Legion Go, MSI Claw)   Windows + UEFI already
  plausible  old Surface (x86)                  already Windows PCs; 11 models can't get Win 11
  plausible  Snapdragon X laptops               Fedora 44 on some models; not stranded

LONGER SHOTS
  hard       Intel Macs with T2 (2018-2020)     needs a manual Recovery visit
  hard       x86 Chromebooks                    developer mode wipes data first
  plausible  generic Android TV boxes           SD boot, but Kodi-type OS, not Fedora

BLOCKED TODAY
  blocked    every smart TV platform checked    app developer modes only
  blocked    branded sticks (Fire TV, Roku, Chromecast, Apple TV)
  blocked    iPads, Amazon Fire tablets, Samsung on One UI 8, 32-bit-only PCs

ALREADY SOMEONE ELSE'S JOB
  ready      Apple Silicon M1/M2 (and M3 since 2026-09-06): Fedora Asahi Remix
```

**TVs, in one line:** no smart TV checked can boot another OS by any
official means, and the few TV boxes that can would lose full-quality
Netflix. The TV path that works today runs the other way: **a converted old
laptop plugged into the TV by HDMI**, where x86 Chrome gets official
streaming DRM [T13].

---

## Smart TVs and streaming boxes

| Device | Linux build | Boots other OS | Way back | Stranded audience | Streaming after | Verdict |
|---|---|---|---|---|---|---|
| Samsung Tizen TVs | none for the TV | no: developer mode installs signed Tizen apps only [T10][T11] | n/a | 7-year upgrades only for 2024 and some 2023 models [T16]; Netflix dropping 2012-2015 sets [T3][T5] | n/a | blocked |
| LG webOS TVs | webOS Open Source Edition targets Raspberry Pi, not TVs [T38] | no: developer mode sideloads apps on a timer [T9] | n/a | 5 years for 2022+ models [T17]; pre-2015 sets losing Netflix [T3] | n/a | blocked |
| Google TV / Android TV sets (Sony, TCL, Hisense, Philips) | none | no maker documents an unlock [T36] | n/a | unknown | n/a | blocked |
| Roku TVs | none | no: developer mode is for Roku channels [T19] | n/a | Netflix dropped some older Roku devices [T5] | n/a | blocked |
| Amazon Fire TV sets | none | no official unlock found | n/a | Netflix ended 2014-2016 Fire TV devices, 2025-06-03 [T1][T2] | n/a | blocked |
| Vizio | none | no official path found | n/a | Netflix left 2012-2014 VIA sets [T5] | n/a | blocked |
| Hisense VIDAA | none | no [T23] | n/a | unknown | n/a | blocked |
| Generic Android TV box (Amlogic) | CoreELEC; LibreELEC on S905/S905X/D/S912 [T6][T14][T15]; Armbian community only [T21] | yes: SD, then USB, then internal; Android untouched [T6] | pull the card [T6] | unknown | Netflix only via a Kodi add-on, about 720p (forum claim) [T27] | plausible |
| Generic box (Rockchip, Allwinner) | LibreELEC; Armbian community [T15][T21][T40] | SD boot documented [T33][T34] | pull the card | unknown | Widevine L3 at most [T13] | hard |
| Nvidia Shield TV | LineageOS official page [T8]; no desktop Linux found | yes: Nvidia documents the unlock (wipes data) [T7] | Nvidia recovery images [T7] | still updated (Nov 2025) [T26] | usually drops to Widevine L3 after unlock [T39] | hard |
| Chromecast with Google TV, Google TV Streamer | none | no official unlock [T20] | n/a | older Chromecasts broke on 2025-03-09 (expired certificate) [T12] | n/a | blocked |
| Fire TV Stick, Roku sticks, Apple TV | none | no; new Fire sticks run the locked Vega OS [T18][T19] | n/a | Netflix dropping old models [T1][T4][T5] | n/a | blocked |

What this means:

- **The stranded audience is real but scattered.** Netflix alone dropped
  2014-2016 Fire TV devices (2025), started dropping many pre-2015 smart TVs
  and the PS3 (2026), and older Chromecasts stopped working when a
  certificate expired [T1][T3][T12]. No source gave a device count.
- **Future stranding may shrink:** Samsung (7 years) and LG (5 years) now
  promise longer update windows [T16][T17].
- **Even the open boxes break walk-away.** The documented Amlogic start is
  holding a reset button at power-on, and Wi-Fi works only on some chips
  [T6][T35].
- **Streaming is the deal-breaker.** Official Widevine on Linux exists only
  for Chrome on x86_64 [T13].

---

## Computers that aren't x86 Windows PCs

| Device | Linux build | Boots other OS | Way in (from the old OS) | Drivers | Way back | Stranded audience | Verdict |
|---|---|---|---|---|---|---|---|
| **Intel Macs without T2 (pre-2018)** | stock Fedora; Broadcom Wi-Fi needs `broadcom-wl` from RPM Fusion nonfree [C20] | yes, UEFI USB boot [C18] | `bless --setBoot --nextonly` is a documented one-shot next boot [C17], the `bcdedit bootsequence` analogue; may need Recovery with SIP on (unverified) [C18] | Broadcom `wl` can break on kernel updates [C20]; FaceTime camera needs firmware from macOS [C19] | macOS alongside (details unsourced) | all are off current macOS; macOS 27 is Apple Silicon only [C10][C11] | **plausible** |
| Intel Macs with T2 (2018-2020) | community Fedora ISO (t2linux) [C16] | yes, after setting "No Security" + external boot in Recovery by hand [C14][C15] | partly: the Recovery step is manual [C14] | suspend partial, camera out-of-tree, quiet mic, Wi-Fi firmware from macOS [C13] | dual boot only if partitioned by hand [C14] | only 4 models got macOS 26; the rest are off current macOS [C10] | hard |
| Apple Silicon M1/M2 | Fedora Asahi Remix 43 [C5] | yes, by Apple's design [C1][C8] | from macOS, but boot policy needs a hands-on approval [C2][C8][C9] | M1/M2 gaps: Thunderbolt, DP alt mode, Touch ID [C3][C4] | macOS stays [C7] | none: macOS 27 supports M1+ [C11] | ready via Asahi, not ours |
| Apple Silicon M3 | Asahi since 2026-09-06 (not M3 Ultra) [C2] | yes [C8] | from macOS [C2] | no sleep, HDMI off, weak 3D [C2] | macOS stays [C7] | none [C11] | plausible via Asahi |
| Apple Silicon M4/M5 | none installer-ready [C6] | policy allows it [C8] | none yet | unknown | n/a | none [C11] | blocked: no build yet |
| x86 Chromebooks | no official Fedora; community docs (chrultrabook) [C23] | only via developer mode; Google says other OSes are "not officially supported" [C21] | poor: entering dev mode wipes local data, needs a key combo [C21] | custom audio configs, keyboard remap [C23] | official recovery USB [C26] | 10 years for 2019+ platforms [C24]; 31 M sold in 2020 [C25] | hard |
| ARM Chromebooks | no Fedora; community kernels [C27][C28] | developer mode; no UEFI [C21][C28] | same wipe problem [C21] | per board | recovery USB [C26] | same policy [C24] | blocked for Fedora |
| Snapdragon X laptops | Fedora 44 aarch64 on ThinkPad T14s Gen 6, Yoga Slim 7x, X13s [C29][C30] | yes, UEFI [C31] | from Windows 11: our own path | audio and battery need DSP firmware from Windows [C29]; lags Windows [C31][C32] | keep Windows (Fedora's guide says to) [C29] | none: supported Windows 11 | plausible, not stranded |
| Surface, x86 | Fedora + linux-surface kernel [C36] | yes; third-party CA option [C34] | from Windows: our own path | cameras WIP, some suspend and audio gaps [C35] | Windows alongside | 11 models can't get Windows 11 [C37] | plausible (already in scope as Windows PCs) |
| Surface, ARM | not in linux-surface [C35]; patches upstreaming [C42] | yes [C34] | from Windows | audio/touch/suspend may not work [C42] | keep Windows | none [C37][C38] | hard |
| 32-bit-only x86 PCs | none: Fedora stopped i686 kernels in F31 [C39] | n/a | n/a | n/a | n/a | Windows 10 32-bit machines | blocked |

What this means:

- **Intel Macs without T2 are the best next source.** They're the largest
  group that is truly stranded right now, they're plain UEFI x86 (so the
  kickstart, `%pre` verifier and Fedora image carry over), and `bless
  --nextonly` is a documented one-shot boot. Their driver gaps (Broadcom
  Wi-Fi, the FaceTime camera firmware) fit the harvest pattern upgrade_
  already uses. **Owed before any claim:** whether `bless` works with SIP on,
  the fail-safe back to macOS, and their Secure Boot story (no primary
  source found).
- **T2 Macs can't be walk-away:** someone has to visit Recovery by hand. That
  matches `architecture.md`'s "hard maybe".
- **Apple Silicon stays Asahi's job**, as `architecture.md` already decided.
- **Chromebooks clash with the commit line:** developer mode wipes local data
  the moment it's switched on, before anything could be checked.
- **Snapdragon X is the only non-x86 family on Fedora's own wiki**, and its
  source OS is Windows, but nobody is stranded on it.

---

## Other devices

| Device | Linux build | Boots other OS | Way back | Stranded audience | Verdict |
|---|---|---|---|---|---|
| x86 handhelds (Legion Go S, ROG Ally, MSI Claw) | official SteamOS support; SteamOS 3.8 widened it [O19] | yes, x86 UEFI | vendor Windows recovery (unresearched) | Windows 10 handhelds | **plausible: closest to upgrade_'s model** |
| Steam Deck | already Linux (SteamOS) [O17] | yes; Valve documents dual boot [O18] | official recovery image [O18] | Valve still supports the LCD model [O17] | ready (not a target) |
| Raspberry Pi 4 / 5 | Fedora official on Pi 4 since F37; Fedora 44 on Pi 5 [O21][O22] | yes, SD/USB by design | swap the card | n/a | ready (reference point) |
| Unlockable phones (Pixel 3a, OnePlus 6/6T, Fairphone 4/5) | postmarketOS, Mobian, Ubuntu Touch [O12][O13][O14] | official unlock | vendor images | yes (old models) | plausible (niche) |
| Google Pixel Tablet | LineageOS official [O5] | official unlock | factory images | updates to June 2028 [O8] | hard for Linux |
| Samsung Galaxy on One UI 8 | n/a | no: unlocking removed [O9] | n/a | large | blocked |
| Xiaomi | per model | global: account-gated application; China: largely unavailable [O15] | per model | yes | hard / blocked |
| iPads, Amazon Fire tablets, carrier-locked Pixels | none | no [O10][O16] | n/a | yes: iPadOS 27 drops five models [O11] | blocked |
| PlayStation 3 | OtherOS removed in 2010; class action settled 2016 [O20] | no | n/a | n/a | blocked (history) |
| PS4/PS5/Xbox/Switch | not researched | no official path known (unverified) | n/a | n/a | blocked (unverified) |

**One catch for all phones and tablets:** unlocking an Android bootloader
normally wipes the device (well known, not re-sourced here). So "the old
system stays as a rollback" can't hold. That alone puts them outside
upgrade_'s model.

---

## The rules (policy level, not legal advice)

- **US: DMCA section 1201** bans getting around access controls. Every three
  years the Library of Congress grants exceptions. The current set (in force
  2024-10-28 to 2027-10-28) lets owners jailbreak phones, tablets, smart TVs,
  voice assistants and routers to run lawfully obtained apps [O1][O2][O4].
  **Whether replacing the whole OS is covered isn't clear from the wording**
  (a question for a lawyer).
- **Exceptions cover doing it, not handing out the tool.** The
  anti-trafficking rules, 1201(a)(2) and (b)(1), still bar distributing
  circumvention tools [O1].
- **EU:**
  - The Software Directive allows decompiling for interoperability [O24].
  - The Commission dropped the Radio Equipment Directive "software lock"
    plan in January 2026 [O25].
  - The Repair Directive (applies from 2026-07-31) bars software tricks
    that block repair [O28].
  - The Cyber Resilience Act's main duties start in December 2027 [O30].
  - None of these requires bootloader unlocking.
- **The market runs the other way:** Samsung removed unlocking (One UI 8),
  Xiaomi tightened it (2025), and Google is adding developer verification for
  sideloaded apps [O9][O15][O34].
- **For this project:** a tool that only uses doors the maker opens stays
  outside the circumvention question entirely. That's the rule this page
  applies, and it keeps the source readable and publishable.

---

## What this changes in upgrade_ (proposals, not decided)

1. **Long-term plan:** list "future sources" in order: Intel Macs without T2,
   x86 handhelds, then Snapdragon X; TVs recorded as blocked today, with the
   HDMI-laptop path as the TV story.
2. **A current-product question the research turned up:** Secured-core PCs
   (including Copilot+ PCs) reportedly ship with the Microsoft third-party
   UEFI CA **off**, which would stop shim, and so the handoff, on x86
   Windows PCs too [C33]. Lenovo's document says so; Microsoft's own
   documentation wasn't checked. If true, the scanner should detect it and
   refuse or explain before the handoff. Tracked as RISKS R29.
3. **Gaps to close** before any row moves: `bless --nextonly` with SIP on;
   Intel Mac Secure Boot; per-device unlock procedures (Pixel, Fairphone,
   OnePlus); whether the 1201 exemptions cover replacing the OS; official
   status of current consoles.

---

## Sources

### TVs and boxes

- [T1] https://www.flatpanelshd.com/news.php?subaction=showfull&id=1747898366
- [T2] https://www.techradar.com/televisions/streaming-devices/reminder-netflix-stops-working-today-on-some-older-amazon-fire-tv-devices-heres-the-list-of-models
- [T3] https://www.thestreet.com/entertainment/netflix-quitely-drops-support-for-millions-of-ps3s-and-legacy-smart-tvs
- [T4] https://www.techradar.com/streaming/netflix-is-leaving-your-older-apple-tv
- [T5] https://www.consumerreports.org/electronics-computers/streaming-media/netflix-will-stop-working-on-some-older-samsung-vizio-smart-tvs-roku-streaming-devices-a3512344253
- [T6] https://wiki.coreelec.org/coreelec:eemc
- [T7] https://developer.nvidia.com/how-flash-recovery-image
- [T8] https://wiki.lineageos.org/devices/foster/
- [T9] https://forum.webostv.developer.lge.com/t/developer-mode-keeps-login-me-out-before-999h-run-out/6428
- [T10] https://developer.samsung.com/smarttv/develop/getting-started/using-sdk/tv-device.html
- [T11] https://developer.samsung.com/smarttv/develop/faq/application-testing.html
- [T12] https://www.theregister.com/2025/03/10/google_chromecast_outage/
- [T13] https://www.da.vidbuchanan.co.uk/blog/netflix-on-asahi.html
- [T14] https://www.cnx-software.com/2023/03/07/libreelec-11-released-kodi-20-brings-back-amlogic-platforms/
- [T15] https://www.cnx-software.com/2024/05/05/libreelec-12-released-with-kodi-21-64-bit-arm-support-for-raspberry-pi-4-5-and-platforms/
- [T16] https://www.flatpanelshd.com/news.php?subaction=showfull&id=1724665225
- [T17] https://www.lg.com/us/press-release/more-lg-smart-tv-owners-set-to-enjoy-the-latest-webos-upgrade-making-their-tvs-feel-brand-new
- [T18] https://www.aftvnews.com/amazon-confirms-all-future-fire-tv-sticks-will-run-vega-os-no-more-android-or-sideloading-on-new-models/
- [T19] https://developer.roku.com/dev/docs/developer-mode
- [T20] https://liliputing.com/chromecast-with-google-tv-hd-has-an-unlockable-bootloader-unlike-the-4k-model/
- [T21] https://forum.armbian.com/topic/24296-community-support-for-amlogic-tv-boxes/
- [T23] https://techjunctions.com/how-to-allow-unknown-sources-on-hisense-tv/
- [T26] https://www.androidcentral.com/streaming-tv/nvidia-shield-tv/nvidia-shield-tv-update-november-2025
- [T27] https://forum.libreelec.tv/thread/13198-netflix/ (forum claim, not primary)
- [T33] https://linux-sunxi.org/BROM
- [T34] https://opensource.rock-chips.com/wiki_Boot_option (did not load; search excerpt only)
- [T35] https://discourse.coreelec.org/t/supported-wifi-chipset-list-for-coreelec-19-20-and-21/50759
- [T36] https://source.android.com/docs/core/architecture/bootloader/locking_unlocking
- [T38] https://www.webosose.org/docs/guides/setup/system-requirements/
- [T39] https://en.androidayuda.com/Widevine-error-on-Android-TV:-solutions-and-tricks-to-recover-L1-level/ (secondary)
- [T40] https://wiki.libreelec.tv/hardware/allwinner

### Computers

- [C1] https://asahilinux.org/about/
- [C2] https://asahilinux.org/2026/09/m2-episode-1/
- [C3] https://asahilinux.org/docs/platform/feature-support/m1/
- [C4] https://asahilinux.org/docs/platform/feature-support/m2/
- [C5] https://fedoramagazine.org/fedora-asahi-remix-43-is-now-available/
- [C6] https://appleinsider.com/articles/26/08/29/asahi-linux-nears-m3-support-release-m4-and-m5-are-on-the-way
- [C7] https://asahilinux.org/docs/sw/partitioning-cheatsheet/
- [C8] https://support.apple.com/guide/security/startup-disk-security-policy-control-sec7d92dc49f/web
- [C9] https://support.apple.com/guide/deployment/startup-security-dep5810e849c/web
- [C10] https://www.engadget.com/computing/macos-tahoe-is-the-end-of-the-line-for-intel-macs-113036626.html
- [C11] https://the-gadgeteer.com/2026/06/22/apple-made-intel-mac-obsolete-macos-27s-cutoff/
- [C13] https://wiki.t2linux.org/state/
- [C14] https://wiki.t2linux.org/guides/preinstall/
- [C15] https://support.apple.com/en-us/102522
- [C16] https://wiki.t2linux.org/distributions/fedora/installation/
- [C17] https://keith.github.io/xcode-man-pages/bless.8.html
- [C18] https://github.com/coolcoder613eb/OneFileLinux (secondary)
- [C19] https://github.com/patjak/facetimehd
- [C20] https://github.com/rpmfusion/broadcom-wl/blob/master/fedora.readme
- [C21] https://www.chromium.org/chromium-os/developer-library/guides/device/developer-mode/
- [C23] https://docs.chrultrabook.com/docs/installing/distros.html
- [C24] https://support.google.com/chrome/a/answer/6220366?hl=en
- [C25] https://pirg.org/edfund/resources/chromebook-churn-report-highlights-problems-of-short-lived-laptops-in-schools/
- [C26] https://support.google.com/chromebook/answer/1080595?hl=en
- [C27] https://github.com/hexdump0815/linux-mainline-on-arm-chromebooks
- [C28] https://pypi.org/project/depthcharge-tools
- [C29] https://fedoraproject.org/wiki/Snapdragon_WoA_Laptop_Install
- [C30] https://www.phoronix.com/news/Fedora-44-Approves-DTB-WOA
- [C31] https://www.linaro.org/blog/linux-on-snapdragon-x-elite/
- [C32] https://www.phoronix.com/review/ubuntu-2604-snapdragon-x-elite (search snippet only)
- [C33] https://download.lenovo.com/pccbbs/mobiles_pdf/Enable_Secure_Boot_for_Linux_Secured-core_PCs.pdf
- [C34] https://learn.microsoft.com/en-us/surface/manage-surface-uefi-settings
- [C35] https://github.com/linux-surface/linux-surface/wiki/Supported-Devices-and-Features
- [C36] https://github.com/linux-surface/linux-surface/wiki/Installation-and-Setup
- [C37] https://support.microsoft.com/en-us/surface/drivers-firmware/which-surface-devices-can-be-upgraded-to-windows-11
- [C38] https://learn.microsoft.com/en-us/windows-hardware/design/minimum/supported/windows-11-supported-qualcomm-processors
- [C39] https://fedoraproject.org/wiki/Changes/Stop_Building_i686_Kernels
- [C42] https://www.phoronix.com/news/MS-Surface-Pro-11-Linux-Patches (search snippet only)

### Other devices and the rules

- [O1] https://www.law.cornell.edu/uscode/text/17/1201
- [O2] https://www.federalregister.gov/documents/2024/10/28/2024-24563/exemption-to-prohibition-on-circumvention-of-copyright-protection-systems-for-access-control
- [O4] https://www.law.cornell.edu/cfr/text/37/201.40
- [O5] https://wiki.lineageos.org/devices/tangorpro/
- [O8] https://9to5google.com/2026/01/29/google-gives-pixel-tablet-another-two-years-of-android-os-updates/
- [O9] https://www.androidauthority.com/samsung-bootloader-unlocking-disabled-one-ui-8-3581366/
- [O10] https://en.wikipedia.org/wiki/Fire_OS
- [O11] https://www.macrumors.com/2026/06/08/ipados-27-drops-support-for-a-wave-of-ipads/
- [O12] https://postmarketos.org/blog/2025/12/23/v25.12-release/
- [O13] https://wiki.debian.org/Mobian/Devices
- [O14] https://devices.ubuntu-touch.io/
- [O15] https://c.mi.com/global/post/673501
- [O16] https://calyxos.org/install/verizon/
- [O17] https://en.wikipedia.org/wiki/Steam_Deck
- [O18] https://www.notebookcheck.net/Steam-Deck-receives-Windows-10-capabilities-with-official-drivers-as-Valve-confirms-Windows-11-and-Dual-Boot-support.607764.0.html
- [O19] https://techcrunch.com/2025/01/08/steamos-expands-to-other-gaming-handhelds-with-the-lenovo-legion-go-s/
- [O20] https://en.wikipedia.org/wiki/OtherOS
- [O21] https://fedoraproject.org/wiki/Changes/RaspberryPi4
- [O22] https://nullr0ute.com/2026/03/fedora-44-on-the-raspberry-pi-5/
- [O24] https://eur-lex.europa.eu/eli/dir/2009/24/oj/eng
- [O25] https://fsfe.org/news/2026/news-20260430-01.en.html
- [O28] https://eur-lex.europa.eu/eli/dir/2024/1799/oj/eng
- [O30] https://digital-strategy.ec.europa.eu/en/policies/cyber-resilience-act
- [O34] https://9to5google.com/2025/08/25/android-apps-developer-verification/
