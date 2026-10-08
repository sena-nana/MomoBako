//! The size that decides where theme intent gets resolved.
//!
//! `docs/reference/theme.md` §1.5 resolves sizing intent when a node's style is
//! *written*, not when it is read, and resolves colour the other way round.
//! The only reason for the split is that a colour is 16 bytes and a
//! `LayoutStyle` is three orders of magnitude larger: putting the box on the
//! read path meant copying it per control per frame, which measured +196% on
//! a palette switch.
//!
//! So the rationale rests on a number. If `LayoutStyle` ever becomes small,
//! the split stops being justified and should be revisited rather than
//! inherited. This test is what makes that a decision instead of an accident.

/// Measured 4808 bytes on 2026-09-19. The bound is deliberately loose: the
/// exact figure moves with field layout and is not the point. "Too big to
/// copy on a hot path" is the point.
const TOO_BIG_TO_COPY_PER_FRAME: usize = 1024;

#[test]
fn layout_style_is_still_too_big_to_resolve_on_the_read_path() {
    let layout = size_of::<nana_ui_core::LayoutStyle>();
    assert!(
        layout > TOO_BIG_TO_COPY_PER_FRAME,
        "LayoutStyle is now {layout} bytes. It was 4808 when theme.md \u{00a7}1.5 \
         chose to resolve sizing intent on write instead of on read, purely to \
         avoid copying it per control per frame. If it is genuinely small now, \
         revisit that choice and this test; do not just widen the bound."
    );
    assert!(
        size_of::<nana_ui_runtime::ComputedStyle>() < layout,
        "colour resolution rides on ComputedStyle and is cheap to redo \
         downstream precisely because it is the smaller of the two"
    );
}
