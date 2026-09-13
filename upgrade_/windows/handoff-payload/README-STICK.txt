upgrade_  -  V0 boot-handoff test stick
=========================================

This stick tests ONE thing: whether this computer's firmware boots a USB
payload exactly once when Windows asks it to, and falls back to Windows
on its own when it cannot. Nothing on the internal disk is changed. The
one boot-configuration change the test makes is undone on return
whatever happens.

THE LIVE-BOOT TEST (V1) - the second thing this stick can do
------------------------------------------------------------
Double-click  RUN-VERIFY.cmd  instead of RUN-TEST.cmd. Same one click.
It scans, writes job.json for this computer, generates the kickstart,
arms the same one-time handoff and restarts. This time the stick boots
the Fedora installer: it checks that this is the right computer (disk
serial and size), checks the display and Wi-Fi hardware, reads the
desktop image on this stick back byte for byte, writes its report to
upgrade_\report\ on this stick and restarts back into Windows.
NOTHING IS INSTALLED. It takes a few minutes longer than the V0 test
(reading a 2.6 GB image off a slow stick is most of it). When Windows
comes back, the same result window appears; the row lands in
v0-handoff.csv and the report in upgrade_\report\verify.json.
If it refuses to start it changes nothing - read its message.

THE WALK-AWAY PROBE (RUN-PROBE.cmd) - read-only, one restart
------------------------------------------------------------
Double-click RUN-PROBE.cmd, click "Yes", press a key. The computer
restarts once. When Windows comes back, DO NOT SIGN IN for two minutes
- leave it at the sign-in screen with the stick in. Then sign in as
usual: a window shows the result. It tests that the conversion can
continue after a restart with nobody signed in (the row lands in
upgrade_\walkaway-probe.csv on this stick; never edit it). Nothing on
the disk is changed.

THE STORAGE-MODE TEST (RUN-STORAGE-MODE.cmd) - both SATA modes, one click
---------------------------------------------------------------------
Proves the scanner's "Storage controller mode" line on this computer
in BOTH SATA modes (Intel RST/RAID and AHCI). Double-click, click
"Yes", read the window, click OK. The computer scans, then restarts
straight into its setup screen: change SATA Mode to what the window
asked for, F10, Yes. It boots Safe Mode once (so Windows binds the
driver for the re-enumerated controller) and restarts by itself, scans
again with nobody signed in, and opens the setup screen a second time:
set SATA Mode back, F10, Yes. Safe Mode once more, a last scan, and
everything it armed is removed. Sign in: a window shows the result.
Your files are not touched - the mode changes how the disk is
addressed, not what is on it. If the setup screen has no SATA Mode
option (Acer: try Ctrl+S on the Main tab first), exit without saving;
the test sees the mode did not change and stops on its own. The record
is in upgrade_\storage-mode\ on this stick; never edit it. Not for a
computer with BitLocker on (it refuses and changes nothing).

THE CONVERSION (RUN-CONVERT.cmd) - this one CHANGES the internal disk
----------------------------------------------------------------------
Only on a computer you mean to convert. Double-click RUN-CONVERT.cmd,
click "Yes" on the blue prompt, and type CONVERT when asked. It scans,
writes the job, generates the kickstart, then runs the prologue: it
re-checks the job against this computer, runs Windows' own disk check
if Windows has flagged C: (that is a restart of its own - leave the
stick in and walk away; it continues by itself, before anyone signs
in, and if it stops a window says so at your next sign-in),
re-measures the room, shrinks the Windows partition, suspends
BitLocker for one restart, and restarts into the Linux installer on
this stick. Linux is installed BESIDE Windows; Windows stays in the
boot menu until you reclaim it later, in Linux. Every refusal happens
before anything is changed, and a refusal writes upgrade_\outcome.json
on this stick saying where and why. When you next boot Windows from
the menu, a small window confirms the one-time boot entry was removed.

