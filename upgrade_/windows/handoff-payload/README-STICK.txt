upgrade_  -  V0 boot-handoff test stick
=========================================

This stick tests ONE thing: whether this computer's firmware boots a USB
payload exactly once when Windows asks it to, and falls back to Windows
on its own when it cannot. Nothing on the internal disk is changed. The
one boot-configuration change the test makes is undone by step 4
whatever happens.

Which stick is this?  Look in EFI\BOOT:
  BOOTX64.EFI alone + startup.nsh at the root   = the SHELL stick (unsigned)
  BOOTX64.EFI + grubx64.efi + grub.cfg + grubenv = the SHIM stick (signed)

Steps - on the computer under test, with the stick plugged in
--------------------------------------------------------------
 1. Double-click RUN-SCANNER.cmd. Click "Yes" on the blue prompt.
    Read three lines of the report: "BitLocker", "Secure Boot",
    "Boot partition (ESP)". If BitLocker is ON: save the recovery key
    somewhere that is NOT this computer before going on (the report
    says how). The scanner also leaves machine-capture.json and the
    report on the stick - bring them back.

 2. Double-click ARM-HANDOFF.cmd. Click "Yes". Pick the row:

      Secure Boot   stick    pick   expected result
      -----------   -----    ----   ----------------------------------
      ON            shell     3     ignored   (firmware refuses; fail-safe)
      OFF           shell     1     fired-once
      OFF           shell     2     ignored   (NoFile fail-safe)
      ON            shim      1     fired-once
      any, BitLocker ON, either stick:  4  (NoSuspend - records what happens)

    The harness refuses to arm when it cannot tell whether BitLocker is
    on, or when BitLocker is on and row 4 was not chosen and suspension
    was not possible. A refusal changes nothing - read the message.

 3. Let it reboot. WATCH THE SCREEN and note: did any key have to be
    pressed? Any message (a "Secure Boot Violation" box, a recovery-key
    screen, a stuck vendor logo)? Did Windows come back on its own?
    If Windows does not come back: open the firmware boot menu (the
    vendor's key at power-on - F12 on Acer, needs "F12 Boot Menu" enabled
    in setup; F2 opens setup) and pick Windows Boot Manager once.

 4. Back in Windows: double-click CHECK-HANDOFF.cmd. Click "Yes". Answer
    the three questions honestly. It removes the test entry, restores
    the boot configuration, and writes ONE row to v0-handoff.csv on this
    stick. If the result was 'persisted' or 'reordered', run
    Test-Handoff.ps1 -Check -RestoreBcd from an elevated PowerShell in
    this folder as well.

 5. To change Secure Boot between rows: firmware setup at power-on.
    On many Acer machines the Secure Boot setting is greyed out until a
    Supervisor Password is set (Security tab); set one, change Secure
    Boot, and clear the password afterwards if you like.

Bring back: v0-handoff.csv, machine-capture.json, upgrade-report-*.txt.
Never edit the CSV by hand; it is transported verbatim into the repo.

Built by make-kit.sh - see KIT-MANIFEST.txt beside this file for the
commit, versions and checksums of exactly what is on this stick.
