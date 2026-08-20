# upgrade_ (the converter)

Does the conversion. Starts in Windows, finishes in Linux. One USB stick;
no external drive.

- `windows/` — prologue: validate, then either stage files to the stick's
  exFAT partition (clean-slate path) or shrink Windows aside with
  `Resize-Partition` (safety-copy path); boot handoff. Fully reversible.
- `linux/` — cutover: re-verify identity, verify checksums by reading them
  back, automated hardware checks, then install, inject artifacts, restore.
  On the clean-slate path this holds the commit line (the wipe), behind a
  two-minute human hardware gate. On the safety-copy path nothing destructive
  happens here at all — the commit line is the reclaim, in `settle-in`.

Nothing built yet. Deliberate: these are the only components that write, and
they are last in the build order so they can be reviewed hardest.

See `docs/architecture.md`.
