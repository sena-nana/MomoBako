use crate::split_pane::SplitAxis;

pub fn split_ratio_portions(ratio: f32) -> (u16, u16) {
    let first = (ratio.clamp(0.05, 0.95) * 1_000.0).round() as u16;
    (first.max(1), 1_000_u16.saturating_sub(first).max(1))
}

/// Backend-neutral first/second fill portions for a ratio pane split.
pub fn ratio_pane_split(axis: SplitAxis, ratio: f32) -> (SplitAxis, u16, u16) {
    let (first, second) = split_ratio_portions(ratio);
    (axis, first, second)
}

#[cfg(test)]
mod tests {
    use super::split_ratio_portions;

    #[test]
    fn split_ratio_keeps_nonzero_portions() {
        assert_eq!(split_ratio_portions(0.6), (600, 400));
        assert_eq!(split_ratio_portions(0.0), (50, 950));
        assert_eq!(split_ratio_portions(1.0), (950, 50));
    }
}
