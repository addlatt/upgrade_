//! The Rust password hasher against Read-Password.ps1 (RISKS R32; VALIDATION
//! V13). `password-cases.json` lists the calls: the 12 cases of the
//! PowerShell self-test under their own names, with the same inputs, plus
//! more (block edges, round edges, salt edges, control characters).
//! `password-golden.json` is what the PowerShell returned for each (written
//! by `password-golden.ps1`). Every return must be equal in full.

use serde_json::{json, Value};
use upgrade_job::password::{new_salt, pair_refusal, sha512_crypt, sha512_crypt_with, CRYPT_ALPHABET};

fn cases() -> Vec<Value> {
    serde_json::from_str(include_str!("password-cases.json")).unwrap()
}

fn is_alphabet(s: &str) -> bool {
    s.bytes().all(|b| CRYPT_ALPHABET.contains(&b))
}

fn run(fn_name: &str, a: &Value) -> Value {
    let text = |k: &str| a.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    match fn_name {
        "ConvertTo-Sha512Crypt" => {
            let rounds = a.get("Rounds").and_then(Value::as_u64).map(|r| r as u32);
            let given = a.get("RoundsGiven").and_then(Value::as_bool).unwrap_or(false);
            match rounds {
                Some(r) => json!(sha512_crypt_with(&text("Password"), &text("Salt"), r, given)),
                None => json!(sha512_crypt(&text("Password"), &text("Salt"), None)),
            }
        }
        "Test-PasswordPair" => json!(pair_refusal(&text("First"), &text("Second"))),
        "salt-shape" => {
            let s = new_salt().unwrap();
            json!(s.len() == 16 && is_alphabet(&s))
        }
        "hash-shape" => {
            let h = sha512_crypt("pässwörd", &new_salt().unwrap(), None);
            let parts: Vec<&str> = h.split('$').collect();
            json!(parts.len() == 4 && parts[0].is_empty() && parts[1] == "6" && parts[2].len() == 16 && is_alphabet(parts[2]) && parts[3].len() == 86 && is_alphabet(parts[3]))
        }
        other => panic!("password-cases.json names a function this test does not know: {other}"),
    }
}

#[test]
fn every_call_matches_powershell() {
    let golden: Value = serde_json::from_str(include_str!("password-golden.json")).unwrap();
    let cases = cases();
    assert_eq!(golden.as_object().unwrap().len(), cases.len(), "password-golden.json is stale: run ./port-check.sh --record");
    let (mut wrong, mut stricter) = (Vec::new(), 0);
    for case in &cases {
        let name = case["name"].as_str().unwrap();
        let want = golden.get(name).unwrap_or_else(|| panic!("password-golden.json has no '{name}': run ./port-check.sh --record"));
        let got = run(case["fn"].as_str().unwrap(), &case["args"]);
        if name == "port: a pair differing only in a combining form is not the same" {
            // Kept on purpose (docs/RUST-PORT.md): PowerShell's -cne compares
            // by the culture's rules and calls "é" and "e" + a combining accent
            // the same, so the script accepts the pair; glibc compares bytes
            // at sign-in, so the Rust refuses it. Stricter. The day the
            // PowerShell refuses too, this exception is stale.
            stricter += 1;
            if !want.is_null() {
                wrong.push(format!("FAIL  {name}\n  the PowerShell now refuses this pair: drop the exception\n  PowerShell: {want}"));
            } else if got != json!("the two entries are not the same") {
                wrong.push(format!("FAIL  {name}\n  the Rust must refuse a pair whose bytes differ\n  Rust: {got}"));
            }
            continue;
        }
        if want != &got {
            wrong.push(format!("FAIL  {name}\n  PowerShell: {want}\n  Rust:       {got}"));
        }
    }
    assert_eq!(stricter, 1, "the combining-form pair is the one difference kept on purpose here");
    assert!(cases.len() >= 33, "{} cases", cases.len());
    assert!(wrong.is_empty(), "differences from Read-Password.ps1:\n\n{}", wrong.join("\n\n"));
}

/// The specification's own vectors (Drepper, SHA-crypt.txt), held directly,
/// without the golden file.
#[test]
fn the_specification_vectors_hold() {
    assert_eq!(sha512_crypt("Hello world!", "saltstring", None), "$6$saltstring$svn8UoSVapNtMuq1ukKS4tPQd8iKwSMHWjl/O817G3uBnIFNjnQJuesI68u4OTLiBFdcbYEdFCoEOfaS35inz1");
    assert_eq!(sha512_crypt("Hello world!", "saltstringsaltstring", Some(10000)), "$6$rounds=10000$saltstringsaltst$OW1/O6BYHV6BcXZu8QVeXbDWra3Oeqh0sbHbbMCVNSnCM/UrjmM0Dp8vOuZeHBy/YTBmSK6H9qs/y3RnOaw5v.");
    assert_eq!(sha512_crypt("This is just a test", "toolongsaltstring", Some(5000)), "$6$rounds=5000$toolongsaltstrin$lQ8jolhgVRVhY4b5pZKaysCLi0QBxGoNeKQzQ3glMhwllF7oGDZxUhx1yxdYcz/e1JSbq3y6JMxxl8audkUEm0");
    assert_eq!(sha512_crypt("a very much longer text to encrypt.  This one even stretches over morethan one line.", "anotherlongsaltstring", Some(1400)), "$6$rounds=1400$anotherlongsalts$POfYwTEok97VWcjxIiSOjiykti.o/pQs.wPvMxQ6Fm7I6IoYN3CmLs66x9t0oSwbtEW7o7UmJEiDwGqd8p4ur1");
    assert_eq!(sha512_crypt("the minimum number is still observed", "roundstoolow", Some(10)), "$6$rounds=1000$roundstoolow$kUMsbe306n21p9R.FRkW3IGn.S9NPN0x50YhH1xhLsPuWGsUSklZt58jaTfF4ZEQpyUNGc0dqbpBYYBaHHrsX.");
    // checked 2026-09-26 against glibc crypt(3) and openssl passwd -6
    assert_eq!(sha512_crypt("pässwörd", "abcdefghijklmnop", None), "$6$abcdefghijklmnop$Z142AM4CbyHnvFikRKauX.vgsnvjYLvt4bZlZZZrlgVhDW0zltnUun6G9I5xvVirZ/Y9MRz96lJh5eoUicidR.");
}

#[test]
fn the_self_test_is_all_here() {
    let n = cases().iter().filter(|c| c["origin"] == "selftest").count();
    assert_eq!(n, 12, "{n} self-test cases in password-cases.json");
}

#[test]
fn two_fresh_salts_differ() {
    assert_ne!(new_salt().unwrap(), new_salt().unwrap());
}
