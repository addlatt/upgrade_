# V1b / R21: Secure-Boot-on chainload via MOK (Hyper-V, 2026-08-31)

This is the one Secure-Boot-on clause of V1b/R21 that a VM on this host can
reach. It is recorded here as prose, **not** as a `v1b-alongside.csv` row.
The boot cycles were driven by hand at the console, so the marker unit /
`boots.log` flow that the CSV verdict script reads was not used, and there
is no harness-produced row to append. CLAUDE.md: the harness writes the CSV
rows; a human-driven experiment gets a human-written note, clearly
labelled.

A few terms first:

- **db** is the firmware's list of trusted signers. Under Secure Boot, the
  firmware only starts a boot loader signed by someone in db.
- **Shim** is a small first-stage boot loader, signed by Microsoft, that
  lets Linux boot with Secure Boot on. It then checks the next loader
  itself.
- **MOK** (Machine Owner Key) is shim's own extra list of trusted keys,
  called MokList. A key is added with `mokutil`, then confirmed at the
  next boot in MokManager.
- **Chainload** means GRUB (Linux's boot menu) hands over to Windows' boot
  loader, `bootmgfw.efi`.

## Why a second VM

`UPGRIGHV`'s Secure Boot template is locked to `MicrosoftWindows` once its
vTPM (virtual TPM security chip) initialised (measured 2026-08-30,
`rig/hyperv/README.md`), and that db refuses Fedora's shim. Hyper-V's two
SB templates are mutually exclusive: no db holds both the Windows CA and
the third-party UEFI CA that signs shim. So a Secure-Boot-on dual boot *as
it exists on real hardware* cannot be reproduced here.

The honest substitute for the **chainload** clause is a second guest,
`UPGRIGMOK`:

- It uses the `MicrosoftUEFICertificateAuthority` template, which trusts
  shim.
- The **Microsoft Windows Production PCA 2011** certificate is enrolled
  into shim's MokList, so shim will verify `bootmgfw.efi` on the GRUB
  chainload path. The certificate was extracted from this guest's own
  `bootmgfw.efi` Authenticode signature.

The setup:

- Guest: `UPGRIGMOK`, Gen 2, `MicrosoftUEFICertificateAuthority` template,
  fresh vTPM (`NewLocalKeyProtector`), disk copied from
  `UPGRIGHV.pre-install.vhdx` (post-32-GiB-shrink, pre-Fedora, BitLocker
  suspended). Firmware: Hyper-V UEFI Release v4.1.
- The PCA DER is `rig/hyperv/artifacts/v1b-mok/win-pca.der` (gitignored):
  `CN = Microsoft Windows Production PCA 2011`, SHA-1
  `58:0A:6F:4C:C4:E4:B6:69:B9:EB:DC:1B:2B:3E:08:7B:80:D0:67:8D`.

## What fired

1. **Alongside install under Secure Boot ENFORCING.** The Fedora 42 netinst
   booted, with shim → GRUB → kernel all signature-verified by the
   firmware's db. Anaconda (Fedora's installer) ran the kickstart
   (`rig/hyperv/v1b-mok-ks.cfg`) hands-off into the freed space, reusing
   the 100 MiB ESP (EFI System Partition, where boot loaders live)
   `--noformat`, and powered off. This is the V1/V1b
   install clause with SB actually on.
   - ESP after: 150 files, 65,971,200 B free.
   - The install added the same 7 files / 6,872,950 B as SB-off
     (`EFI/fedora/{shim,shimx64,mmx64,grubx64,grub.cfg,BOOTX64.CSV}` +
     `EFI/Boot/fbx64.efi`).
   - `EFI/Boot/bootx64.efi` was overwritten by shim again (finding 1,
     third firmware).
   - `bootmgfw.efi` byte-identical (`d1f7e351…`).
   - os-prober found Windows
     (`/dev/sda1@/EFI/Microsoft/Boot/bootmgfw.efi`).

2. **Secure Boot confirmed enforcing**, from both sides: Fedora
   `mokutil --sb-state` → `SecureBoot enabled`; and, once Windows booted,
   `Confirm-SecureBootUEFI` → `True`.

3. **NEGATIVE: before enrolment, the chainload is refused.** With the PCA
   not in MokList, selecting *Windows Boot Manager* in the GRUB menu
   produced:

   ```
   error: ../../grub-core/kern/efi/sb.c:192:bad shim signature.
   ```

   GRUB's `chainloader` asked shim to verify `bootmgfw.efi`. Shim checked db
   (UEFI-CA template, so no Windows CA) and MokList (empty) and
   **refused**. Windows was unbootable via GRUB. This is the Secure-Boot
   gate working on the chainload path. It is the evidence that the positive
   result below is not SB silently passing everything.

4. **POSITIVE: after enrolment, Windows boots.**
   - `mokutil --import` queued the PCA (`mokutil --list-new` →
     `580a6f4cc4 Microsoft Windows Production PCA 2011`).
   - MokManager enrolled it on the next boot (password entry, "Enroll the
     key(s)? → Yes").
   - `mokutil --test-key /root/win-pca.der` → `already enrolled`.
   - Selecting *Windows Boot Manager* in GRUB then booted Windows 10 (build
     19045) all the way to the desktop, Secure Boot enforcing throughout.
   - BitLocker (suspended on the copied disk) resumed by itself to
     protection On, with no recovery prompt (the fresh vTPM re-sealed on
     first Windows boot).

## What this does NOT show (do not overclaim)

- **Not** a real machine's db. Real hardware ships *both* the Windows CA and
  the UEFI CA in db, so its firmware boots `bootmgfw.efi` from its **own**
  Windows Boot Manager entry directly, with no shim and no MokList in the
  path. This experiment reproduces only the **chainload verification**:
  shim verifying a Microsoft-signed binary via an enrolled MOK. That is
  because Hyper-V's templates cannot express a both-CA db. Under the
  UEFI-CA template Windows' own firmware entry stays refused; only the GRUB
  path reaches it.
- **Not** the vendor matrix. This is one more firmware (Hyper-V UEFI v4.1),
  not the ≥3-vendor population. A VM pass narrows R21; it does not close it
  (CLAUDE.md rule #5).
- The db-composition clause and the physical vendor matrix stay open, on real
  hardware, regardless of this result.

## Reproduce

`rig/hyperv/README.md` → the MOK run-book. Tooling: `rig/hyperv/v1b.sh`
(env-overridable `VMNAME`/`LEG`/`SB`/`KS`/`OEM_EXTRA`), kickstart
`rig/hyperv/v1b-mok-ks.cfg`, cert `rig/hyperv/artifacts/v1b-mok/win-pca.der`.

Two traps on this rig:

- The console was driven by WMI virtual-key codes, because the
  `type`/`TypeText` path garbles characters here (see README).
- A failed chainload pollutes GRUB's `$root`. So boot Fedora from a *fresh*
  GRUB and use `grub2-reboot` to select Windows deterministically.
