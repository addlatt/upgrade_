//! `upgrade-autoinstall stick-file PROGRAM JOB` prints the stick's own
//! answer file (see the library): what whoever writes the stick puts at
//! its root as `autoinstall.yaml`. The paths are as Ubuntu's live system
//! sees them, under `/cdrom`.
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    match a.first().map(String::as_str) {
        Some("stick-file") if a.len() == 3 => print!("{}", upgrade_autoinstall::stick_file(&a[1], &a[2])),
        _ => {
            eprintln!("usage: upgrade-autoinstall stick-file PROGRAM JOB");
            std::process::exit(2);
        }
    }
}
