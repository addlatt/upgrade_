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

| Where | What it does |
|---|---|
| `prologue/` | `upgrade-prologue`, the Windows side: re-checks the job against the live machine, confirms a flagged disk, measures the room twice, shrinks Windows, pauses BitLocker and sets up the one-time boot into the stick. Carries on after restarts with nobody signed in. Stops and writes down why at every refusal. Also shim in Windows' slot with its guard, and the storage-mode harness. The undo is not here: it runs from Linux (`settle-in rollback`, decided 2026-10-12). |
| `kickstart/` | `upgrade-kickstart`: writes Fedora's installer recipe (a "kickstart") from `job.json`. |
| `autoinstall/` | `upgrade-autoinstall`: the same for Ubuntu (its installer's answer file, an "autoinstall"). The stick carries a file that installs nothing by itself; the real one is written only after every check passed. |
| `windows/window/` | `UPGRADE.exe`, the window the person sees. It runs the programs and shows their words. |
| `windows/handoff-payload/` | What goes on the stick: the starter (`UPGRADE.cmd`) and the boot files. See its README. |
| `linux/installer/` | `upgrade-installer`, one program inside the installer on the stick. `verify` runs before anything is decided (`%pre`): right machine, hardware, the desktop image read back byte for byte. Every refusal comes here, before anything is erased. `outcome` writes `outcome.json` at the end, whatever happened. |

Where each stands: `docs/STATUS.md`.

## Where the commit line sits

The person is asked which path, as equals (decided 2026-11-23): neither is
the default.

- **Keep Windows**: nothing permanent happens here at all. Windows gets
  smaller and Linux moves in next door. The point of no return is reclaim,
  later, in `settle-in`.
- **Erase and install**: the commit line is the person's last click in the
  window (decided 2026-11-02). There is no countdown and no cancel after
  it, and the window says so at that click. `upgrade-installer verify`
  still runs every check first, and refuses before anything is erased.

These are the only parts that write to a disk, so they were built last and
get reviewed hardest. Full design in `docs/architecture.md`.
