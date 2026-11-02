//! Which Option key is held, for `option_as_meta = "left"` and `"right"`.
//!
//! GPUI reports Option as one modifier. The device flags of the keyboard state
//! tell the two keys apart.

/// `(left, right)`: whether each Option key is down. Both are `false` on
/// platforms that cannot tell the keys apart.
#[cfg(target_os = "macos")]
pub fn option_keys_down() -> (bool, bool) {
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventSourceFlagsState(state_id: u32) -> u64;
    }
    /// `kCGEventSourceStateCombinedSessionState`: the keyboard state of the session.
    const COMBINED_SESSION_STATE: u32 = 0;
    /// `NX_DEVICELALTKEYMASK` and `NX_DEVICERALTKEYMASK` from `IOKit/hidsystem/IOLLEvent.h`.
    const LEFT_OPTION: u64 = 0x0000_0020;
    const RIGHT_OPTION: u64 = 0x0000_0040;
    // SAFETY: the call takes an integer and returns the flags by value.
    let flags = unsafe { CGEventSourceFlagsState(COMBINED_SESSION_STATE) };
    (flags & LEFT_OPTION != 0, flags & RIGHT_OPTION != 0)
}

#[cfg(not(target_os = "macos"))]
pub fn option_keys_down() -> (bool, bool) {
    (false, false)
}