ACCEPTING DATA LOSS (RUN-CONVERT-ACCEPTING-DATA-LOSS.cmd)
----------------------------------------------------------
Not the normal way. If the scanner refused this computer because its
drive is failing (bad blocks, SMART errors) or its Windows volume needs
a full repair, this launcher goes ahead anyway - after you have copied
your files OFF the computer and typed, exactly:
    I confirm that I understand the risks and could lose data
It lifts only those two refusals. A wrong machine, a wrong stick,
legacy BIOS, an unknown BitLocker state, a bad image or a failed
boot-file snapshot still stop it. Everything it writes afterwards says
DATA LOSS ACCEPTED. DIAG-VOLUME.cmd and DIAG-SMART.cmd (read-only) show
you what the drive is reporting before you decide.

ROLLING BACK (ROLLBACK.cmd) - Windows first again
--------------------------------------------------
On a computer converted with Windows kept: boot Windows from the boot
menu, double-click ROLLBACK.cmd, type ROLLBACK. It puts Windows Boot
Manager first in the firmware's order and puts Windows' own fallback
boot file back from the copy this stick took before the conversion.
It deletes nothing: Linux stays on the disk and in the firmware's boot
menu. Restart, and Windows comes up by itself.

THE ONE-CLICK WAY (V0)
----------------------
 1. Plug this stick into the computer under test.
 2. Open it and double-click  RUN-TEST.cmd .  Click "Yes" on the blue
    prompt. That is the only click.
 3. It scans the computer (a report and machine-capture.json land on
    this stick), arms the test with the signed payload, and restarts.
    Leave the stick in. If you can, watch the screen during the restart
    and remember whether you had to press anything.
 4. When Windows comes back, sign in as usual (this test harness still
    needs a sign-in; the conversion and the probe do not). A window
    appears with the result and one question (did it come back without
    a key press?).
    Answer it; a second window says the row was saved. Done.
 5. Unplug the stick and send it back. It holds v0-handoff.csv,
    machine-capture.json and upgrade-report-*.txt. Never edit the CSV.

If the test refuses to start it changes nothing - read its message. The
usual reason is BitLocker: if the report says it is on, save the recovery
key somewhere that is NOT this computer, then run RUN-TEST.cmd again.

If Windows does not come back by itself after the restart: at power-on
press the firmware boot-menu key (F12 on Acer - it must be enabled in
setup under Boot > F12 Boot Menu; F2 opens setup) and choose "Windows
Boot Manager" once. Then sign in (the harness needs it; the conversion
would not); the result window still appears and cleans up. Answer "No"
to the key-press question.

THE MATRIX WAY (for people running several rows by hand)
--------------------------------------------------------
 RUN-SCANNER.cmd   the scan only
 ARM-HANDOFF.cmd   pick a row: signed baseline / unsigned baseline
                   (Secure Boot off) / unsigned with Secure Boot on
                   (expect: ignored) / NoFile (expect: ignored) /
                   NoSuspend. Then choose whether the return check
                   runs itself or you run CHECK-HANDOFF.cmd by hand.
 CHECK-HANDOFF.cmd the manual return check: classifies, cleans up,
                   asks three questions, appends the row.

Changing Secure Boot: firmware setup at power-on. On many Acer machines
the setting is greyed out until a Supervisor Password is set (Security
tab); set one, change Secure Boot, clear the password afterwards if
you like.

WHAT IS ON THIS STICK
---------------------
 EFI\BOOT\BOOTX64.EFI + grubx64.efi + grub.cfg + grubenv
                   Fedora's signed shim and GRUB: records the firing in
                   grubenv, then reboots. The payload RUN-TEST.cmd uses.
 EFI\SHELL\SHELLX64.EFI + startup.nsh (root)
                   the unsigned UEFI Shell: writes fired.txt, reboots.
                   Used by the unsigned matrix rows only.
 Test-Handoff.ps1  the harness (arm / check / self-test)
 upgrade-scan.ps1  the scanner
 KIT-MANIFEST.txt, SHA256SUMS
                   exactly what is on this stick: commit, versions,
                   checksums. Re-check with: sha256sum -c SHA256SUMS

Built by make-kit.sh in the upgrade_ repository.
