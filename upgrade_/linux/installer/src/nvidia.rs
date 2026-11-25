//! NVIDIA's own driver, for Ubuntu's installer (RISKS R47).
//!
//! Ubuntu's stick carries NVIDIA's driver in two branches, each with its
//! kernel module already built and signed for the kernel the install
//! brings, and the installer picks the branch for a card by itself when
//! `drivers: {install: true}` is in the answer file (its own tool,
//! `ubuntu-drivers`, matches the card against each package's list).
//!
//! That switch is turned on only when it can do something: this computer
//! has an NVIDIA graphics card, and `data/devices/nvidia.json` (measured
//! from that same ISO's packages, never typed) says the stick carries a
//! driver for it. Any other computer installs exactly as before.
//!
//! Two halves, as everywhere (rule #5): [`cards`] reads the machine,
//! [`plan`] and [`installed`] decide from what was read.

use crate::util::{read_to_string, sorted_entries};
use serde_json::{Value, json};

pub const TABLE: &str = include_str!("../../../../data/devices/nvidia.json");

/// One NVIDIA graphics card, as the kernel lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    /// the PCI device id, `1c03`
    pub device: String,
    /// the driver the running kernel bound to it (`nouveau`, `nvidia`), or empty
    pub driver: String,
}

/// What the early check decided, for the log and the stick's record.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// every NVIDIA card with the branch the stick carries for it, if any
    pub cards: Vec<(Card, Option<String>)>,
    /// why the switch is off although a card is there, or empty
    pub why_not: String,
}

impl Plan {
    /// True when the answer file should ask for the drivers.
    pub fn install(&self) -> bool {
        self.why_not.is_empty() && self.cards.iter().any(|(_, b)| b.is_some())
    }
    pub fn branches(&self) -> Vec<String> {
        let mut b: Vec<String> = self.cards.iter().filter_map(|(_, b)| b.clone()).collect();
        b.sort();
        b.dedup();
        b
    }
    pub fn record(&self) -> Value {
        json!({"install": self.install(), "why_not": self.why_not,
            "cards": self.cards.iter().map(|(c, b)| json!({"id": format!("10de:{}", c.device), "driver_now": c.driver, "branch": b})).collect::<Vec<_>>()})
    }
}

fn hex(text: &str) -> String {
    text.trim().trim_start_matches("0x").to_ascii_lowercase()
}

/// Every display-class PCI device of vendor 10de under `sys`
/// (`/sys/bus/pci/devices`). Class 03 is display: a card that drives a
/// screen (0300) and a laptop's second card that does not (0302) alike.
pub fn cards(sys: &str) -> Vec<Card> {
    sorted_entries(sys)
        .into_iter()
        .filter(|d| hex(&read_to_string(&format!("{sys}/{d}/vendor"))) == "10de" && hex(&read_to_string(&format!("{sys}/{d}/class"))).starts_with("03"))
        .map(|d| Card {
            device: hex(&read_to_string(&format!("{sys}/{d}/device"))),
            driver: std::fs::read_link(format!("{sys}/{d}/driver")).ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_default(),
        })
        .collect()
}

/// The newest branch in the table that lists the card and has its module
/// built and signed for the installed kernel. A branch without one would
/// be built on the computer, and under Secure Boot that needs a key step
/// nobody is there for.
pub fn branch_for(table: &Value, device: &str) -> Option<String> {
    table["Branches"]
        .as_array()?
        .iter()
        .filter(|b| !b["SignedModuleFor"].as_str().unwrap_or("").is_empty() && b["Ids"].as_str().unwrap_or("").split(' ').any(|i| i == device))
        .filter_map(|b| b["Branch"].as_str())
        .max_by_key(|b| b.parse::<u32>().unwrap_or(0))
        .map(str::to_string)
}

/// `release` is what the job installs, as the table names releases
/// (`ubuntu-26.04.1`). A table measured from another release says nothing
/// about this stick, so the switch stays off.
pub fn plan(table: &Value, release: &str, cards: &[Card]) -> Plan {
    let measured = table["Measured"]["Release"].as_str().unwrap_or("");
    let why_not = if cards.is_empty() || measured == release { String::new() } else { format!("the driver table was measured from '{measured}', and this stick installs '{release}'") };
    Plan { cards: cards.iter().map(|c| (c.clone(), branch_for(table, &c.device))).collect(), why_not }
}

