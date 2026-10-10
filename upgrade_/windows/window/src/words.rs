//! Every word the window shows, in one place. DRAFTS (2026-09-27): the owner
//! has not approved them yet. Plain words, no em dashes (CLAUDE.md, "How we
//! write"). Changing a sentence here changes nothing the scripts decide.

pub const TITLE: &str = "upgrade_";

pub const WELCOME_HEADING: &str = "Test this computer with Linux";
pub const WELCOME_LEAD: &str = "This checks that Linux will work on this computer. Nothing is installed, and nothing on this computer's drive is changed.";
pub const WELCOME_STEPS_HEADING: &str = "What happens";
pub const WELCOME_STEPS: [&str; 4] = [
    "It looks at this computer's hardware and drives. This takes a few minutes.",
    "It restarts the computer from this USB stick.",
    "Linux checks the screen, the Wi-Fi and the copy of Linux on the stick, writes what it found onto the stick, and restarts back into Windows by itself.",
    "When you sign in again, this window comes back and shows the result.",
];
pub const WELCOME_STICK: &str = "Leave the USB stick plugged in the whole time.";
pub const WELCOME_BITLOCKER: &str = "If this computer uses BitLocker, it is paused for one restart, so Windows starts again without asking for its recovery key.";
pub const START: &str = "Start the test";
pub const CLOSE: &str = "Close";

pub const RUNNING_HEADING: &str = "Testing this computer";
pub const RUNNING_CANCEL: &str = "Nothing has been changed yet. To stop, close this window.";
pub const RUNNING_ARMING: &str = "Setting up the restart. This takes a few seconds, and the window cannot be closed until it is done.";
pub const DETAILS: &str = "Details";

/// The step names, in order. The fifth is the only one that touches this
/// computer's start-up settings.
pub const STEPS: [&str; 5] = [
    "Check the USB stick",
    "Look at this computer",
    "Write the plan for this computer",
    "Prepare the instructions for Linux",
    "Set up the one-time restart onto the stick",
];

pub const KIT_HEADING: &str = "This USB stick is not complete";
pub const KIT_LEAD: &str = "These files should be on the stick and are missing:";
pub const KIT_FIX: &str = "Nothing was changed. Make the stick again, then try again.";

pub const RED_HEADING: &str = "This computer cannot be converted as it is";
pub const STOPPED_HEADING: &str = "The test stopped";
pub const NOTHING_CHANGED: &str = "Nothing was changed on this computer.";
pub const LOG_WHERE: &str = "Everything the test printed is in upgrade_\\convert.log on the USB stick.";

pub const RESTARTING_HEADING: &str = "Restarting in about 20 seconds";
pub const RESTARTING_LINES: [&str; 3] = [
    "Leave the USB stick in.",
    "The computer starts Linux from the stick, checks it, and comes back to Windows by itself. This takes a few minutes.",
    "Sign in as usual. This window opens again and shows the result.",
];

pub const BACK_HEADING: &str = "Welcome back";
pub const BACK_WAITING: &str = "Checking how the restart went...";
pub const BACK_POPUP: &str = "A small box may ask whether anyone had to press a key during the restart. Please answer it: it is part of the test.";
pub const BACK_TIMED_OUT: &str = "The check after the restart did not finish. Nothing on this computer's drive was changed. Leave the USB stick in and restart once more, or look in upgrade_\\convert.log on the stick.";

pub const RESULT_RAN: &str = "Linux ran on this computer";
pub const RESULT_REFUSED: &str = "Linux started, but stopped before it had checked everything";
pub const RESULT_DID_NOT_RUN: &str = "Linux did not start";
pub const RESULT_NO_STICK: &str = "The USB stick could not be found";
pub const RESULT_NO_STICK_LINE: &str = "Plug the stick back in and open UPGRADE.exe on it to see the result again.";
pub const RESULT_DID_NOT_RUN_LINE: &str = "The computer came back to Windows without starting from the USB stick.";
pub const RESULT_FOOT: &str = "Nothing was installed, and nothing on this computer's drive was changed. You can unplug the USB stick now.";

pub const ROW_RIGHT_COMPUTER: &str = "The stick recognised this computer";
pub const ROW_SCREEN: &str = "Screen";
pub const ROW_WIFI: &str = "Wi-Fi";
pub const ROW_SOUND: &str = "Sound firmware";
pub const ROW_IMAGE: &str = "The copy of Linux on the stick";
pub const ROW_RESTART: &str = "The one-time restart onto the stick";

pub const OK: &str = "works";
pub const PROBLEM: &str = "problem found";
pub const NOT_CHECKED: &str = "not checked";

