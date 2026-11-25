//! Which driver an NVIDIA graphics card is running on (RISKS R47).
//!
//! Read at first start, from the kernel's own device list, so it is the
//! same on every distribution: Ubuntu's installer may have put NVIDIA's
//! own driver in from the stick, Fedora's does not yet. Nothing is
//! installed or changed here. The person is told which one it is, in plain
//! words, and only when the computer has an NVIDIA card at all.
//!
//! Two halves (rule #5): [`cards`] reads, [`judge`] decides.

use serde_json::{json, Value};

/// `(device id, bound driver or empty)` for every display-class PCI device
/// of vendor 10de under `root`'s `/sys/bus/pci/devices`. Class 03 is
/// display: a card that drives a screen (0300) and a laptop's second card
/// that does not (0302) alike.
pub fn cards(root: &str) -> Vec<(String, String)> {
    let sys = format!("{}/sys/bus/pci/devices", root.trim_end_matches('/'));
    let hex = |name: &str, d: &str| std::fs::read_to_string(format!("{sys}/{d}/{name}")).unwrap_or_default().trim().trim_start_matches("0x").to_ascii_lowercase();
    let mut slots: Vec<String> = std::fs::read_dir(&sys).map(|r| r.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
    slots.sort();
    slots
        .into_iter()
        .filter(|d| hex("vendor", d) == "10de" && hex("class", d).starts_with("03"))
        .map(|d| {
            let driver = std::fs::read_link(format!("{sys}/{d}/driver")).ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_default();
            (hex("device", &d), driver)
        })
        .collect()
}

/// `nvidia` only when every NVIDIA card is on NVIDIA's own driver. One on
/// the built-in driver, or on none, is `built-in`: the more cautious word.
pub fn judge(cards: &[(String, String)]) -> Value {
    let list: Vec<Value> = cards.iter().map(|(id, driver)| json!({ "id": format!("10de:{id}"), "driver": driver })).collect();
    let result = if cards.is_empty() {
        "no-nvidia-card"
    } else if cards.iter().all(|(_, d)| d == "nvidia") {
        "nvidia"
    } else {
        "built-in"
    };
    json!({ "result": result, "cards": list })
}

pub fn run(root: &str) -> Value {
    judge(&cards(root))
}

/// The lines for the person (2026-11-25; words are a draft for the owner).
/// Nothing for a computer with no NVIDIA card.
pub fn lines(g: &Value) -> Option<Vec<String>> {
    match g["result"].as_str() {
        Some("nvidia") => Some(vec!["Your NVIDIA graphics card is running on NVIDIA's own driver. Nothing to do.".to_string()]),
        Some("built-in") => Some(vec![
            "Your NVIDIA graphics card is running on the driver built into Linux. The desktop, video and the web work.".to_string(),
            "Games and 3D programs will be slow: NVIDIA's own driver is not installed.".to_string(),
        ]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(id: &str, driver: &str) -> (String, String) {
        (id.to_string(), driver.to_string())
    }

    #[test]
    fn only_every_card_on_nvidias_driver_is_nvidia() {
        assert_eq!(judge(&[])["result"], "no-nvidia-card");
        assert_eq!(judge(&[c("1c03", "nvidia")])["result"], "nvidia");
        assert_eq!(judge(&[c("1c03", "nouveau")])["result"], "built-in");
        assert_eq!(judge(&[c("1c03", "")])["result"], "built-in", "no driver at all is not NVIDIA's");
        assert_eq!(judge(&[c("1c03", "nvidia"), c("1d10", "nouveau")])["result"], "built-in");
        assert_eq!(judge(&[c("1c03", "nouveau")])["cards"][0], json!({ "id": "10de:1c03", "driver": "nouveau" }));
    }

    #[test]
    fn nothing_is_said_without_an_nvidia_card() {
        assert!(lines(&judge(&[])).is_none());
        assert!(lines(&Value::Null).is_none(), "a summary from before this check");
        assert!(lines(&judge(&[c("1c03", "nvidia")])).unwrap()[0].contains("NVIDIA's own driver"));
        assert!(lines(&judge(&[c("1c03", "nouveau")])).unwrap()[1].contains("not installed"));
    }

    #[test]
    fn cards_come_from_the_kernels_list() {
        let root = std::env::temp_dir().join(format!("upg-graphics-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let sys = root.join("sys/bus/pci/devices");
        for (slot, vendor, device, class) in [("0000:00:02.0", "0x8086", "0x5917", "0x030000"), ("0000:01:00.0", "0x10de", "0x1d10", "0x030200"), ("0000:01:00.1", "0x10de", "0x0fb9", "0x040300")] {
            let p = sys.join(slot);
            std::fs::create_dir_all(&p).unwrap();
            std::fs::write(p.join("vendor"), format!("{vendor}\n")).unwrap();
            std::fs::write(p.join("device"), format!("{device}\n")).unwrap();
            std::fs::write(p.join("class"), format!("{class}\n")).unwrap();
        }
        std::os::unix::fs::symlink("../../bus/pci/drivers/nvidia", sys.join("0000:01:00.0/driver")).unwrap();
        // the Intel card and NVIDIA's sound device are not NVIDIA graphics cards
        assert_eq!(cards(root.to_str().unwrap()), [c("1d10", "nvidia")]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
