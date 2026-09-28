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