pub const HANDOFF_FIRED: &str = "worked, and the computer came back to Windows";
pub const HANDOFF_IGNORED: &str = "the computer did not use it; nothing was changed";
pub const HANDOFF_PERSISTED: &str = "the computer kept the setting after using it; it has been removed now";
pub const HANDOFF_REORDERED: &str = "the computer changed its start-up order; the one-time entry has been removed";

pub const NO_WINDOW: &str = "The upgrade_ window could not open on this computer (its graphics could not draw it).\n\nNothing was changed. Double-click RUN-VERIFY.cmd on the USB stick instead: it does the same test in a text window.";

// ---------------------------------------------------------------- the chooser

pub const CHOOSE_HEADING: &str = "What would you like to do?";
pub const CHOOSE_VERIFY: &str = "Test this computer with Linux";
pub const CHOOSE_VERIFY_LINE: &str = "Nothing is installed and nothing on this computer's drive is changed.";
pub const CHOOSE_CONVERT: &str = "Convert this computer: keep Windows, install Linux beside it";
pub const CHOOSE_CONVERT_LINE: &str = "This changes the internal drive. You type one word before anything changes.";
pub const CHOOSE_CONVERT_ACK: &str = "Convert this computer, ACCEPTING DATA LOSS";
pub const CHOOSE_CONVERT_ACK_LINE: &str = "For a computer the scanner refused because its drive is failing. Read the next screen first.";

// ---------------------------------------------------------------- convert: the words of RUN-CONVERT.cmd (2026-10-07)

pub const CONVERT_HEADING: &str = "Convert this computer";
pub const CONVERT_LEAD: &str = "This is the converter. It changes the internal drive: it may run Windows' own disk check (with a restart), it shrinks the Windows partition to make room, and it restarts into the Linux installer from this stick. Windows is kept and stays bootable from the boot menu until you choose to reclaim it later, in Linux.";
pub const CONVERT_ORDER: [&str; 6] = [
    "It looks at this computer (nothing is changed).",
    "You choose what the computer shows when it starts, and the password for your Linux account.",
    "It writes the plan for this computer and the instructions for Linux. It refuses on a RED scan, legacy BIOS, an unknown BitLocker state or an unmapped locale.",
    "It asks you to type CONVERT.",
    "The prologue: it re-checks the plan against this computer, runs the disk check if Windows flagged C: (restart), re-measures the room, shrinks C:, pauses BitLocker for one restart, sets up the one-time restart onto the stick and restarts.",
    "Every refusal happens before anything is changed. A restart in the middle is normal: leave the stick in and walk away, it continues by itself before anyone signs in. If it stops, a window says so at your next sign-in.",
];
pub const CONVERT_WIFI: &str = "Your saved Wi-Fi networks and their passwords are copied onto this stick, so Fedora can connect to them on its own. They are removed from the stick at the end of the install and from Fedora once it has set them up.";

pub const ACK_HEADING: &str = "Read this first";
pub const ACK_LEAD: &str = "The scanner refuses computers whose drive is failing or whose Windows volume needs a repair, because converting them can silently lose files. This path lets you go ahead anyway.";
pub const ACK_BEFORE: [&str; 3] = [
    "copy every file you care about OFF this computer, now;",
    "assume anything still on it may be gone afterwards;",
    "know that a failing drive can stop working at any point, including in the middle of the conversion.",
];
pub const ACK_BEFORE_HEADING: &str = "Before you type anything:";
pub const ACK_TYPE: &str = "Type the following sentence exactly:";
pub const ACK_BANNER: &str = "DATA LOSS ACCEPTED";

pub const DESKTOP_HEADING: &str = "What this computer shows when it starts";
pub const DESKTOP_KDE: &str = "KDE Plasma desktop";
pub const DESKTOP_KDE_LINE: &str = "Looks and works most like Windows: a taskbar along the bottom, a start menu in the corner, windows you drag, snap and minimise. The easiest choice if you are used to Windows.";
pub const DESKTOP_GNOME: &str = "GNOME desktop";
pub const DESKTOP_GNOME_LINE: &str = "Simpler and calmer: one bar along the top, and a single button that shows all your open windows and apps at once. Fewer settings to think about, but it works a little differently from Windows, so expect a short getting-used-to.";
pub const DESKTOP_CONSOLE: &str = "Text console only";
pub const DESKTOP_CONSOLE_LINE: &str = "No desktop: a black screen where you type commands. Only for people who already use Linux. The KDE desktop is still installed and can be switched on later.";
pub const DESKTOP_UNSURE: &str = "Not sure? Choose KDE Plasma.";

pub const PASSWORD_HEADING: &str = "Your Linux account and its password";
pub const PASSWORD_LEAD: &str = "You sign in to Linux with the password you choose now. Write it down if you need to: nothing else stores it.";
pub const PASSWORD_LABEL: &str = "Password";
pub const PASSWORD_AGAIN: &str = "Type it again";
pub const PASSWORD_NOT_SET: &str = "Not set";

