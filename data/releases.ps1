# =============================================================================
#  upgrade_ / Linux releases the kit can carry, and what Secure Boot needs
# =============================================================================
#  Decided (2026-10-03, the owner; RISKS R34): the release is chosen from what
#  the machine accepts. evaluate reads the machine (Secure Boot on or off, the
#  firmware's SBAT revocation level, which Microsoft certificate authorities
#  its db trusts), matches it against this table, and the kit is built for a
#  release the machine can start, both from the stick and once installed.
#
#  Every fact below the header of an entry is MEASURED from the release's own
#  files by data/tools/measure-release.py and pinned by sha256. Never type or
#  edit them by hand: re-run the tool. The kit builder refuses files whose
#  hashes differ from the entry.
#
#  Per entry:
#    Family/Installer  who installs it unattended. Only 'fedora'/'kickstart'
#                      has installer automation today. A release without it
#                      can be listed (and judged) but not installed.
#    Netinst           the installer ISO: its EFI/BOOT files boot the stick.
#    Desktops          live ISOs; ImageSha256 is the image the stick carries.
#    Boot              each boot file's facts. Role 'stick' = what the stick
#                      starts; 'installed <desktop>' = what the installed
#                      system starts. Sbat = the generations it declares;
#                      SbatLevel = a shim's built-in revocation levels;
#                      SignedBy = the authorities its signatures chain to.
#
#  Adding a release (a distro, or a new version): fetch its ISOs, run
#  measure-release.py, commit the entry with the URLs it read. A release
#  that cannot start with Secure Boot on (no Microsoft-signed shim) is still
#  worth an entry: the scanner then says so plainly.
# =============================================================================

function Get-UpgReleaseTable {
    @(
    # (releases end)
    )
}
