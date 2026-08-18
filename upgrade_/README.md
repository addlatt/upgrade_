# upgrade_ (the module)

Does the conversion. Starts in Windows, finishes in Linux, and contains the
**commit line** — the first partition write, the only irreversible moment in
the system.

- `windows/` — prologue: validate, image to external, stage, boot handoff.
  Fully reversible; nothing is destroyed here.
- `linux/` — cutover: re-verify identity, check staged checksums, then partition,
  install, inject artifacts, restore. Also hosts rollback mode.

Nothing built yet. Deliberate: these are the only components that write, and
they are last in the build order so they can be reviewed hardest.

See `docs/architecture.md`.
