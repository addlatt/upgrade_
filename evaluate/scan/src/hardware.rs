//! The hardware checks that read only the device list: Wi-Fi, graphics,
//! audio, and maker-specific notes. These are the ones a machine recording
//! (the corpus) can replay.

use crate::check::{Scan, Status};
use crate::data::{status_of, tables};
use crate::facts::{Pnp, Sys};
use crate::parse::pci_id;
use crate::ps::{eq_ci, matches, s, starts_with_ci, truthy};

pub fn wifi(scan: &mut Scan, pnp: &[Pnp]) {
    let t = tables();
    let cards: Vec<&Pnp> = pnp
        .iter()
        .filter(|d| eq_ci(s(&d.pnp_class), "Net") && starts_with_ci(s(&d.device_id), "PCI\\") && matches(r"Wi-?Fi|Wireless|WLAN|802\.11", s(&d.name)))
        .collect();
    if cards.is_empty() {
        scan.add("Hardware", "Wi-Fi", Status::Info, "no wireless card detected")
            .note("No PCI wireless adapter found. If this is a desktop on Ethernet, that is expected.");
        return;
    }
    for c in cards {
        let Some(id) = pci_id(s(&c.device_id)) else { continue };
        let vendor = &id[..4];
        let name = s(&c.name);
        if let Some(e) = t.wifi.get(&id) {
            let status = status_of(&e.status);
            let remedy = if status == Status::Fail {
                "Before you start, get a USB Ethernet adapter or confirm your phone can do USB tethering. Without one you may finish the install with no way to get online."
            } else {
                ""
            };
            scan.add("Hardware", "Wi-Fi", status, format!("{} [{id}] - driver {}", e.name, e.driver)).note(&*e.note).min_kernel(&*e.min_kernel).remedy(remedy);
        } else if let Some(e) = t.wifi_vendor_fallback.get(vendor) {
            scan.unmatched.push(format!("wifi {id} ({name})"));
            scan.add("Hardware", "Wi-Fi", status_of(&e.status), format!("{name} [{id}] - unrecognised {} part", e.vendor)).note(&*e.note);
        } else {
            scan.unmatched.push(format!("wifi {id} ({name})"));
            scan.add("Hardware", "Wi-Fi", Status::Unknown, format!("{name} [{id}]"))
                .note("Unrecognised wireless vendor. Search \"linux <this device id>\" before committing, and have a wired fallback ready.");
        }
    }
}

pub fn gpu(scan: &mut Scan, pnp: &[Pnp]) {
    let t = tables();
    let gpus: Vec<&Pnp> = pnp.iter().filter(|d| eq_ci(s(&d.pnp_class), "Display") && starts_with_ci(s(&d.device_id), "PCI\\")).collect();
    if gpus.is_empty() {
        scan.add("Hardware", "Graphics", Status::Unknown, "none detected");
        return;
    }
    for g in &gpus {
        let Some(id) = pci_id(s(&g.device_id)) else { continue };
        let vendor = &id[..4];
        let name = s(&g.name);
        if let Some(e) = t.gpu.get(&id) {
            scan.add("Hardware", "Graphics", status_of(&e.status), format!("{} [{id}] - driver {}", e.name, e.driver)).note(&*e.note).min_kernel(&*e.min_kernel);
        } else if let Some(e) = t.gpu_vendor_rules.get(vendor) {
            scan.unmatched.push(format!("gpu {id} ({name})"));
            scan.add("Hardware", "Graphics", status_of(&e.status), format!("{name} [{id}] - driver {}", e.driver)).note(&*e.note);
        } else {
            scan.unmatched.push(format!("gpu {id} ({name})"));
            scan.add("Hardware", "Graphics", Status::Unknown, format!("{name} [{id}]")).note("Unrecognised graphics vendor.");
        }
    }
    if gpus.len() > 1 {
        scan.add("Hardware", "Hybrid graphics", Status::Warn, format!("{} GPUs present", gpus.len()))
            .note("This laptop switches between an integrated and a discrete GPU. On Linux that switching works but is manual and less transparent than under Windows - expect to choose which GPU an application uses, and expect worse battery life than Windows if the discrete GPU stays awake.")
            .remedy("Pick a distribution with graphics switching built in - Pop!_OS and Fedora handle this best.");
    }
}

pub fn audio(scan: &mut Scan, pnp: &[Pnp]) {
    let mut found = false;
    for q in &tables().audio_quirks {
        if pnp.iter().any(|d| truthy(&d.device_id) && matches(&q.pattern, s(&d.device_id))) {
            found = true;
            scan.add("Hardware", "Audio", status_of(&q.status), format!("{} detected", q.name))
                .note(&*q.note)
                .min_kernel(&*q.min_kernel)
                .remedy("Test audio in the live USB session before installing. If headphones work but the internal speakers do not, that is this exact issue.");
        }
    }
    if !found {
        scan.add("Hardware", "Audio", Status::Ok, "standard HD Audio - no known smart-amp quirk");
    }
}

pub fn vendor(scan: &mut Scan, sys: &Sys) {
    let subject = format!("{} {}", s(&sys.vendor), s(&sys.model));
    if let Some(q) = tables().vendor_quirks.iter().find(|q| matches(&q.pattern, &subject)) {
        scan.add("Hardware", "Vendor-specific", status_of(&q.status), subject).note(&*q.note);
    }
}