pub const CONVERT_CONTINUE: &str = "Continue";
pub const CONVERT_STEPS: [&str; 5] = [
    "Check the USB stick",
    "Look at this computer",
    "Write the plan for this computer and the instructions for Linux",
    "Your decision",
    "The prologue: the disk check, the shrink, the one-time restart",
];
pub const CONVERT_RUNNING_HEADING: &str = "Converting this computer";
pub const CONVERT_RUNNING_PROLOGUE: &str = "The prologue is running. It stops by itself if anything is not as it should be, and nothing is changed until it says so. The window cannot be closed now.";

pub const SIGN_IN_HEADING: &str = "Your Fedora sign-in";
pub const SIGN_IN_USER: &str = "user";
pub const SIGN_IN_PASSWORD: &str = "password: the one you just chose";
pub const DECIDE_HEADING: &str = "Your decision";
pub const DECIDE_LINES: [&str; 3] = [
    "This will change the internal disk of this computer: Windows' disk check may run (with a restart), the Windows partition will be shrunk, and Linux will be installed beside it. Windows stays bootable from the boot menu until you reclaim it later. If the disk check runs it may be slow: do not switch the computer off while it runs. If Windows has an update waiting to finish, the computer restarts first to let it finish, then carries on by itself.",
    "If Windows' restore points are what stops the partition shrinking, they will be deleted. Restore points are Windows' own undo history for system changes, not your files, and deleting them cannot be undone. The same goes for Windows' change journal, its running list of which files changed recently: if it is what stops the shrinking, it is deleted and started again empty. Your files are not touched, but search and sync programs will look through them again afterwards.",
    "Nothing else is deleted before Linux is installed.",
];
pub const DECIDE_TYPE: &str = "Type CONVERT (in capitals) to continue. Anything else stops.";
pub const DECIDE_NOT_CONFIRMED: &str = "Not confirmed. Nothing was changed.";

pub const CONVERT_RESTARTING_HEADING: &str = "Restarting into the installer in about 15 seconds";
pub const CONVERT_RESTARTING_LINES: [&str; 3] = [
    "Leave the USB stick in.",
    "Windows is still here and still bootable; it stays that way until you reclaim it in Linux.",
    "You can walk away. The conversion continues by itself; if it stops, a window says so at your next sign-in, and the record is in upgrade_\\outcome.json on the stick.",
];
pub const CONVERT_STOPPED_HEADING: &str = "The conversion stopped";
pub const CONVERT_STOPPED_FOOT: &str = "Windows is as it was. The record is in upgrade_\\outcome.json on this stick.";
pub const LINUX_NAME_UNKNOWN: &str = "The Linux account name could not be worked out. Nothing was changed.";

// ---------------------------------------------------------------- erase: the words of RUN-ERASE-AND-INSTALL.cmd (2026-10-08)

pub const CHOOSE_ERASE: &str = "Erase this computer and install Fedora";
pub const CHOOSE_ERASE_LINE: &str = "Everything on this computer is deleted; nothing is kept. You type one sentence before anything changes.";
pub const CHOOSE_ERASE_ACK: &str = "Erase this computer and install Fedora, ACCEPTING DATA LOSS";
pub const ERASE_HEADING: &str = "Erase this computer and install Fedora";
pub const ERASE_READ_FIRST: &str = "Read this first";
pub const ERASE_LINES: [&str; 5] = [
    "This deletes EVERYTHING on this computer's drives: Windows, every program, every setting and every file. Nothing is kept and nothing is copied anywhere. Fedora Linux is installed in its place.",
    "Before you type anything, copy every file you want to keep OFF this computer.",
    "Nothing changes until the very end: the computer restarts into the installer, and a 2-minute countdown appears on the screen. Press any key during the countdown to cancel: Windows comes back untouched. When the countdown ends, the drives are erased. You can walk away.",
    "If you change your mind later, you can put Windows back, but it will be a new, empty Windows: nothing on this computer today comes back. On many older computers that means Windows 10, which no longer gets free security updates.",
    "Your saved Wi-Fi networks and their passwords are copied onto this stick, so Fedora can connect to them on its own. They are removed from the stick at the end of the install and from Fedora once it has set them up.",
];
pub const ERASE_TYPE: &str = "To erase everything on this computer and install Fedora, type the following sentence exactly. Anything else stops here.";
pub const ERASE_STEPS: [&str; 5] = [
    "Check the USB stick",
    "Look at this computer",
    "Write the plan: it names every drive that will be erased",
    "Your decision",
    "The prologue: the one-time restart into the installer",
];
pub const ERASE_RUNNING_HEADING: &str = "Preparing to erase this computer";
pub const ERASE_DECIDE_HEADING: &str = "Restarting into the installer";
pub const ERASE_DECIDE_LINES: [&str; 2] = [
    "Leave the USB stick in. After the restart a 2-minute countdown appears: press any key during it to cancel and come back to Windows, untouched.",
    "If Windows has an update waiting, the computer restarts first to let it finish, then carries on by itself.",
];
pub const ERASE_GO: &str = "Restart into the installer";
pub const ERASE_RESTARTING_HEADING: &str = "Restarting into the installer in about 15 seconds";
pub const ERASE_RESTARTING_LINES: [&str; 2] = [
    "Leave the USB stick in. The 2-minute countdown comes first: press any key during it to cancel and come back to Windows, untouched.",
    "When the countdown ends, the drives are erased and Fedora is installed. You can walk away.",
];

