//! Font zoom. The terminal font size is the size in `config.toml` plus a zoom
//! offset, in points. The offset is kept in memory by the workspace: it is not
//! saved, and a restart starts at the configured size. No GPUI here, so the
//! math is tested on its own.

/// The smallest and largest font size the zoom allows, in points.
pub const MIN_SIZE: f32 = 6.0;
pub const MAX_SIZE: f32 = 72.0;

/// The font size for a configured `base` size and a zoom `offset`, clamped.
pub fn effective_size(base: f32, offset: f32) -> f32 {
    (base + offset).clamp(MIN_SIZE, MAX_SIZE)
}

/// The offset after one step of `delta` points (`+1.0` or `-1.0`). The step
/// starts from the size on screen, so at a limit a step back shows at once
/// instead of first undoing offset that has no effect.
pub fn stepped_offset(base: f32, offset: f32, delta: f32) -> f32 {
    (effective_size(base, offset) + delta).clamp(MIN_SIZE, MAX_SIZE) - base
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_moves_the_configured_size() {
        assert_eq!(effective_size(14.0, 0.0), 14.0);
        assert_eq!(effective_size(14.0, 2.0), 16.0);
        assert_eq!(effective_size(14.0, -3.0), 11.0);
    }

    #[test]
    fn effective_size_is_clamped_to_6_and_72() {
        assert_eq!(effective_size(14.0, 100.0), 72.0);
        assert_eq!(effective_size(14.0, -100.0), 6.0);
        assert_eq!(effective_size(100.0, 0.0), 72.0);
        assert_eq!(effective_size(2.0, 0.0), 6.0);
    }

    #[test]
    fn a_zero_offset_is_the_configured_size_clamped() {
        assert_eq!(effective_size(14.0, 0.0), 14.0);
        assert_eq!(effective_size(100.0, 0.0), 72.0);
    }

    #[test]
    fn steps_move_one_point() {
        assert_eq!(stepped_offset(14.0, 0.0, 1.0), 1.0);
        assert_eq!(stepped_offset(14.0, 0.0, -1.0), -1.0);
        assert_eq!(stepped_offset(14.0, 1.0, -1.0), 0.0);
    }

    #[test]
    fn fractional_base_sizes_step_by_a_point() {
        assert_eq!(stepped_offset(12.5, 0.0, 1.0), 1.0);
        assert_eq!(effective_size(12.5, 1.0), 13.5);
    }

    #[test]
    fn steps_stop_at_the_limits() {
        // At 72 pt a step up changes nothing, and the offset does not grow past it.
        assert_eq!(stepped_offset(14.0, 58.0, 1.0), 58.0);
        // At 6 pt a step down changes nothing.
        assert_eq!(stepped_offset(14.0, -8.0, -1.0), -8.0);
        // One step down from 72 pt shows at once, at 71 pt.
        assert_eq!(stepped_offset(14.0, 58.0, -1.0), 57.0);
    }

    #[test]
    fn a_new_base_keeps_the_offset() {
        // config.toml goes from 14 to 20 pt while zoomed in by 3 points.
        assert_eq!(effective_size(14.0, 3.0), 17.0);
        assert_eq!(effective_size(20.0, 3.0), 23.0);
        // Near the top the sum is clamped to 72 pt.
        assert_eq!(effective_size(70.0, 3.0), 72.0);
        // The next step down starts from the size on screen, 72 pt.
        assert_eq!(stepped_offset(70.0, 3.0, -1.0), 1.0);
    }
}
