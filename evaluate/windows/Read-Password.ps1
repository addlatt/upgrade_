<#
.SYNOPSIS
    upgrade_ - ask for the new Linux account's password, write only its hash.

.DESCRIPTION
    The erase-and-install launchers (RISKS R27) run this directly, not
    through Invoke-Logged: the password is typed twice, hidden, and never
    written anywhere - not to a log, not to the stick, not to a command
    line. What is written, to -OutFile, is its SHA-512 crypt hash
    ("$6$<salt>$<hash>", the format Fedora's /etc/shadow and kickstart's
    `user --iscrypted` use), which the job writer puts in job.json
    (schemas/README.md: "the account password is SHA-512 crypt or nothing").

    Windows PowerShell 5.1 has no crypt(3), so the algorithm is written out
    here from its specification (Ulrich Drepper, "Unix crypt using SHA-256
    and SHA-512", akkadia.org/drepper/SHA-crypt.txt) and self-tested against
    that document's test vectors and against `openssl passwd -6`.

.PARAMETER OutFile
    Where to write the hash (one line). The file holds no secret a login
    prompt does not already accept, but it is still deleted by the launcher
    once the job is written.
#>
[CmdletBinding()]
param(
    [string]$OutFile,
    [string]$LinuxName,
    [switch]$SelfTest
)
$ErrorActionPreference = 'Stop'
$ReadPasswordVersion = '0.2.0'
$CryptAlphabet = './0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz'

function Get-Sha512 {
    param([byte[]]$Bytes)
    $h = [Security.Cryptography.SHA512]::Create()
    try { , $h.ComputeHash($Bytes) } finally { $h.Dispose() }
}

function Join-Bytes {
    param([object[]]$Parts)
    $ms = New-Object IO.MemoryStream
    foreach ($p in $Parts) { if ($p -and $p.Length) { $ms.Write([byte[]]$p, 0, $p.Length) } }
    , $ms.ToArray()
}

function Get-Repeated {
    # the first $Length bytes of $Block repeated
    param([byte[]]$Block, [int]$Length)
    $out = New-Object byte[] $Length
    for ($i = 0; $i -lt $Length; $i++) { $out[$i] = $Block[$i % $Block.Length] }
    , $out
}

function ConvertTo-Sha512Crypt {
    # Pure (self-tested): SHA-512 crypt of $Password with $Salt (at most 16
    # characters are used) and $Rounds (default 5000, clamped 1000..999999999,
    # written into the result only when not the default) - Drepper's steps 1-22.
    param([string]$Password, [string]$Salt, [int]$Rounds = 5000, [switch]$RoundsGiven)
    $P = [Text.Encoding]::UTF8.GetBytes($Password)
    if ($Salt.Length -gt 16) { $Salt = $Salt.Substring(0, 16) }
    $S = [Text.Encoding]::UTF8.GetBytes($Salt)
    if ($Rounds -lt 1000) { $Rounds = 1000 }
    # steps 4-8: B = H(P S P)
    $B = Get-Sha512 (Join-Bytes @($P, $S, $P))
    # steps 1-3, 9-12: A = H(P S B-repeated-to-|P| then, per bit of |P|, B or P)
    $ma = New-Object IO.MemoryStream
    $ma.Write($P, 0, $P.Length); $ma.Write($S, 0, $S.Length)
    $n = $P.Length
    while ($n -gt 64) { $ma.Write($B, 0, 64); $n -= 64 }
    $ma.Write($B, 0, $n)
    for ($n = $P.Length; $n -gt 0; $n = $n -shr 1) {
        if ($n -band 1) { $ma.Write($B, 0, 64) } else { $ma.Write($P, 0, $P.Length) }
    }
    $A = Get-Sha512 $ma.ToArray()
    # steps 13-16: P-sequence from H(P repeated |P| times)
    $dp = New-Object IO.MemoryStream
    for ($i = 0; $i -lt $P.Length; $i++) { $dp.Write($P, 0, $P.Length) }
    $PS = Get-Repeated (Get-Sha512 $dp.ToArray()) $P.Length
    # steps 17-20: S-sequence from H(S repeated 16 + A[0] times)
    $ds = New-Object IO.MemoryStream
    for ($i = 0; $i -lt (16 + [int]$A[0]); $i++) { $ds.Write($S, 0, $S.Length) }
    $SS = Get-Repeated (Get-Sha512 $ds.ToArray()) $S.Length
    # step 21: the rounds
    $h = [Security.Cryptography.SHA512]::Create()
    try {
        $C = $A
        for ($i = 0; $i -lt $Rounds; $i++) {
            $mc = New-Object IO.MemoryStream
            if ($i -band 1) { $mc.Write($PS, 0, $PS.Length) } else { $mc.Write($C, 0, 64) }
            if ($i % 3) { $mc.Write($SS, 0, $SS.Length) }
            if ($i % 7) { $mc.Write($PS, 0, $PS.Length) }
            if ($i -band 1) { $mc.Write($C, 0, 64) } else { $mc.Write($PS, 0, $PS.Length) }
            $C = $h.ComputeHash($mc.ToArray())
        }
    } finally { $h.Dispose() }
    # step 22: the SHA-512 byte order into crypt's base-64
    $order = @(@(0,21,42),@(22,43,1),@(44,2,23),@(3,24,45),@(25,46,4),@(47,5,26),@(6,27,48),@(28,49,7),@(50,8,29),@(9,30,51),@(31,52,10),
               @(53,11,32),@(12,33,54),@(34,55,13),@(56,14,35),@(15,36,57),@(37,58,16),@(59,17,38),@(18,39,60),@(40,61,19),@(62,20,41))
    $sb = New-Object Text.StringBuilder
    foreach ($t in $order) {
        $w = ([int]$C[$t[0]] -shl 16) -bor ([int]$C[$t[1]] -shl 8) -bor [int]$C[$t[2]]
        for ($k = 0; $k -lt 4; $k++) { [void]$sb.Append($CryptAlphabet[$w -band 63]); $w = $w -shr 6 }
    }
    $w = [int]$C[63]
    for ($k = 0; $k -lt 2; $k++) { [void]$sb.Append($CryptAlphabet[$w -band 63]); $w = $w -shr 6 }
    $prefix = if ($RoundsGiven) { "`$6`$rounds=$Rounds`$" } else { '$6$' }
    "$prefix$Salt`$$($sb.ToString())"
}

function New-CryptSalt {
    # 16 characters from crypt's alphabet, from the OS random source
    $rng = New-Object Security.Cryptography.RNGCryptoServiceProvider
    $b = New-Object byte[] 16; $rng.GetBytes($b); $rng.Dispose()
    -join ($b | ForEach-Object { $CryptAlphabet[$_ % 64] })
}

function ConvertFrom-SecurePlain {
    param([Security.SecureString]$Secure)
    $ptr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($Secure)
    try { [Runtime.InteropServices.Marshal]::PtrToStringBSTR($ptr) } finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($ptr) }
}

