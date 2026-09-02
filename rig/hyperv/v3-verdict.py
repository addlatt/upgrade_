#!/usr/bin/env python3
"""V3 / R19 verdict: compare the Windows-side and Linux-side manifests of the
same BitLocker volume and append ONE row to docs/validation-results/
v3-bitlk-read.csv. Refuse-by-default: any byte-different file that Windows
does not itself rewrite between the hash and the shutdown is a FAIL.

usage: v3-verdict.py <artifacts dir with oemdrv.img> <csv> <harness version> <firmware> <fedora context>
"""
import csv, datetime, os, re, subprocess, sys

A, CSV, HARNESS, FIRMWARE, CONTEXT = sys.argv[1:6]
DRY = CSV == '-'          # compare and print only; no row (mid-run peeks)
IMG = os.path.join(A, 'oemdrv.img') + '@@1M'
OUT = os.path.join(A, 'verdict')
os.makedirs(OUT, exist_ok=True)

# Windows rewrites these between the manifest and the shutdown (registry
# hives, logs, caches; CryptnetUrlCache metadata added 2026-09-01 after a
# 330-byte entry changed post-hash and BOTH Linux drivers read the same new
# bytes); a difference there is expected, NOT evidence of a bad read.
# Everything else under Users is held to byte-identical.
VOLATILE = re.compile(r'(ntuser\.dat|usrclass\.dat|\.log$|\.log\d*$|\.blf$|\.regtrans-ms$|\.etl$|\.tmp$|\.jfm$|\.aodl$|-wal$|-shm$|/lockfile$|'
                      r'/appdata/local/temp/|/inetcache/|/webcache/|/iconcache|/microsoft/windows/explorer/|/microsoft/windows/caches/|/cryptneturlcache/|'
                      r'/microsoft/windows/powershell/|/microsoft/windows/notifications/|/connecteddevicesplatform/|/microsoft/onedrive/|'
                      r'/settings/settings\.dat|/tokenbroker/|/packages/[^/]+/(localstate|ac|tempstate|settings)/|/dumpstack)', re.I)

def mtype(name):
    r = subprocess.run(['mtype', '-i', IMG, '::/v3/' + name], capture_output=True)
    if r.returncode != 0:
        return None
    return r.stdout.decode('utf-8', 'replace')

def manifest(name):
    t = mtype(name)
    if t is None:
        return None
    rows = {}
    for line in t.replace('\r\n', '\n').split('\n'):
        if not line:
            continue
        sha, size, rel = line.split('\t', 2)
        rows[rel] = (sha, int(size))
    return rows

def facts(name):
    t = mtype(name) or ''
    d = {}
    for line in t.replace('\r\n', '\n').split('\n'):
        if '=' in line:
            k, v = line.split('=', 1); d[k] = v
    return d

fw = facts('facts-win.txt'); fl = facts('facts-linux.txt')
def compare(win, lin, label):
    """returns dict of counts + writes the differing paths to OUT/<label>.txt"""
    if win is None or lin is None:
        return None
    ident = mism = volatile = only_w = only_l = err_w = err_l = special = 0
    lines = []
    for rel in sorted(set(win) | set(lin)):
        w, l = win.get(rel), lin.get(rel)
        if w is None or l is None:
            side = 'only-linux' if w is None else 'only-windows'
            if VOLATILE.search('/' + rel):
                volatile += 1; lines.append('volatile-%s\t%s' % (side, rel))
            elif w is None:
                only_l += 1; lines.append('only-linux\t%s' % rel)
            else:
                only_w += 1; lines.append('only-windows\t%s' % rel)
            continue
        if w[0].startswith('ERR:'):
            err_w += 1; lines.append('win-unreadable\t%s\t%s' % (w[0], rel)); continue
        if l[0].startswith('ERR:'):
            err_l += 1; lines.append('linux-unreadable\t%s\t%s' % (l[0], rel)); continue
        if l[0].startswith('SPECIAL:'):
            # Linux presents it as a non-regular file (a FIFO for a 0-byte SYSTEM file under ntfs-3g): a
            # plain open() blocks forever. Design input for settle-in (lstat first). Its bytes are still
            # fully known only if Windows saw it empty; otherwise it is an unreadable file.
            if w[1] == 0:
                special += 1; lines.append('special-empty\t%s\t%s' % (l[0], rel))
            else:
                err_l += 1; lines.append('linux-unreadable\t%s (windows size %d)\t%s' % (l[0], w[1], rel))
            continue
        if w == l:
            ident += 1
        elif VOLATILE.search('/' + rel):
            volatile += 1; lines.append('volatile-differs\t%s' % rel)
        else:
            mism += 1; lines.append('MISMATCH\twin=%s/%d\tlinux=%s/%d\t%s' % (w[0][:16], w[1], l[0][:16], l[1], rel))
    with open(os.path.join(OUT, label + '.txt'), 'w', encoding='utf-8') as f:
        f.write('\n'.join(lines) + ('\n' if lines else ''))
    return dict(identical=ident, mismatch=mism, volatile=volatile, only_windows=only_w, only_linux=only_l,
                win_unreadable=err_w, linux_unreadable=err_l, special_empty=special, total=len(set(win) | set(lin)))

def summ(c):
    if c is None: return 'missing'
    return '%d/%d identical, %d mismatch, %d volatile, %d only-win, %d only-linux, %d win-unreadable, %d linux-unreadable, %d special-empty' % (
        c['identical'], c['total'], c['mismatch'], c['volatile'], c['only_windows'], c['only_linux'], c['win_unreadable'], c['linux_unreadable'], c['special_empty'])

