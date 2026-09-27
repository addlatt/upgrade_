# Contributing

The most valuable thing you can give this project is one line in a table.

## Adding a device

Run the scanner. At the bottom of your report is a section listing the
devices it didn't recognise:

```
-- HELP THE PROJECT ---------------------------------------------------------

    wifi 14c3:7925 (MediaTek Wi-Fi 7 MT7925 Wireless LAN Card)
```

Add it to `data/devices.ps1`:

```powershell
'14c3:7925' = @{ Name='MediaTek MT7925'; Driver='mt7925e'; MinKernel='6.7'; Status='warn'
                 Note='Wi-Fi 7 card, supported from kernel 6.7. On older kernels there is no driver at all - you will boot with no wireless.' }
```

In the PR, say **how you know**. Any one of these is enough:

- You installed Linux on this hardware and it worked, or it didn't. Say
  which distribution and which kernel version.
- A link to the kernel commit or release notes that added support.
- A link to the driver's documented list of supported hardware.

"It should work" is not a source. A wrong `ok` in this table sends someone
into an install that fails. That is exactly what this project exists to
prevent.

## Adding a machine capture (the second most valuable contribution)

```powershell
.\upgrade-scan.ps1 -DumpMachine machine-capture.json
```

This writes a **hardware-only** snapshot of what the scanner's checks read on
your machine: the PCI / ACPI / HDAUDIO device list and some basic system
facts. No account names, no network names, no files, no device paths that
carry serial numbers. Read the JSON yourself before you send it. It should
hold nothing you couldn't read off the outside of the machine.

Fill in the `Expected` block (which status each check should give your
machine; the maintainers will help), and PR it into
`evaluate/windows/corpus/`. Every capture there is replayed through the real
detection code on every self-test run. So your machine keeps being tested by
every future change, even though we never met it. Right now, one capture
from a machine with Intel RST/VMD turned on is worth more to this project
than any code.

## The bar for each status

| Status | Means |
|---|---|
| `ok` | Works on first boot, with nothing for the user to do. |
| `warn` | Works, but the user must do or know something: a recent kernel, firmware, a setup step. **The `Note` must say exactly what.** |
| `fail` | Does not work, or needs new hardware, or needs something the user cannot get from the machine they're converting. |

When you're unsure between two, pick the more cautious one. An unneeded
`warn` costs someone five minutes of reading. An optimistic `ok` costs them
their afternoon and their confidence.

## Writing notes

Notes are shown to the user word for word, and the user is often not
technical.

- Say what they will *see*, not what is technically true. "Internal
  speakers stay silent while headphones work" beats "the codec requires an
  out-of-tree ASoC driver."
- Say what to do about it, or say plainly that nothing can be done.
- No hedging that carries no information. "May or may not work" tells them
  nothing they didn't already know.

## The distribution table

`data/distros.ps1` goes stale faster than anything else here, and a stale
kernel number gives advice that is confident and wrong. Entries marked
`Approx=$true` were not checked against a primary source (the release notes
themselves) and need confirming.

If you refresh it, set `$script:UpgDistroTableVerified` to the date you
checked, and cite the release notes in your PR. The scanner warns users when
the table is more than 120 days old.

## Code

Windows PowerShell **5.1**. That's what ships on a stock Windows 10 and 11
machine, and this tool runs on machines nobody has set up for development.
No PowerShell 7 syntax: no `?:` ternaries, no `??`, no `-Parallel`.

Run the tests before opening a PR:

```powershell
.\evaluate\windows\upgrade-scan.ps1 -SelfTest
.\evaluate\windows\Harvest-UpgradeState.ps1 -SelfTest
```

If you change verdict or detection logic, add a case to `Invoke-UpgSelfTest`
that covers it. If you change the harvester's parsing or arithmetic, add one
to `Invoke-HarvestSelfTest`. The checks that read the live OS are split in
two: a half that collects and a half that judges. Keep it that way. Judging
functions take their inputs as parameters, so the self-test can feed them
machines that don't exist here (see CLAUDE.md rule #5).

## The one rule that isn't negotiable

**Refuse by default.**

There will be pressure to soften warnings, loosen checks and let borderline
hardware through, including from contributors whose own machine works fine.
Resist it. This project's only asset is that its report can be trusted. A
scanner that says "probably fine" and is wrong is worse than no scanner,
because the person acted on it and lost their data.

If a check is wrong, fix the check. Don't remove it.

## Scope

The scanner reads. It never writes. The converter does write (it shrinks
Windows, installs Fedora, and on one launcher erases the disks), and those
parts are built last, behind real-hardware evidence, and reviewed hardest.
Contributions that touch partitions, change firmware settings or install
anything are out of scope until there's a much larger corpus of verified
hardware outcomes to justify them. The hardware tables in `data/` and
scanner reports are where help counts most today.