/// After the install: is each branch the plan expected in the new system?
/// `status` is the new system's `/var/lib/dpkg/status`. Per branch: the
/// driver package, and the kernel module package for every kernel there.
pub fn installed(status: &str, branches: &[String]) -> Value {
    let ok: Vec<&str> = status.split("\n\n").filter(|p| p.lines().any(|l| l == "Status: install ok installed")).filter_map(|p| p.lines().find_map(|l| l.strip_prefix("Package: "))).collect();
    let all: Vec<Value> = branches
        .iter()
        .map(|b| {
            let modules: Vec<&str> = ok.iter().copied().filter(|p| p.starts_with(&format!("linux-modules-nvidia-{b}-"))).collect();
            json!({"branch": b, "driver_package": ok.contains(&format!("nvidia-driver-{b}").as_str()), "module_packages": modules})
        })
        .collect();
    let pass = !all.is_empty() && all.iter().all(|b| b["driver_package"] == true && b["module_packages"].as_array().is_some_and(|m| !m.is_empty()));
    json!({"result": if branches.is_empty() { "not-asked" } else if pass { "installed" } else { "missing" }, "branches": all})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Value {
        serde_json::from_str(TABLE).unwrap()
    }
    fn card(device: &str) -> Card {
        Card { device: device.into(), driver: "nouveau".into() }
    }

    #[test]
    fn the_table_is_the_one_measured_for_the_ubuntu_the_kit_carries() {
        let t = table();
        assert_eq!(t["Measured"]["Release"], "ubuntu-26.04.1");
        // a GTX 1060 and the Aspire's MX150 are in 580 only; an RTX 4060 is in both, and gets the newer
        assert_eq!(branch_for(&t, "1c03").as_deref(), Some("580"));
        assert_eq!(branch_for(&t, "1d10").as_deref(), Some("580"));
        assert_eq!(branch_for(&t, "28e0").as_deref(), Some("595"));
        // a GTX 650: no branch on the stick lists it
        assert_eq!(branch_for(&t, "0fc6"), None);
    }

    #[test]
    fn the_switch_is_on_only_for_a_card_the_stick_carries_a_driver_for() {
        let t = table();
        assert!(!plan(&t, "ubuntu-26.04.1", &[]).install(), "no NVIDIA card: as before");
        let p = plan(&t, "ubuntu-26.04.1", &[card("1c03")]);
        assert!(p.install());
        assert_eq!(p.branches(), ["580"]);
        assert!(!plan(&t, "ubuntu-26.04.1", &[card("0fc6")]).install(), "a card no branch lists");
        let other = plan(&t, "ubuntu-26.04.2", &[card("1c03")]);
        assert!(!other.install(), "a stick the table was not measured from");
        assert!(other.why_not.contains("ubuntu-26.04.2"));
        assert_eq!(p.record()["cards"][0], json!({"id": "10de:1c03", "driver_now": "nouveau", "branch": "580"}));
    }

    #[test]
    fn a_branch_with_no_signed_module_does_not_count() {
        let t = json!({"Measured": {"Release": "u"}, "Branches": [
            {"Branch": "580", "SignedModuleFor": "7.0.0-30-generic", "Ids": "1c03"},
            {"Branch": "595", "SignedModuleFor": "", "Ids": "1c03 28e0"}]});
        assert_eq!(branch_for(&t, "1c03").as_deref(), Some("580"));
        assert_eq!(branch_for(&t, "28e0"), None);
    }

    #[test]
    fn cards_are_read_from_the_kernels_list() {
        let d = std::env::temp_dir().join(format!("upg-nvidia-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        for (slot, vendor, device, class) in [("0000:00:02.0", "0x8086", "0x5917", "0x030000"), ("0000:01:00.0", "0x10de", "0x1d10", "0x030200"), ("0000:01:00.1", "0x10de", "0x0fb9", "0x040300")] {
            let p = d.join(slot);
            std::fs::create_dir_all(&p).unwrap();
            std::fs::write(p.join("vendor"), format!("{vendor}\n")).unwrap();
            std::fs::write(p.join("device"), format!("{device}\n")).unwrap();
            std::fs::write(p.join("class"), format!("{class}\n")).unwrap();
        }
        std::os::unix::fs::symlink("../../bus/pci/drivers/nouveau", d.join("0000:01:00.0/driver")).unwrap();
        // the Intel card and NVIDIA's sound device are not NVIDIA graphics cards
        assert_eq!(cards(d.to_str().unwrap()), [card("1d10")]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn after_the_install_the_packages_are_looked_for() {
        let status = "Package: nvidia-driver-580\nStatus: install ok installed\nVersion: 1\n\nPackage: linux-modules-nvidia-580-7.0.0-30-generic\nStatus: install ok installed\n\nPackage: nvidia-driver-595\nStatus: deinstall ok config-files\n";
        assert_eq!(installed(status, &["580".into()])["result"], "installed");
        let r = installed(status, &["595".into()]);
        assert_eq!(r["result"], "missing");
        assert_eq!(r["branches"][0]["driver_package"], false);
        assert_eq!(installed(status, &[])["result"], "not-asked");
    }
}
