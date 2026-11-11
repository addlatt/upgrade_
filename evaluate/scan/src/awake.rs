//! Keep the computer awake while a program works (decided 2026-10-05, the
//! owner; RISKS R33, roadmap item 16). Windows puts an idle computer to
//! sleep, and a conversion left alone is idle by its measure: the Aspire
//! slept in the middle of a run.
//!
//! `SetThreadExecutionState` is Windows' own "this thread still needs the
//! system" call. The request belongs to the thread that made it and ends
//! when that thread ends, so nothing has to give it back: a program that
//! crashes or is stopped leaves the computer free to sleep again. Each
//! program calls `hold()` once, on the thread that lives as long as its
//! work does.
//!
//! What it cannot do: a closed lid, the power button and a flat battery
//! follow the power plan, not this request. The window says so.

/// "Until I say otherwise" (not just a reset of the idle clock).
pub const ES_CONTINUOUS: u32 = 0x8000_0000;
/// The system is needed: no idle sleep.
pub const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;
/// The screen is needed. On a Modern Standby computer the screen going dark
/// is what starts the sleep, so the screen is held too.
pub const ES_DISPLAY_REQUIRED: u32 = 0x0000_0002;

/// What `hold()` asks for.
pub const HOLD: u32 = ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED;

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn SetThreadExecutionState(flags: u32) -> u32;
}

/// Ask Windows to stay awake, with the screen on, for as long as the calling
/// thread lives. True when Windows took the request. Elsewhere it does
/// nothing and says false.
pub fn hold() -> bool {
    #[cfg(windows)]
    {
        // SAFETY: the call takes a flag word and has no other preconditions; 0 is its failure.
        unsafe { SetThreadExecutionState(HOLD) != 0 }
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Give the request back early, on the thread that made it.
pub fn release() -> bool {
    #[cfg(windows)]
    {
        // SAFETY: as above.
        unsafe { SetThreadExecutionState(ES_CONTINUOUS) != 0 }
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_request_is_continuous_and_names_the_system_and_the_screen() {
        // 0x80000003: the one click test's own lock was 0x80000001 (no screen)
        assert_eq!(HOLD, 0x8000_0003);
    }
}
