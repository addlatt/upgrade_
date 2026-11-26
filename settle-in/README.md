# settle-in

Runs once, on the first boot into Linux.

It looks like a welcome screen, but it is the Linux half of the conversion:
it sets up what Windows knew, brings the files across, and holds the ways
back.

What it does:

- **The clock and Wi-Fi**, once, at the first startup, before the network
  (`settle-in first-start`, run by `linux/upgrade_-settle-in.service`). If
  Windows kept the hardware clock in local time, it turns it into UTC from
  evidence, or leaves it alone and says why. It writes one NetworkManager
  connection file per network Linux can join, readable by root only, lists
  the ones it cannot (enterprise, WEP) with the reason, then deletes its
  copy of the passwords.
- **Bring your files home** (keep-Windows only). It opens the kept Windows
  partition read-only, unlocking BitLocker with the saved key, and copies
  the folders the person chooses, checking each copy. It brings the
  browsers' bookmarks and a list of the programs with their Linux versions.
  It installs nothing. Files that live only in OneDrive stay there.
- **Windows first again** (`settle-in rollback`), on a computer converted
  with Windows kept. It deletes nothing.
- **Go back to Windows** after an erase: the walk-away way back, which
  downloads Windows from Microsoft, writes the stick and reads it back.
- **Reclaim** (keep-Windows only): deletes the kept Windows and moves Linux
  into the whole disk, after a typed sentence. This is that path's commit
  line. It is offered, never pressed.
- **Remove an old boot entry** that points at a Windows no longer there.

The erase path carries no files, so there is nothing to bring across there.

What it won't do: teach you Linux, install apps, run a tour or check in
later. A check that the hardware works (the speakers, not just the
headphones) is designed and not built.

**Runs on any Linux** (decided 2026-09-27). It is one self-contained
program, written in Rust, the same file on every distribution. It reads
only the handoff folder the installer filled (`/var/lib/upgrade_/`), and
works through the kernel and file formats every desktop Linux shares. See
`docs/architecture.md`, "It runs on any Linux".

The parts: `src/` is the program, `window/` the window a person sees,
`gate/` the small Windows program the walk-away way back leaves in the
fresh Windows, `linux/` the service, the desktop entries and the polkit
policy (the rule that asks for the password before a step that needs root).

Third-party code, kept short on purpose: `serde_json` (reads the job),
`roxmltree` (reads Windows' Wi-Fi profiles), `tz-rs` (reads the system's
own time-zone files), `libc` (the kernel calls for the clocks), `sha2` (checks a Windows
installer against Microsoft's published SHA-256), `ntfs` and `lz4_flex`
(read files inside OneDrive folders, which the system's NTFS driver
cannot; RISKS R43), `regress` (pattern matching), and what
they pull in (`serde`, `itoa`, `memchr`, `zmij`, and build-time macro
packages). `Cargo.lock` pins every version.

Build and test: `cargo test`, then
`cargo build --release --target x86_64-unknown-linux-musl` (a static
file; `make-kit.sh` does both and puts it on the stick).

Where each part stands: `docs/STATUS.md`. How it works:
`docs/architecture.md`, "Module 3: settle-in".
