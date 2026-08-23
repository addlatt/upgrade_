# upgrade_ / V0 handoff payload
#
# The UEFI Shell runs this file automatically at startup. Its whole job is to
# prove, in a machine-readable way, that the firmware actually ran our boot
# entry - then get out of the way and go back to Windows.
#
# Keep the marker name in sync with $FiredMarker in Test-Handoff.ps1.
#
# We must write the marker to THE STICK, not to whatever the shell happens to
# map as fs0:. On any machine that already has Windows installed there is also
# an EFI System Partition (a FAT volume), and the shell often maps that as
# fs0: - so a bare `fs0:` writes the marker to the ESP and the harness, which
# looks for it on the stick, misreads a real firing as a fail-safe. (Confirmed
# on QEMU+OVMF, 2026-08-23.) Instead, find the volume that actually holds this
# script: only the stick has startup.nsh at its root - the ESP does not.

echo -off
for %v in fs0 fs1 fs2 fs3 fs4 fs5 fs6 fs7
    if exist %v:\startup.nsh then
        %v:
        goto FIRED
    endif
endfor
echo upgrade_ V0: could not locate the stick volume (no fsN:\startup.nsh).
goto DONE

:FIRED
echo fired > fired.txt
echo upgrade_ V0: handoff fired. Returning to Windows in 3 seconds.
stall 3000000
reset

:DONE
