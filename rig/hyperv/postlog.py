"""
postlog.py - what the verdicts read in the installer's post step (upgrade-installer
0.1.3+, post.rs): report/post.log and report/grub.cfg as `outcome` copies them to the
stick. Shared by v9-verdict.py (arm C) and v2-verdict.py (keep-Windows). Tooling only.
"""
import re

def installer_version(verify_log_text):
    m = re.search(r"upgrade-installer (\d+)\.(\d+)\.(\d+)", verify_log_text or "")
    return tuple(int(x) for x in m.groups()) if m else None

def has_post_step(verify_log_text):
    v = installer_version(verify_log_text)
    return v is not None and v >= (0, 1, 3)

def commands(text):
    """[(command, output lines, ok)] in the order the log holds them."""
    out, cur = [], None
    for line in (text or "").splitlines():
        if line.startswith("+ (exit "):
            if cur: cur[2] = False
            continue
        if line.startswith("+ "):
            cur = [line[2:], [], True]; out.append(cur); continue
        if cur: cur[1].append(line)
    return [tuple(c) for c in out]

def target_set(text, want):
    """(ok, why): `systemctl set-default <want>` ran and did not fail, and get-default reads it back."""
    cs = commands(text)
    sd = [c for c in cs if c[0].startswith("systemctl set-default ")]
    if not sd: return False, "post.log: no systemctl set-default"
    if sd[-1][0] != "systemctl set-default " + want: return False, "post.log: %s, the job asked %s" % (sd[-1][0], want)
    if not sd[-1][2]: return False, "post.log: %s failed" % sd[-1][0]
    gd = [c for c in cs if c[0] == "systemctl get-default"]
    if not gd or want not in "\n".join(gd[-1][1]): return False, "post.log: get-default does not read %s" % want
    return True, "post.log: start target %s set and read back" % want

def kept_entry(post_text, grub_cfg, esp_serial):
    """(ok, why) for keep-Windows: os-prober off, our entry in grub.cfg naming the ESP's real UUID, no other Windows entry."""
    why = []
    cs = commands(post_text)
    if not any(c[0] == "write /etc/default/grub (GRUB_DISABLE_OS_PROBER=true)" for c in cs):
        why.append("post.log: /etc/default/grub not written with os-prober off")
    m = re.search(r"^\+ findmnt -no UUID /boot/efi: (\S+)", post_text or "", re.M)
    found = m.group(1) if m else None
    entries = re.findall(r"^menuentry '([^']*[Ww]indows[^']*)'", grub_cfg or "", re.M)
    uuid = re.search(r"menuentry 'Windows \(kept\)'.*?search --no-floppy --fs-uuid --set=root (\S+)", grub_cfg or "", re.S)
    uuid = uuid.group(1) if uuid else None
    if entries != ["Windows (kept)"]: why.append("grub.cfg's Windows entries: %s (want only 'Windows (kept)'; another one means os-prober ran)" % (entries or "none"))
    if not uuid: why.append("grub.cfg: the kept entry names no ESP")
    elif esp_serial and uuid.upper() != esp_serial.upper(): why.append("grub.cfg names ESP %s, the disk's ESP is %s" % (uuid, esp_serial))
    if found and uuid and found.upper() != uuid.upper(): why.append("post.log read ESP %s, grub.cfg names %s" % (found, uuid))
    if not esp_serial: why.append("the ESP's serial was not read offline")
    if why: return False, "; ".join(why)
    return True, "grub.cfg: 'Windows (kept)' alone, ESP %s (the disk's own, read offline); os-prober off in /etc/default/grub" % uuid
