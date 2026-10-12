# Status

Where every gate and every open risk stands, in one table. This is the only
doc that says so; the others link here. Each row names the CSV line that is
its latest evidence, and `./docs-check.sh` reads that line back and fails if
the result word differs. The story behind each row is in `log/`.

```text
[####]  real machine   [###.]  rig   [##..]  built, untried
[#...]  planned        [....]  not started   [FAIL]  failed for real
```

A gate (`V##`) is a thing to prove, defined in `VALIDATION.md`. A risk
(`R##`) is a thing that may be wrong, defined in `RISKS.md`. The state word
is the one in that heading. "Line N" is the CSV's line as an editor shows
it; line 1 is the header. One real machine is one data point, not proof:
nearly every `[####]` below is the Acer Aspire A515-51G, Secure Boot on.

## Right now (2026-10-12)

The keep-Windows path on the Aspire did itself on 2026-10-11: the return
put shim in Windows' slot from its own evidence, and the clock held (R21,
R36). The owner decided the PowerShell leaves the product; phase 1 is done.
On 2026-10-12 the rollback ran on the Aspire for the first time: files back
byte for byte, guard gone, Linux untouched, but the firmware kept Fedora
first, because a bcdedit boot-order change does not survive a restart on
this Acer while efibootmgr's does (`r21-rollback.csv` line 6). The same run
found that kickstart 0.9.0 saves GRUB's "Windows chosen" flag on `/boot`,
where the return cannot read it. Left: those two fixes, the unforced rig row
(the rig's guest channel is down), a real Windows update under the guard
(R22), phases 2 and 3 of the leaving. The order and what each waits on:
`ROADMAP.md`.

## Gates

| ID | What | State | Latest evidence | Owed |
|---|---|---|---|---|
| V0 | the boot handoff fires once, Secure Boot on | `[####]` held-on-machine, one vendor | `v0-handoff.csv` line 12, `fired-once` (2026-09-12, the Aspire; rig line 15) | Dell, Lenovo, HP; the fail-safe rows on real firmware |
| V1 | the unattended install completes, Secure Boot on | `[####]` held-on-machine | `v9-erase.csv` line 30, `erased-installed` (2026-10-04); live boot `v1-live-boot.csv` line 5 | other vendors; a plain-words screen for a `%pre` refusal |
| V1b | install beside a shrunk Windows, both still boot | `[####]` held-on-machine, one dying drive | `v2-install.csv` line 15, `pass` (2026-10-11, the return applied shim in Windows' slot by itself); `r21-rollback.csv` line 6, `windows-not-direct` (2026-10-12: files back, guard gone, but bcdedit's order did not survive the restart on the Acer) | the rollback's order half on such firmware; the GRUB flag's home on the ESP; the unforced rig row; a Windows update under the guard; a healthy-drive machine |
| V2 | extracted amp firmware makes the speakers work | `[#...]` planned | none yet | the G16 experiment |
| V3 | the kept BitLocker volume reads from Linux | `[###.]` held-on-rig | `v3-bitlk-read.csv` line 16, `pass-plumbing` (2026-09-02) | a real disk; `settle-in`'s pull |
| V4 | real disks can shrink enough | `[####]` held-on-machine, one disk (its old Windows: no) | `r18-prologue.csv` line 18, `pass-plumbing` (2026-10-11, the fresh Windows 11, 238.2 to 213.2 GB) | the rate from more machines |
| V5 | RST and VMD detection on real hardware | `[####]` held-on-machine, the warn direction only | `v5-controller-mode.csv` line 2, `warn-rst-on-ahci` (2026-09-13) | the RAID/VMD row: an 11th-gen+ machine, or firmware that honours boot-to-setup |
| V6 | a signed binary earns Defender's tolerance | `[#...]` planned | none yet | an entity, a certificate, and the calendar |
| V7 | the scanner generalizes beyond one laptop | `[#...]` planned | none yet | ship it, collect reports |
| V8 | OneDrive placeholders are never copied as files | `[###.]` held-on-rig | `v8-materialize.csv` line 5, `pass-plumbing` (2026-09-08, a synthetic provider on the G16) | a signed-in OneDrive; `settle-in`'s refusal (not built) |
| V9 | one-click erase and install | `[####]` held-on-machine, one vendor | `v9-erase.csv` line 32, `cancelled-untouched` (2026-10-06); the install, lines 30 and 31 (2026-10-04) | other vendors; carrying files (stage 2) |
| V10 | `settle-in`'s first start on any Linux | `[####]` held-on-machine, Fedora only | `settle-in-first-start.csv` line 20, `pass` (2026-10-04) | a non-Fedora distribution; the old-entry button on real firmware; the file pull |
| V11 | the way back to Windows | `[####]` held-on-machine, one licence kind | `v11-walkaway.csv` line 5, `windows-reached` (2026-10-11, run 3); line 4 `cancelled-untouched` (2026-10-06) | a machine with a firmware key; a cancel started by the program's own restart |
| V12 | the window in front of the programs | `[###.]` held-on-rig, the stop path only | `v12-window.csv` line 3, `stopped-before-arm` (2026-10-08) | a row on the Aspire: the reopen after restart, the result screen |
| V13 | the port from PowerShell to Rust | `[####]` held-on-machine | `v2-install.csv` line 15, `pass` (2026-10-11, the whole conversion through the Rust prologue); `r18-prologue.csv` line 19, `pass-plumbing` (the Rust-only bench); ledger 657 pass, 45 owed | the unforced rig row; the 45 owed lines; phase 2 (storage-mode harness, diag tools, stick writer); phase 3 (the data tables) |
| V14 | the kit's Linux release starts where it is used | `[####]` held-on-machine, one firmware | `v9-erase.csv` line 30, `erased-installed` (2026-10-04, Fedora 44, Secure Boot on) | Fedora 42's measured record; a second firmware; the next SBAT level |

## Open risks

| ID | What | State | Latest evidence | Owed |
|---|---|---|---|---|
| R1 | VMD detection has never fired on real hardware | `[###.]` held-on-rig | `v5-controller-mode.csv` line 2, `warn-rst-on-ahci` (the AHCI direction only) | a machine in RST RAID or VMD mode |
| R2 | two-machine test corpus | `[##..]` built | none yet (captures: the G16, the Aspire, one spoof) | spread across vendors (V7) |
| R3 | the distro kernel table is stale | `[#...]` planned | none yet | verify against release notes; Ubuntu 26.04 |
| R4 | scanner and converter contradict each other | `[##..]` built | none yet | the stick-fit numbers fired on a job |
| R5 | single-user assumption | `[##..]` built | none yet | the refusal fired on a machine with other profiles; reclaim's refusal |
| R6 | truncated sizing undersizes the backup | `[##..]` built | none yet | the refusal fired for real |
| R7 | Broadcom fallback over-refuses | `[#...]` planned | none yet | a Broadcom machine |
| R8 | cloud-only files copied as stubs | `[##..]` built | `v8-materialize.csv` line 5, `pass-plumbing` (materializer, unused) | `settle-in`'s refusal; a signed-in OneDrive |
| R9 | no CI; `dist/` can drift | `[#...]` planned | none yet | a workflow |
| R10 | NVIDIA detection matches display text | `[#...]` planned | none yet | a PCI-ID match |
| R11 | the Secure Boot check verifies nothing | `[#...]` planned | none yet | the owner's call whether R34's check meets this clause |
| R12 | code signing and antivirus | `[#...]` planned | none yet | a certificate (V6) |
| R13 | the stick as a credential store | `[##..]` built | none yet | a row naming the Wi-Fi removal from the stick; the BitLocker key wipe after the pull |
| R14 | supply chain | `[####]` held-on-machine, the reproducible half | `r14-rebuild.csv` line 26, `same` = y (2026-10-09, five programs) | signing; the release credential |
| R15 | the handoff on other vendors' firmware | `[####]` held-on-machine, one vendor | `v0-handoff.csv` line 12, `fired-once` | Dell, Lenovo, HP; fail-safe rows on real firmware |
| R16 | the stick writer writes the wrong device | `[####]` held-on-machine | `r16-stick-writer.csv` line 12, `verified` = y (2026-09-13, the G16) | a second device class; the writer in Rust |
| R17 | counterfeit or failing flash as the carrier | `[####]` held-on-machine, the gate half | `v1-live-boot.csv` line 5, `pass-plumbing` (read back at 22.6 MB/s) | the counterfeit half: a fake stick |
| R18 | shrink headroom unmeasured | `[####]` held-on-machine, one disk | `r18-prologue.csv` line 18, `pass-plumbing` (2026-10-11) | more machines; the pagefile read back |
| R19 | the BitLocker read is a new trust dependency | `[###.]` held-on-rig | `v3-bitlk-read.csv` line 16, `pass-plumbing` | a real disk |
| R20 | browser profile porting unverified | `[#...]` planned | none yet | one move per browser and version |
| R21 | the alongside install may break Windows boot | `[####]` held-on-machine, the return's own evidence | `v2-install.csv` line 15, `pass` (2026-10-11); `r21-rollback.csv` line 6, `windows-not-direct` (2026-10-12, the first physical rollback: everything held but the direct boot) | the rollback's order half on the Acer; the GRUB flag saved where Windows can read it; the unforced rig row; a Windows update under the guard; other vendors |
| R22 | servicing re-takes the boot order | `[###.]` held-on-rig | `v2-install.csv` line 13, `pass-plumbing` (the guard re-applied, 2026-10-10; ran at every Aspire start since, nothing to re-apply) | a real Windows update under the guard; `settle-in`'s order unit |
| R23 | the acknowledged-data-loss path | `[####]` held-on-machine | `r18-prologue.csv` line 16, `pass-plumbing` (in use on the Aspire since run 4) | nothing to widen; it stays two refusals (rule #1) |
| R24 | the walk-away resume as SYSTEM | `[####]` held-on-machine | `walkaway-probe.csv` line 5, `resumed-unattended` (2026-10-10, the Rust prologue) | BitLocker with a PIN |
| R25 | Windows Update restarts during the prologue | `[####]` held-on-machine | `r18-prologue.csv` line 18, `pass-plumbing` (2026-10-11, keep-Windows: the update let finish, then shrink and handoff) | Windows 11's CBS markers; the injected rig row |
| R26 | the offer to discard Windows | `[#...]` planned | none yet (the harvest half: `harvest-folder-map.csv`) | staging on the exFAT partition; the live-session check; build it |
| R27 | one-click erase and install | `[####]` held-on-machine | `v9-erase.csv` line 32, `cancelled-untouched` (2026-10-06) | other vendors; carrying files |
| R28 | `settle-in` runs on any Linux | `[####]` held-on-machine, Fedora | `settle-in-first-start.csv` line 20, `pass` | a non-Fedora distribution; the old-entry button on real firmware |
| R29 | the third-party UEFI CA off on Secured-core PCs | `[#...]` planned | none yet | Microsoft's own document; a Secured-core machine; reading the setting from Windows |
| R30 | the way back to Windows | `[###.]` held-on-rig | `v11-way-back.csv` line 3, `pass-plumbing` (2026-09-27) | a machine with a firmware key; `install.swm` with Secure Boot on |
| R31 | the window in front of the programs | `[##..]` built | `v12-window.csv` line 3, `stopped-before-arm` | a passing row; the words approved |
| R32 | the port from PowerShell to Rust | `[####]` held-on-machine, the scripts gone (2026-10-11) | `v2-install.csv` line 15, `pass` (the Rust prologue, the whole path); `r18-prologue.csv` line 19, `pass-plumbing` (the Rust-only bench) | the unforced rig row; the 45 owed ledger lines; phase 2 and 3 of the leaving |
| R33 | the walk-away way back | `[####]` held-on-machine | `v11-walkaway.csv` line 5, `windows-reached` (2026-10-11) | a cancel from the program's own restart; a machine that stops at the key page; keeping the machine awake |
| R34 | Secure Boot revokes the kit's boot files | `[####]` held-on-machine | `v9-erase.csv` line 30, `erased-installed` (Fedora 44, Secure Boot on) | the next level (`grub,6`); Fedora 42's record |
| R35 | the stick installs again after the install | `[####]` held-on-machine | `v9-erase.csv` line 31, `not-installed-again` (2026-10-04) | other firmware |
| R36 | the kept Windows' clock after a Linux boot | `[####]` held-on-machine | `v2-install.csv` line 15, `pass` (2026-10-11, +1 s, +2 s, +1 s after each Linux boot) | the rig's two boots on the keep-Windows arm |

Closed: F1 to F4, the four fixed defects of 2026-08-22 (`RISKS.md`, Closed).
