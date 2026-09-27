# upgrade_ (the converter)

This is the part that actually does the move. It starts on Windows and
finishes on Linux, using one USB stick and nothing else.

Think of it as a bridge with one gate in the middle. Everything before the
gate can be walked back. The gate is the commit line, and it only opens
once every check has said yes.

```text
  Windows side                     Linux side (installer on the stick)
  ----------------------------     -----------------------------------------
  re-check the job                 right machine? hardware OK? image intact?
  repair the disk if flagged                  |
  make room (shrink Windows)          THE COMMIT LINE
  pause BitLocker for one restart             |
  set up a one-time boot           install, check it boots, write outcome.json
```

## What's here

| File | What it does | State |
|---|---|---|
| `windows/Invoke-Prologue.ps1` | The Windows side: re-checks the job against the live machine, repairs a flagged disk, measures the room twice, shrinks Windows, pauses BitLocker and sets up the one-time boot into the stick. Carries on after restarts with nobody signed in. Stops and writes down why at every refusal. | `[###.]` rig, `[####]` real machine stops safely |
| `windows/Invoke-Rollback.ps1` | Undo. Puts Windows first in the boot order again, from the snapshot taken before the install. | `[###.]` rig |
| `windows/New-Kickstart.ps1` | Writes the installer's recipe (a Fedora "kickstart") from `job.json`. | `[####]` |
| `windows/Test-Handoff.ps1` | Test harness for the one-time boot (V0). | `[####]` |
| `windows/handoff-payload/` | What goes on the stick: launchers, boot files. See its README. | `[####]` |
| `linux/verify.sh` | Runs inside the installer before anything is decided (`%pre`): right machine, hardware, the desktop image read back byte for byte. For erase-and-install it holds the 2-minute countdown that you can cancel. | `[###.]` rig, `[####]` live boot on a real machine |
| `linux/outcome.sh` | Writes `outcome.json` at the end, whatever happened. | `[###.]` |

## Where the commit line sits

- **Keep Windows** (the default): nothing permanent happens here at all.
  Windows gets smaller and Linux moves in next door. The point of no return
  is the clean-up offer later, in `settle-in`.
- **Erase and install**: the commit line is the end of the 2-minute
  countdown in `verify.sh`. Until it hits zero, the disks are untouched.

These are the only parts that write to a disk, so they were built last and
get reviewed hardest. Full design in `docs/architecture.md`.
