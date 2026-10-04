#!/usr/bin/env python3
"""
pe_facts.py - the facts Secure Boot judges a boot file by, read from the file
itself (RISKS R34, decided 2026-10-03). Never typed by hand.

    pe_facts.py FILE...      -> one JSON object per file on stdout

For each PE (EFI) file: sha256; the text of its .sbat section (the SBAT
generations it declares, judged against the firmware's revocation level by
shim's rule); for a shim, the revocation levels built into its .sbatlevel
section (shim applies them itself); and the subject of every certificate
authority its Authenticode signatures chain to (which Microsoft keys must
be in the firmware's db for it to start: 2011 or 2023).
"""
import hashlib, json, re, struct, subprocess, sys, tempfile, os

def sections(d):
    pe = struct.unpack_from('<I', d, 0x3c)[0]
    assert d[pe:pe + 4] == b'PE\0\0', 'not a PE file'
    nsec = struct.unpack_from('<H', d, pe + 6)[0]
    opt = struct.unpack_from('<H', d, pe + 20)[0]
    tab = pe + 24 + opt
    out = {}
    for i in range(nsec):
        s = tab + 40 * i
        name = d[s:s + 8].rstrip(b'\0').decode('ascii', 'replace')
        if name.startswith('/') and name[1:].isdigit():
            # a name longer than 8 characters (.sbatlevel) lives in the COFF string table
            symtab, nsym = struct.unpack_from('<II', d, pe + 12)
            at0 = symtab + nsym * 18 + int(name[1:])
            name = d[at0:d.index(b'\0', at0)].decode('ascii', 'replace')
        size, at = struct.unpack_from('<II', d, s + 16)
        out[name] = d[at:at + size]
    return pe, out

def signers(d, pe):
    magic = struct.unpack_from('<H', d, pe + 24)[0]
    dd = pe + 24 + (112 if magic == 0x20b else 96)
    off, size = struct.unpack_from('<II', d, dd + 4 * 8)
    cas, pos = [], off
    while size and pos < off + size:
        ln = struct.unpack_from('<I', d, pos)[0]
        with tempfile.NamedTemporaryFile(delete=False) as t:
            t.write(d[pos + 8:pos + ln])
        try:
            txt = subprocess.run(['openssl', 'pkcs7', '-inform', 'DER', '-in', t.name, '-print_certs', '-noout'],
                                 capture_output=True, text=True).stdout
        finally:
            os.unlink(t.name)
        # the issuers named in the chain: the authority the firmware's db must hold is one of them
        for m in re.finditer(r'issuer=.*?CN ?= ?([^,\n]+)', txt):
            if m.group(1).strip() not in cas: cas.append(m.group(1).strip())
        pos += (ln + 7) // 8 * 8
    return cas

def facts(path):
    d = open(path, 'rb').read()
    pe, sec = sections(d)
    sbat = sec.get('.sbat', b'').split(b'\0')[0].decode('ascii', 'replace').strip()
    lvl = sec.get('.sbatlevel', b'')
    levels = [m.decode() for m in re.findall(rb'sbat,1,\d{10}[^\0]*', lvl)]
    return {'file': os.path.basename(path), 'sha256': hashlib.sha256(d).hexdigest(), 'sbat': sbat,
            'sbatlevel': levels, 'signed_by': signers(d, pe)}

if __name__ == '__main__':
    for p in sys.argv[1:]:
        print(json.dumps(facts(p)))