// ---------------------------------------------------------------- roll back (ROLLBACK.cmd)

pub const CHOOSE_ROLLBACK: &str = "Roll back: Windows first again";
pub const CHOOSE_ROLLBACK_LINE: &str = "For a computer converted with Windows kept. Puts Windows Boot Manager first and its fallback boot file back. Deletes nothing.";
pub const ROLLBACK_HEADING: &str = "Roll back to Windows first";
pub const ROLLBACK_LINES: [&str; 2] = [
    "For a computer converted with Windows kept. It puts Windows Boot Manager first in the firmware's boot order and puts Windows' own fallback boot file back from the copy this stick took before the conversion.",
    "It deletes NOTHING: Linux stays on the disk and in the firmware's boot menu; its space is only returned when you ask for that separately.",
];
pub const ROLLBACK_NO_SNAPSHOT: &str = "This stick holds no copy of the boot files (upgrade_\\esp-snapshot): it was not the stick this computer was converted with. Nothing was changed.";
pub const ROLLBACK_TYPE: &str = "Type ROLLBACK (in capitals) to continue. Anything else stops.";
pub const ROLLBACK_RUNNING: &str = "Rolling back";
pub const ROLLBACK_DONE_HEADING: &str = "Done";
pub const ROLLBACK_DONE_LINE: &str = "Restart the computer; it boots Windows directly.";
pub const ROLLBACK_FAILED_HEADING: &str = "The rollback did not complete";

// ---------------------------------------------------------------- the walk-away probe (RUN-PROBE.cmd)

pub const CHOOSE_PROBE: &str = "Walk-away probe (read-only, one restart)";
pub const CHOOSE_PROBE_LINE: &str = "Tests that the conversion can continue after a restart with nobody signed in. Nothing on the disk is changed.";
pub const PROBE_HEADING: &str = "Walk-away probe";
pub const PROBE_LINES: [&str; 3] = [
    "This tests one thing: that the conversion can continue after a restart with NOBODY signed in. It registers the same startup task the conversion uses, restarts, and on the way back records who ran it, whether anyone was signed in, and how long this USB stick took to appear. Then it removes the task. Nothing on the disk is changed.",
    "When Windows comes back, DO NOT SIGN IN for two minutes. Leave it at the sign-in screen with the stick in. Then sign in as usual: a window shows the result, and the row is on this stick (upgrade_\\walkaway-probe.csv). Never edit that file.",
    "This restarts the computer once.",
];
pub const PROBE_GO: &str = "Restart now";
pub const PROBE_RUNNING: &str = "Setting up the probe";
pub const PROBE_RESTARTING_HEADING: &str = "Restarting in about 15 seconds";
pub const PROBE_RESTARTING_LINE: &str = "Leave the stick in. When Windows comes back, do not sign in for two minutes.";

// ---------------------------------------------------------------- cancel (CANCEL-CONVERSION.cmd)

pub const CHOOSE_CANCEL: &str = "Cancel a conversion in progress";
pub const CHOOSE_CANCEL_LINE: &str = "For a computer back in Windows with a conversion still marked as in progress. Nothing is erased; Windows stays as it is.";
pub const CANCEL_HEADING: &str = "Cancel the conversion";
pub const CANCEL_LINES: [&str; 3] = [
    "This is the safe direction: it only undoes what the prologue did on the Windows side. It removes the one-time boot entry to the stick, turns BitLocker protection back on if it was paused, puts the pagefile and hibernation back, deletes the Wi-Fi passwords from the stick and moves the state aside. Nothing is erased.",
    "A shrink already made stays (Disk Management can extend C: again).",
    "Before it changes anything, it copies the firmware's boot list and the prologue's state onto this stick.",
];
pub const CANCEL_GO: &str = "Cancel the conversion";
pub const CANCEL_RUNNING: &str = "Cancelling";
pub const CANCEL_DONE_HEADING: &str = "The conversion is cancelled";
pub const CANCEL_DONE_LINE: &str = "Windows is as it is. The record of what was undone is in upgrade_\\convert.log on this stick.";
pub const CANCEL_FAILED_HEADING: &str = "The cancel did not complete";
