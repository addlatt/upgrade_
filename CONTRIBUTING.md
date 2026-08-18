# Contributing

The most valuable contribution to this project is a single line in a table.

## Adding a device

Run the scanner. At the bottom of your report there's a section listing devices
it didn't recognise:

```
-- HELP THE PROJECT ---------------------------------------------------------

    wifi 14c3:7925 (MediaTek Wi-Fi 7 MT7925 Wireless LAN Card)
```

Add it to `data/devices.ps1`:

```powershell
'14c3:7925' = @{ Name='MediaTek MT7925'; Driver='mt7925e'; MinKernel='6.7'; Status='warn'
                 Note='Wi-Fi 7 card, supported from kernel 6.7. On older kernels there is no driver at all - you will boot with no wireless.' }
```

Include in the PR **how you know**. Any one of these is enough:

- You installed Linux on this hardware and it worked, or didn't — say which
  distribution and kernel version
- A link to the kernel commit or release notes that added support
- A link to the driver's documented hardware support list

"It should work" is not a source. A wrong `ok` in this table sends someone
into an install that fails, which is the exact outcome the project exists to
prevent.

## The bar for each status

| Status | Means |
|---|---|
| `ok` | Works on first boot with no user intervention. |
| `warn` | Works, but the user must do or know something — a recent kernel, firmware, a config step. **The `Note` must say exactly what.** |
| `fail` | Does not work, or needs hardware replacement, or needs something the user cannot obtain from the machine they're converting. |

When you're unsure between two, pick the more cautious one. An unnecessary
`warn` costs someone five minutes of reading. An optimistic `ok` costs them
their afternoon and their confidence.

## Writing notes

Notes are shown to the user verbatim, and the user is often not technical.

- Say what they will *observe*, not what is technically true. "Internal
  speakers stay silent while headphones work" beats "the codec requires an
  out-of-tree ASoC driver."
- Say what to do about it, or say plainly that there's nothing to be done.
- No hedging that doesn't carry information. "May or may not work" tells them
  nothing they didn't already know.

## The distribution table

`data/distros.ps1` goes stale faster than anything else here, and a stale
kernel number produces confidently wrong advice. Entries marked `Approx=$true`
were not checked against a primary source and need confirming.

If you refresh it, update `$script:UpgDistroTableVerified` to the date you
checked, and cite the release notes in your PR. The scanner warns users when
the table is more than 120 days old.

## Code

Windows PowerShell **5.1**. That's what ships on a stock Windows 10 and 11
machine, and this tool runs on machines nobody has set up for development. No
PowerShell 7 syntax: no `?:` ternaries, no `??`, no `-Parallel`.

Run the tests before opening a PR:

```powershell
.\evaluate\windows\upgrade-scan.ps1 -SelfTest
```

If you change verdict logic, add a case to `Invoke-UpgSelfTest` covering it.

## The one rule that isn't negotiable

**Refuse by default.**

There will be pressure — including from contributors whose own machine works
fine — to soften warnings, loosen checks, and let marginal hardware through.
Resist it. This project's only asset is that its report is trustworthy. A
scanner that says "probably fine" and isn't, is worse than no scanner, because
the person acted on it and lost their data.

If a check is wrong, fix the check. Don't remove it.

## Scope

Right now this reads. It does not write. Proposals that touch partitions,
modify firmware settings, or install anything are out of scope until there's a
much larger corpus of verified hardware outcomes to justify them.
