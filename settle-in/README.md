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

State: `[#...]` planned, not built. The one proven piece is reading a
BitLocker drive from Linux (`[###.]` on the rig, `v3-bitlk-read.csv`). See
`docs/architecture.md`.