prev_oops = mtype('journal-prev-boot-kernel.txt') or ''
crashed_prev = bool(re.search(r'Oops|BUG:|general protection|Call Trace', prev_oops))
stage = fl.get('stage', '')
win_corpus = manifest('manifest-win-corpus.txt'); win_users = manifest('manifest-win-users.txt')

rows = []
for tag, drv in (('ntfs3g', 'ntfs-3g'), ('ntfs3', 'ntfs3'), ('ntfs3ra0', 'ntfs3 (readahead 0)')):
    corpus = compare(win_corpus, manifest('manifest-linux-%s-corpus.txt' % tag), 'corpus-' + tag)
    users  = compare(win_users,  manifest('manifest-linux-%s-users.txt' % tag),  'users-' + tag)
    notes = []
    if not fl:
        result = 'incomplete'; notes.append('no Linux facts reached OEMDRV (guest never ran the reader, or hung before its first sync)')
    elif stage == 'no-key':
        result = 'incomplete'; notes.append('no key reached the guest')
    elif fl.get('open_rc') != '0':
        result = 'unlock-failed'; notes.append('cryptsetup open rc=%s: %s' % (fl.get('open_rc'), fl.get('open_message')))
    elif fl.get('mount_%s_rc' % tag) is None:
        result = 'guest-crashed' if stage.startswith(tag + ':') or (stage != 'done' and tag == 'ntfs3' and not stage.startswith('ntfs3g')) else 'incomplete'
        notes.append('driver pass never started (last stage: %s)' % stage)
    elif fl.get('mount_%s_rc' % tag) != '0':
        result = 'mount-failed'; notes.append('%s mount rc=%s %s' % (drv, fl.get('mount_%s_rc' % tag), fl.get('mount_%s_message' % tag, '')))
    elif fl.get('hash_%s_seconds' % tag) is None:
        result = 'guest-crashed'; notes.append('guest stopped mid-read at stage: %s' % stage)
    elif corpus is None or users is None:
        result = 'incomplete'; notes.append('manifests missing (corpus=%s users=%s)' % (corpus is not None, users is not None))
    elif corpus['mismatch'] or users['mismatch']:
        result = 'mismatch'
    elif corpus['linux_unreadable'] or users['linux_unreadable'] or corpus['only_windows'] or users['only_windows']:
        result = 'read-errors'
    elif int(fw.get('corpus_files', '0')) < 2000:
        result = 'incomplete'; notes.append('corpus too small (%s files)' % fw.get('corpus_files'))
    else:
        result = 'pass-plumbing'
    if 'does not match' in fl.get('open_message', ''):
        notes.append('cryptsetup warned: ' + fl['open_message'].strip())
    notes.append('corpus: ' + summ(corpus))
    notes.append('users: ' + summ(users))
    if users and users['volatile']:
        notes.append('volatile paths listed in artifacts/v3/verdict/users-%s.txt' % tag)
    if (users and users['special_empty']) or (corpus and corpus['special_empty']):
        notes.append('%d empty Windows SYSTEM files appear as FIFOs on Linux: settle-in must lstat before open or it hangs' % ((users or {}).get('special_empty', 0) + (corpus or {}).get('special_empty', 0)))
    if crashed_prev:
        notes.append('PREVIOUS attempt of this run oopsed (journal-prev-boot-kernel.txt): ' + (re.search(r'(BUG:[^\n]*|Oops[^\n]*|general protection[^\n]*)', prev_oops).group(1)[:160] if re.search(r'(BUG:[^\n]*|Oops[^\n]*|general protection[^\n]*)', prev_oops) else 'see file'))
    if fw.get('hiberboot_enabled') not in ('0', ''):
        notes.append('HiberbootEnabled=%s (full shutdown forced by the harness)' % fw.get('hiberboot_enabled'))
    rows.append({
        'timestamp': datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ'),
        'harness': HARNESS,
        'firmware': FIRMWARE,
        'config': fw.get('config', ''),
        'windows_build': fw.get('windows_build', ''),
        'bitlocker_method': fw.get('bitlocker_method', ''),
        'used_space_only': 'yes' if 'Used Space Only' in fw.get('used_space_only', '') else ('no' if fw.get('used_space_only') else ''),
        'protectors': fw.get('bitlocker_protectors', ''),
        'volume_status': fw.get('bitlocker_status', ''),
        'partition_bytes': fw.get('partition_bytes', ''),
        'bitlk_volume_bytes': fl.get('bitlk_volume_bytes', ''),
        'linux_context': CONTEXT,
        'kernel': fl.get('kernel', ''),
        'cryptsetup': fl.get('cryptsetup', ''),
        'unlock': 'ok' if fl.get('open_rc') == '0' else ('failed' if fl.get('open_rc') else 'not-reached'),
        'ntfs_driver': drv,
        'corpus_files': fw.get('corpus_files', ''),
        'corpus_identical': '' if corpus is None else corpus['identical'],
        'corpus_mismatch': '' if corpus is None else corpus['mismatch'],
        'users_files': fw.get('users_files', ''),
        'users_identical': '' if users is None else users['identical'],
        'users_mismatch': '' if users is None else users['mismatch'],
        'users_volatile': '' if users is None else users['volatile'],
        'result': result,
        'notes': ' | '.join(notes),
    })

if not DRY:
    new = not os.path.exists(CSV)
    with open(CSV, 'a', newline='', encoding='utf-8') as f:
        w = csv.DictWriter(f, fieldnames=list(rows[0]))
        if new: w.writeheader()
        for row in rows: w.writerow(row)
for row in rows:
    print('v3-verdict: %s (%s)' % (row['result'], row['ntfs_driver']))
    for k, v in row.items(): print('  %s: %s' % (k, v))
