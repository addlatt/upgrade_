upgrade_  -  the kit stick
==========================

Everything on the Windows side is one program, UPGRADE.exe. Double-click
UPGRADE.cmd to start it. It scans this computer, says what it found, asks
what you want, and does it: the conversion (Fedora beside your Windows,
or a clean slate that erases the computer if you choose that), how to
roll back (from Linux), or a cancel. It says
"you can still cancel" until the one moment that is true no longer.

If the scanner refused this computer because its drive is failing or its
Windows volume needs a repair, there is a separate launcher:
UPGRADE-ACCEPTING-DATA-LOSS.cmd. It asks for one sentence, typed exactly,
and lifts that refusal and nothing else. Every screen after it says DATA
LOSS ACCEPTED.

What the programs write lands in upgrade_\ on this stick: the job, the
installer's records, the prologue's records, the reports. Never edit
them; they are the evidence.

TOOLS FOR A TECHNICIAN (read-only unless it says otherwise)
-----------------------------------------------------------
DIAG-VOLUME.cmd, DIAG-SMART.cmd, DIAG-SECUREBOOT.cmd: what Windows says
about the C: volume, the drive's health, and Secure Boot's revocations.
They change nothing.

RUN-STORAGE-MODE.cmd: the scanner's "storage controller mode" line in
BOTH SATA modes (RST/RAID and AHCI), one click. It restarts into the
firmware's setup screen twice and boots Safe Mode once; the window
tells you what to change each time. Your files are not touched. Not for
a computer with BitLocker on (it refuses and changes nothing). This is
the last PowerShell on the stick; it leaves when it is ported.

WHAT IS ON THE STICK
--------------------
UPGRADE.exe and the four programs it runs (upgrade-scan.exe,
upgrade-harvest.exe, upgrade-job.exe, upgrade-prologue.exe); Fedora's
signed boot chain, installer and desktop images; settle-in, the program
that finishes the job on the Linux side; SHA256SUMS and KIT-MANIFEST.txt
naming the commit and every file's checksum. Re-check the stick with
sha256sum -c SHA256SUMS.
