# upgrade_

**Will this computer run Linux?**

A read-only scanner you run on Windows. It tells you whether this specific
machine can move to Linux, what will break, which distribution to use, and what
to do first — before you touch a single partition.

It changes nothing, encrypts nothing, deletes nothing, and sends nothing
anywhere.

---

## Why

Windows 10 stopped getting security updates in October 2025. Windows 11's TPM
and CPU requirements stranded an enormous number of machines that are
mechanically and electrically fine. Those computers are not broken. They were
declared obsolete by someone else's product roadmap.

Most of them can run Linux for another five years. The obstacle is not
capability, it's uncertainty: no one can tell you whether *your* laptop will
work until you've already wiped it.

That is the question this answers.

## What it actually checks

The valuable output is not a yes/no. It's **the kernel version your hardware
needs**, and which popular distributions fail to meet it.

That distinction matters more than anything else here. A first-time user is
almost always pointed at Linux Mint or Ubuntu LTS. On a 2024-or-newer laptop
those ship a kernel too old for the Wi-Fi card, and the user boots into a
system with no wireless, concludes Linux is broken, and reinstalls Windows.
Nothing was broken. They picked a release from before their laptop existed.

| Check | Why it's there |
|---|---|
| **Intel RST / VMD** | The SSD is invisible to every Linux installer. Looks like a broken installer, is actually a BIOS setting. The single most common false "Linux won't install". |
| **Wi-Fi chipset** | Broadcom cards need a driver you can't download without a network connection you don't have yet. Recent MediaTek cards need kernel 6.7+. |
| **Graphics** | New AMD APUs need a matching recent kernel or you get a black screen. NVIDIA needs a distro that installs the proprietary driver for you. |
| **Smart audio amps** | Cirrus and TI amps mean headphones work and the internal speakers are silent. Extremely common on 2023+ laptops. |
| **BitLocker** | Resize an encrypted disk without the recovery key and the data is gone permanently. |
| **Fast Startup** | Windows hibernates instead of shutting down, leaving the partition unsafe to resize. Shutting down does not clear it. |
| **Free space & backup size** | Tells you how big an external drive you actually need. |
| **Installed software** | Adobe, Office, CAD, kernel anti-cheat games. The honest answer is sometimes "don't convert this machine." |

Unrecognised devices are listed at the end of the report so they can be
contributed back.

## Running it

Download `dist/upgrade-scan.ps1`, then in PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -File .\upgrade-scan.ps1
```

The report prints to the screen and saves to your Desktop.

Useful flags:

```powershell
-Json          # also write machine-readable JSON
-NoFile        # print only, save nothing
-OutDir <path> # save somewhere other than the Desktop
-SelfTest      # run the built-in logic tests
```

**Run it as Administrator if you can.** Without elevation, Windows refuses to
report BitLocker status, and the scanner cannot tell you whether your disk is
encrypted — the one unknown that can cost you everything. It still runs fine
unelevated; it just caps its verdict and says so.

## What it will not do

It will not convert anything. It will not touch your partitions. It has no
network code at all.

That is deliberate, and it is the project's first principle: **be honest about
machines we can't do safely, and refuse them.** A tool like this earns trust
once and spends it permanently the first time it bricks someone's laptop. Any
future component that does write to a disk has to clear a far higher bar than
this scanner does.

## Development

```
scan/upgrade-scan.ps1   the scanner
scan/data/devices.ps1   hardware knowledge base   <- most contributions go here
scan/data/distros.ps1   distribution kernel table <- goes stale, needs refreshing
build.sh                inlines data/ into dist/upgrade-scan.ps1
dist/upgrade-scan.ps1    the built single file people download
```

Run from source with `scan/upgrade-scan.ps1` (it loads `data/` from disk), or
build the standalone file:

```bash
./build.sh
```

Targets Windows PowerShell 5.1, which ships on every Windows 10 and 11 install.
No PowerShell 7 syntax — no ternaries, no null-coalescing. If it doesn't run on
a stock machine, it doesn't run where it matters.

Before opening a PR:

```powershell
.\scan\upgrade-scan.ps1 -SelfTest
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Adding a device is a one-line change to
a table. That's the point — the hardware database is the part that only gets
good with many people, and it's the part that makes every future report better.

## Roadmap

- [x] Preflight scanner (Windows, read-only)
- [ ] Hardware database seeded from [linux-hardware.org](https://linux-hardware.org) probe data rather than hand-curated
- [ ] Opt-in outcome reporting: what actually happened when you installed
- [ ] Live-USB version, for machines whose Windows install is dead or locked
- [ ] A printable handoff sheet — where your files went, what replaced what

Deliberately **not** on the roadmap yet: anything that writes to a disk.

## Licence

MIT — see [LICENSE](LICENSE).

A note on that choice: MIT was picked to make forking and vendoring
frictionless, which matters for a tool meant to be handed around at repair
events. If you'd rather this be copyleft, GPL-3.0 is a defensible fit for a
project whose trust model is "read the source" — it's a one-file change while
the contributor count is still small, and much harder later.
