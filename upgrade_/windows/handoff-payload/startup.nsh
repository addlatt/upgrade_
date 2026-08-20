# upgrade_ / V0 handoff payload
#
# The UEFI Shell runs this file automatically at startup. Its whole job is to
# prove, in a machine-readable way, that the firmware actually ran our boot
# entry - then get out of the way and go back to Windows.
#
# Keep the marker name in sync with $FiredMarker in Test-Handoff.ps1.
#
# fs0: is the first mapped filesystem, which for a single-partition FAT32 stick
# is the stick itself. If the shell maps more volumes, adjust to the fsN: that
# holds this script (run `map` at the shell prompt to see them).

fs0:
echo fired > fired.txt
echo upgrade_ V0: handoff fired. Returning to Windows in 3 seconds.
stall 3000000
reset
