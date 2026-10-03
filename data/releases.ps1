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
    # BEGIN fedora-44 (measured 2026-10-03 by data/tools/measure-release.py from the files themselves; do not edit by hand)
    @{ Id = 'fedora-44'; Name = 'Fedora 44'; Family = 'fedora'; Installer = 'kickstart'; Measured = '2026-10-03'; Note = ''
       Netinst = @{ Url = 'https://dl.fedoraproject.org/pub/fedora/linux/releases/44/Server/x86_64/iso/Fedora-Server-netinst-x86_64-44-1.7.iso'; Sha256 = 'ae20c06bea746913cadea7d80463e13f4bf55bee4df2918111c921c674b70283' }
       Desktops = @(
        @{ Id = 'kde'; Label = 'KDE Plasma'; Url = 'https://dl.fedoraproject.org/pub/fedora/linux/releases/44/KDE/x86_64/iso/Fedora-KDE-Desktop-Live-44-1.7.x86_64.iso'; IsoSha256 = 'c8295961d4c41adbf785a31a17c21a971d3b7415fda72dcad0c11c49577bf03a'; ImagePath = 'LiveOS/squashfs.img'; ImageSha256 = '2b6c1d5727467be858481f16a9131f16fbcaf0cf77bf57139987446825a71330' }
        @{ Id = 'gnome'; Label = 'GNOME'; Url = 'https://dl.fedoraproject.org/pub/fedora/linux/releases/44/Workstation/x86_64/iso/Fedora-Workstation-Live-44-1.7.x86_64.iso'; IsoSha256 = '1620295f6a00c27c3208f0c00b8ece4eab1ec69b9002152d97488bf26a426ddf'; ImagePath = 'LiveOS/squashfs.img'; ImageSha256 = 'e4303ff920701e7bb293b18b034de25fe6d046c6f011e56f0820d3bce692879c' }
       )
       Boot = @(
        @{ Role = 'stick'; File = 'BOOTX64.EFI'; Sha256 = '571ea56b855dcf73bec6acb63c5ded44c2a191138bca0d8cfa5aa93f60f46fff'
           Sbat = 'sbat,1,SBAT Version,sbat,1,https://github.com/rhboot/shim/blob/main/SBAT.md
shim,4,UEFI shim,shim,1,https://github.com/rhboot/shim
shim.rh,3,The Fedora Project,shim,16.1,https://src.fedoraproject.org/rpms/shim-unsigned-x64
shim.redhat,3,The Fedora Project,shim,16.1,https://src.fedoraproject.org/rpms/shim-unsigned-x64
shim.fedora,3,The Fedora Project,shim,16.1-1,https://src.fedoraproject.org/rpms/shim-unsigned-x64'
           SbatLevel = @('sbat,1,2024040900
shim,4
grub,4
grub.peimage,2
', 'sbat,1,2025051000
shim,4
grub,5
grub.proxmox,2
'); SignedBy = @('Microsoft Corporation UEFI CA 2011', 'Microsoft Corporation Third Party Marketplace Root') }
        @{ Role = 'stick'; File = 'grubx64.efi'; Sha256 = '51d290821cd0df32de5c2462cd63252276b63ef8dfc53fda79855fe51c9d1e45'
           Sbat = 'sbat,1,SBAT Version,sbat,1,https://github.com/rhboot/shim/blob/main/SBAT.md
grub,5,Free Software Foundation,grub,2.12,https//www.gnu.org/software/grub/
grub.rh,2,Red Hat,grub2,2.12-56.fc44,mailto:secalert@redhat.com'
           SbatLevel = @(); SignedBy = @('fedoraca') }
        @{ Role = 'installed kde'; File = 'shimx64.efi'; Sha256 = '571ea56b855dcf73bec6acb63c5ded44c2a191138bca0d8cfa5aa93f60f46fff'
           Sbat = 'sbat,1,SBAT Version,sbat,1,https://github.com/rhboot/shim/blob/main/SBAT.md
shim,4,UEFI shim,shim,1,https://github.com/rhboot/shim
shim.rh,3,The Fedora Project,shim,16.1,https://src.fedoraproject.org/rpms/shim-unsigned-x64
shim.redhat,3,The Fedora Project,shim,16.1,https://src.fedoraproject.org/rpms/shim-unsigned-x64
shim.fedora,3,The Fedora Project,shim,16.1-1,https://src.fedoraproject.org/rpms/shim-unsigned-x64'
           SbatLevel = @('sbat,1,2024040900
shim,4
grub,4
grub.peimage,2
', 'sbat,1,2025051000
shim,4
grub,5
grub.proxmox,2
'); SignedBy = @('Microsoft Corporation UEFI CA 2011', 'Microsoft Corporation Third Party Marketplace Root') }
        @{ Role = 'installed kde'; File = 'grubx64.efi'; Sha256 = 'b8e335da604169cb5bf61cfe474a9ef9c4f88f3c3140f77d94cca50021b3882b'
           Sbat = 'sbat,1,SBAT Version,sbat,1,https://github.com/rhboot/shim/blob/main/SBAT.md
grub,5,Free Software Foundation,grub,2.12,https//www.gnu.org/software/grub/
grub.rh,2,Red Hat,grub2,2.12-56.fc44,mailto:secalert@redhat.com'
           SbatLevel = @(); SignedBy = @('fedoraca') }
        @{ Role = 'installed gnome'; File = 'shimx64.efi'; Sha256 = '571ea56b855dcf73bec6acb63c5ded44c2a191138bca0d8cfa5aa93f60f46fff'
           Sbat = 'sbat,1,SBAT Version,sbat,1,https://github.com/rhboot/shim/blob/main/SBAT.md
shim,4,UEFI shim,shim,1,https://github.com/rhboot/shim
shim.rh,3,The Fedora Project,shim,16.1,https://src.fedoraproject.org/rpms/shim-unsigned-x64
shim.redhat,3,The Fedora Project,shim,16.1,https://src.fedoraproject.org/rpms/shim-unsigned-x64
shim.fedora,3,The Fedora Project,shim,16.1-1,https://src.fedoraproject.org/rpms/shim-unsigned-x64'
           SbatLevel = @('sbat,1,2024040900
shim,4
grub,4
grub.peimage,2
', 'sbat,1,2025051000
shim,4
grub,5
grub.proxmox,2
'); SignedBy = @('Microsoft Corporation UEFI CA 2011', 'Microsoft Corporation Third Party Marketplace Root') }
        @{ Role = 'installed gnome'; File = 'grubx64.efi'; Sha256 = 'b8e335da604169cb5bf61cfe474a9ef9c4f88f3c3140f77d94cca50021b3882b'
           Sbat = 'sbat,1,SBAT Version,sbat,1,https://github.com/rhboot/shim/blob/main/SBAT.md
grub,5,Free Software Foundation,grub,2.12,https//www.gnu.org/software/grub/
grub.rh,2,Red Hat,grub2,2.12-56.fc44,mailto:secalert@redhat.com'
           SbatLevel = @(); SignedBy = @('fedoraca') }
       ) }
    # END fedora-44
    # (releases end)
    )
}
