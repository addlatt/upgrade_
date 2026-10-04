//! The checks about the computer itself: processor, memory, firmware, the
//! walk-away resume, and which Windows it runs.

use crate::check::{Scan, Status};
use crate::facts::{ResumeFacts, Sys};
use crate::ps::{eq_ci, n, s, truthy};

pub fn architecture(scan: &mut Scan, sys: &Sys) {
    let cpu = s(&sys.cpu_name);
    // Architecture 9 = x64, 12 = ARM64, 0 = x86
    match sys.cpu_arch {
        Some(12) => {
            scan.add("Fundamentals", "CPU architecture", Status::Fail, format!("{cpu} (ARM64)"))
                .note("This is an ARM-based Windows machine. Mainstream Linux distributions do not support these laptops - firmware, GPU and power management support is incomplete to nonexistent. This is the one case where the answer is simply no.")
                .remedy("Do not attempt a conversion on this machine.");
        }
        Some(0) => {
            scan.add("Fundamentals", "CPU architecture", Status::Warn, format!("{cpu} (32-bit)"))
                .note("32-bit only CPU. Most distributions dropped 32-bit support years ago. Debian and a few lightweight distributions still work.")
                .remedy("Use Debian 32-bit, antiX or Q4OS.");
        }
        _ => {
            scan.add("Fundamentals", "CPU architecture", Status::Ok, format!("{cpu} (64-bit, {} cores)", n(sys.cpu_cores)));
        }
    }
}

pub fn memory(scan: &mut Scan, sys: &Sys) {
    let shown = sys.ram_gb.map(|x| x.to_string()).unwrap_or_default();
    // an unread amount compares as less than 3, as it does in PowerShell
    if sys.ram_gb.is_none_or(|x| x < 3.0) {
        scan.add("Fundamentals", "Memory", Status::Warn, format!("{shown} GB"))
            .note("Under 4 GB. A mainstream desktop will feel slow. This machine will run Linux noticeably better than it runs Windows, but pick a lightweight desktop.")
            .remedy("Choose Xubuntu, Linux Mint Xfce or Lubuntu rather than a GNOME/KDE default.");
        return;
    }
    scan.add("Fundamentals", "Memory", Status::Ok, format!("{shown} GB"));
}

/// `secure_boot`: 1 on, 0 off, nothing = could not read.
pub fn firmware(scan: &mut Scan, sys: &Sys, secure_boot: Option<i64>) {
    let mode = if truthy(&sys.firmware) { s(&sys.firmware) } else { "unknown" };
    scan.add("Fundamentals", "Firmware mode", Status::Ok, mode);
    match secure_boot {
        Some(1) => {
            scan.add("Fundamentals", "Secure Boot", Status::Ok, "enabled")
                .note("Ubuntu, Fedora, Linux Mint, Debian and openSUSE all boot with Secure Boot on. Smaller distributions may not, and the proprietary NVIDIA driver needs an extra enrolment step.")
                .remedy("Leave it on unless your chosen distribution refuses to boot.");
        }
        Some(0) => {
            scan.add("Fundamentals", "Secure Boot", Status::Ok, "disabled");
        }
        _ => {
            scan.add("Fundamentals", "Secure Boot", Status::Info, "could not determine");
        }
    }
}

/// Info or warn only, never fail: the prologue refuses for real before its
/// restart (RISKS R24). This is the earlier word.
pub fn resume(scan: &mut Scan, facts: Option<&ResumeFacts>) {
    const T: &str = "Walk-away resume";
    let Some(f) = facts.filter(|f| f.schedule_service.is_some() || f.domain_joined.is_some()) else {
        scan.add("Fundamentals", T, Status::Info, "could not read the Task Scheduler state");
        return;
    };
    if truthy(&f.schedule_service) && !eq_ci(s(&f.schedule_service), "Running") {
        scan.add("Fundamentals", T, Status::Warn, format!("Task Scheduler service is {}", s(&f.schedule_service)))
            .note("The conversion continues after its restarts through a startup task. With the service stopped it would wait at the sign-in screen.")
            .remedy("Set the Task Scheduler service to Automatic and start it.");
        return;
    }
    if f.task_creation_policy == Some(0) {
        scan.add("Fundamentals", T, Status::Warn, "a policy prohibits creating scheduled tasks")
            .note("The conversion continues after its restarts through a startup task; this policy blocks registering it. The converter refuses before the restart, not after.")
            .remedy("This is usually a managed (work or school) device. Convert a personally owned one, or have the policy lifted.");
        return;
    }
    let mut managed = Vec::new();
    if f.domain_joined == Some(true) {
        managed.push("domain-joined");
    }
    if f.azure_ad_joined == Some(true) {
        managed.push("Entra-joined");
    }
    if truthy(&f.mdm) {
        managed.push("MDM-enrolled");
    }
    if !managed.is_empty() {
        scan.add("Fundamentals", T, Status::Info, format!("managed device: {}", managed.join(", ")))
            .note("The unattended resume has only been tested on personally owned machines; management policy can remove the startup task.");
        return;
    }
    scan.add("Fundamentals", T, Status::Ok, "a startup task can be registered; not a managed device");
}

pub fn current_os(scan: &mut Scan, sys: &Sys) {
    let detail = format!("{} (build {})", s(&sys.os_caption), n(sys.os_build));
    if sys.os_build.is_some_and(|b| b >= 22000) {
        scan.add("Context", "Current OS", Status::Info, detail);
        return;
    }
    scan.add("Context", "Current OS", Status::Info, detail)
        .note("Windows 10 stopped receiving security updates in October 2025. This machine is not getting patched any more, which is the reason most people are reading this report.");
}