function Test-PasswordPair {
    # Pure (self-tested): the refusal for two typed entries, or $null.
    param([string]$First, [string]$Second)
    if (-not $First) { return 'the password is empty' }
    if ($First -cne $Second) { return 'the two entries are not the same' }
    if ($First -match '[\x00-\x1f\x7f]') { return 'the password contains a control character' }
    $null
}

function Invoke-SelfTest {
    $failed = 0
    Write-Host ''; Write-Host "  upgrade_  password hasher $ReadPasswordVersion  -  SELF-TEST" -ForegroundColor Cyan; Write-Host ''
    $cases = @(
        # Drepper, SHA-crypt.txt, SHA-512 test vectors
        @{ Name = 'spec vector 1: "Hello world!", salt saltstring, default rounds'
           Run = { ConvertTo-Sha512Crypt -Password 'Hello world!' -Salt 'saltstring' }
           Expect = '$6$saltstring$svn8UoSVapNtMuq1ukKS4tPQd8iKwSMHWjl/O817G3uBnIFNjnQJuesI68u4OTLiBFdcbYEdFCoEOfaS35inz1' }
        @{ Name = 'spec vector 2: rounds=10000, a salt over 16 characters is cut to 16'
           Run = { ConvertTo-Sha512Crypt -Password 'Hello world!' -Salt 'saltstringsaltstring' -Rounds 10000 -RoundsGiven }
           Expect = '$6$rounds=10000$saltstringsaltst$OW1/O6BYHV6BcXZu8QVeXbDWra3Oeqh0sbHbbMCVNSnCM/UrjmM0Dp8vOuZeHBy/YTBmSK6H9qs/y3RnOaw5v.' }
        @{ Name = 'spec vector 3: rounds=5000 written out, "This is just a test"'
           Run = { ConvertTo-Sha512Crypt -Password 'This is just a test' -Salt 'toolongsaltstring' -Rounds 5000 -RoundsGiven }
           Expect = '$6$rounds=5000$toolongsaltstrin$lQ8jolhgVRVhY4b5pZKaysCLi0QBxGoNeKQzQ3glMhwllF7oGDZxUhx1yxdYcz/e1JSbq3y6JMxxl8audkUEm0' }
        @{ Name = 'spec vector 4: a password longer than 64 bytes (rounds=1400)'
           Run = { ConvertTo-Sha512Crypt -Password 'a very much longer text to encrypt.  This one even stretches over morethan one line.' -Salt 'anotherlongsaltstring' -Rounds 1400 -RoundsGiven }
           Expect = '$6$rounds=1400$anotherlongsalts$POfYwTEok97VWcjxIiSOjiykti.o/pQs.wPvMxQ6Fm7I6IoYN3CmLs66x9t0oSwbtEW7o7UmJEiDwGqd8p4ur1' }
        # vector 5 is the spec's; glibc 2.39 now refuses rounds < 1000 outright (checked 2026-09-26). This tool always uses 5000.
        @{ Name = 'spec vector 5: rounds below 1000 are raised to 1000'
           Run = { ConvertTo-Sha512Crypt -Password 'the minimum number is still observed' -Salt 'roundstoolow' -Rounds 10 -RoundsGiven }
           Expect = '$6$rounds=1000$roundstoolow$kUMsbe306n21p9R.FRkW3IGn.S9NPN0x50YhH1xhLsPuWGsUSklZt58jaTfF4ZEQpyUNGc0dqbpBYYBaHHrsX.' }
        # checked 2026-09-26 against glibc crypt(3) (Python's crypt module) and openssl passwd -6:
        # UTF-8 bytes of a non-ASCII password - what Fedora's sign-in hashes
        @{ Name = 'non-ASCII password hashes its UTF-8 bytes, as glibc and openssl do'
           Run = { ConvertTo-Sha512Crypt -Password ('p' + [char]0xE4 + 'ssw' + [char]0xF6 + 'rd') -Salt 'abcdefghijklmnop' }
           Expect = '$6$abcdefghijklmnop$Z142AM4CbyHnvFikRKauX.vgsnvjYLvt4bZlZZZrlgVhDW0zltnUun6G9I5xvVirZ/Y9MRz96lJh5eoUicidR.' }
        @{ Name = 'a fresh salt is 16 characters from the crypt alphabet'
           Run = { $s = New-CryptSalt; ($s.Length -eq 16) -and ($s -cmatch '^[./0-9A-Za-z]{16}$') }; Expect = $true }
        @{ Name = 'a fresh hash has the $6$ shape the schema requires'
           Run = { (ConvertTo-Sha512Crypt -Password 'pässwörd' -Salt (New-CryptSalt)) -cmatch '^\$6\$[./0-9A-Za-z]{16}\$[./0-9A-Za-z]{86}$' }; Expect = $true }
        @{ Name = 'refuse: empty'; Run = { Test-PasswordPair '' '' }; Expect = 'the password is empty' }
        @{ Name = 'refuse: two different entries'; Run = { Test-PasswordPair 'abc' 'abd' }; Expect = 'the two entries are not the same' }
        @{ Name = 'refuse: case matters'; Run = { Test-PasswordPair 'Abc' 'abc' }; Expect = 'the two entries are not the same' }
        @{ Name = 'a matching pair is accepted'; Run = { $null -eq (Test-PasswordPair 'correct horse' 'correct horse') }; Expect = $true }
    )
    foreach ($c in $cases) {
        $got = & $c.Run
        if ("$got" -ceq "$($c.Expect)") { Write-Host "    PASS  $($c.Name)" -ForegroundColor Green }
        else { Write-Host "    FAIL  $($c.Name)  (expected '$($c.Expect)', got '$got')" -ForegroundColor Red; $failed++ }
    }
    Write-Host ''
    if ($failed -gt 0) { Write-Host "  $failed check(s) failed" -ForegroundColor Red; exit 1 }
    Write-Host '  all checks passed' -ForegroundColor Green; Write-Host ''
}

