# settle-in

Runs once, on the first boot into Linux.

It looks like a welcome screen, but it's really a safety check. It makes sure
the hardware actually works (the speakers, *not just the headphones*),
confirms your files arrived, and then hands the machine over.

What it will do:

- **Check the hardware** on the real machine, not just in the installer.
- **Bring your files home.** On the keep-Windows path it opens the old
  Windows partition (unlocking BitLocker with the saved key) and copies your
  folders across, checking each copy.
- **Set up what Windows knew:** the clock, your Wi-Fi networks and passwords,
  and your OneDrive sign-in (files that only live in OneDrive stay there).
- **Offer the clean-up once.** On the keep-Windows path this is the commit
  line: after everything checks out, it offers to delete the old Windows
  partition. It asks one time and never nags. Say no and a `reclaim` command
  is left behind for later.
- On the erase path, it tells you to keep the stick. It's your only backup
  until you decide you don't need one.

What it won't do: teach you Linux, install apps, run a tour or check in
later. The checking is the whole reason it exists.

**Runs on any Linux** (decided 2026-09-27). It is one self-contained
program, written in Rust, the same file on every distribution. It reads
only the handoff folder the installer filled (`/var/lib/upgrade_/`), and
works through the kernel and file formats every desktop Linux shares. See
`docs/architecture.md`, "It runs on any Linux".

What's built (2026-09-27), `settle-in first-start`, run once at startup
before the network (`linux/upgrade_-settle-in.service`):

- **The clock.** If Windows kept the hardware clock in local time, it
  turns it into UTC, once, from evidence. If it isn't sure, it leaves the
  clock alone and says why.
- **Wi-Fi.** One NetworkManager connection file per network Linux can
  join, readable by root only. Enterprise, WEP and the like are listed
  with the reason. Then it deletes its copy of the passwords.
- A report in `/var/lib/upgrade_/settle-in/report.json` (root only).

Third-party code, kept short on purpose: `serde_json` (reads the job),
`roxmltree` (reads Windows' Wi-Fi profiles), `tz-rs` (reads the system's
own time-zone files), `libc` (the kernel calls for the clocks), `sha2` (checks a Windows
installer against Microsoft's published SHA-256), and what
they pull in (`serde`, `itoa`, `memchr`, `zmij`, and build-time macro
packages). `Cargo.lock` pins every version.

Build and test: `cargo test`, then
`cargo build --release --target x86_64-unknown-linux-musl` (a static
file; `make-kit.sh` does both and puts it on the stick).

State: `[##..]` the first startup is built and tested on a fake machine
(22 tests, plus the installer's hand-over and settle-in chained end to
end); not yet run on the rig. Hardware checks, the file pull and the
window with the old-boot-entry button are not built. The one proven piece is reading a
BitLocker drive from Linux (`[###.]` on the rig, `v3-bitlk-read.csv`). See
`docs/architecture.md`.
