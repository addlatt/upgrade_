upgrade_  -  V0 boot-handoff test stick
=========================================

This stick tests ONE thing: whether this computer's firmware boots a USB
payload exactly once when Windows asks it to, and falls back to Windows
on its own when it cannot. Nothing on the internal disk is changed. The
one boot-configuration change the test makes is undone on return
whatever happens.

THE ONE-CLICK WAY
-----------------
 1. Plug this stick into the computer under test.
 2. Open it and double-click  RUN-TEST.cmd .  Click "Yes" on the blue
    prompt. That is the only click.
 3. It scans the computer (a report and machine-capture.json land on
    this stick), arms the test with the signed payload, and restarts.
    Leave the stick in. If you can, watch the screen during the restart
    and remember whether you had to press anything.
 4. When Windows comes back, sign in as usual. A window appears with
    the result and one question (did it come back without a key press?).
    Answer it; a second window says the row was saved. Done.
 5. Unplug the stick and send it back. It holds v0-handoff.csv,
    machine-capture.json and upgrade-report-*.txt. Never edit the CSV.

If the test refuses to start it changes nothing - read its message. The
usual reason is BitLocker: if the report says it is on, save the recovery
key somewhere that is NOT this computer, then run RUN-TEST.cmd again.

If Windows does not come back by itself after the restart: at power-on
press the firmware boot-menu key (F12 on Acer - it must be enabled in
setup under Boot > F12 Boot Menu; F2 opens setup) and choose "Windows
Boot Manager" once. Then sign in; the result window still appears and
cleans up. Answer "No" to the key-press question.

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