if ($SelfTest) { Invoke-SelfTest; return }
if (-not $OutFile) { throw 'give -OutFile <file> (or -SelfTest)' }
if (-not $LinuxName) { throw 'give -LinuxName <the sign-in name> - the person is told which account this password is for (2026-09-26)' }

Write-Host ''
Write-Host "  Your Fedora account:  $LinuxName" -ForegroundColor Cyan
Write-Host "  You sign in to Fedora as $LinuxName with the password you choose now." -ForegroundColor Cyan
Write-Host '  Nothing shows while you type. Write it down if you need to - nothing else stores it.' -ForegroundColor DarkGray
for ($try = 1; $try -le 3; $try++) {
    $a = ConvertFrom-SecurePlain (Read-Host '  Password' -AsSecureString)
    $b = ConvertFrom-SecurePlain (Read-Host '  Type it again' -AsSecureString)
    $why = Test-PasswordPair $a $b
    if (-not $why) {
        $hash = ConvertTo-Sha512Crypt -Password $a -Salt (New-CryptSalt)
        $a = $null; $b = $null
        [IO.File]::WriteAllText($OutFile, "$hash`n", (New-Object Text.UTF8Encoding($false)))
        Write-Host "  Password set for $LinuxName." -ForegroundColor Green
        exit 0
    }
    $a = $null; $b = $null
    Write-Host "  Not set: $why. Try again." -ForegroundColor Yellow
}
Write-Host '  No password was set. Nothing was changed.' -ForegroundColor Red
exit 1
