//! Enter, leave and move animations for the parts of a view whose shape
//! follows data (Vue `<Transition>` / `<TransitionGroup>`).
//!
//! ```ignore
//! when(open, panel).transition(Transition::fade(Duration::from_millis(150)));
//! each(items, |t| t.id, row).transition(Transition::slide(0.0, 8.0, ms(180)).moves(ms(200)));
//! ```
//!
//! A new row or branch plays its enter from the given opacity and transform
//! to its own. A removed one stays where it was, out of hit testing and
//! focus, plays its leave, and is despawned when the leave finishes; its
//! scope is disposed at once, so it no longer follows data. With `moves`,
//! rows that change place slide from where they were (FLIP, on the
//! compositor), including when a leaving row is finally removed.
//!
//! Everything runs on the compositor track of the node: logical style is
//! never written. A transition with no enter or no leave skips that half.

use std::time::Duration;

use nana_ui_core::PaintTransform;

use crate::Easing;

/// One half of a transition: where an entering node starts, or where a
/// leaving one ends, relative to its own opacity and transform.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Presence {
    pub(crate) opacity: Option<f32>,
    pub(crate) transform: Option<PaintTransform>,
    pub(crate) duration: Duration,
    pub(crate) easing: Easing,
}

impl Presence {
    pub fn new(duration: Duration) -> Self {
        Self {
            opacity: None,
            transform: None,
            duration,
            easing: Easing::EaseOutCubic,
        }
    }

    pub fn opacity(mut self, opacity: f32) -> Self {
        self.opacity = Some(opacity);
        self
    }

    pub fn transform(mut self, transform: PaintTransform) -> Self {
        self.transform = Some(transform);
        self
    }

    /// Offset by `(dx, dy)` logical pixels.
    pub fn translate(self, dx: f32, dy: f32) -> Self {
        self.transform(PaintTransform {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: dx,
            f: dy,
        })
    }

    /// Scaled by `scale` around the node's transform origin.
    pub fn scale(self, scale: f32) -> Self {
        self.transform(PaintTransform {
            a: scale,
            b: 0.0,
            c: 0.0,
            d: scale,
            e: 0.0,
            f: 0.0,
        })
    }

    pub fn ease(mut self, easing: Easing) -> Self {
        self.easing = easing;
        self
    }

    pub(crate) fn plays(&self) -> bool {
        !self.duration.is_zero() && (self.opacity.is_some() || self.transform.is_some())
    }
}

/// One property that animates when a binding changes it (CSS
/// `transition: opacity 200ms ease`). See [`super::El::animate`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Implicit {
    pub property: crate::AnimatableProperty,
    pub duration: Duration,
    pub easing: Easing,
}

impl Implicit {
    pub const fn new(property: crate::AnimatableProperty, duration: Duration) -> Self {
        Self {
            property,
            duration,
            easing: Easing::EaseOutCubic,
        }
    }

    pub const fn ease(mut self, easing: Easing) -> Self {
        self.easing = easing;
        self
    }
}

/// How rows and branches enter, leave and move. See the module docs.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Transition {
    pub(crate) enter: Option<Presence>,
    pub(crate) leave: Option<Presence>,
    pub(crate) moves: Option<(Duration, Easing)>,
}

impl Transition {
    /// Nothing animates until a half is set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Fade in from transparent and out to transparent.
    pub fn fade(duration: Duration) -> Self {
        let half = Presence::new(duration).opacity(0.0);
        Self {
            enter: Some(half),
            leave: Some(half),
            moves: None,
        }
    }

    /// Fade and slide in from `(dx, dy)` away, and back out the same way.
    pub fn slide(dx: f32, dy: f32, duration: Duration) -> Self {
        let half = Presence::new(duration).opacity(0.0).translate(dx, dy);
        Self {
            enter: Some(half),
            leave: Some(half),
            moves: None,
        }
    }

    /// Fade and grow from `scale`, and shrink back out.
    pub fn scale(scale: f32, duration: Duration) -> Self {
        let half = Presence::new(duration).opacity(0.0).scale(scale);
        Self {
            enter: Some(half),
            leave: Some(half),
            moves: None,
        }
    }

    pub fn enter(mut self, enter: Presence) -> Self {
        self.enter = Some(enter);
        self
    }

    pub fn leave(mut self, leave: Presence) -> Self {
        self.leave = Some(leave);
        self
    }

    /// No enter animation: new rows appear at once.
    pub fn without_enter(mut self) -> Self {
        self.enter = None;
        self
    }

    /// No leave animation: removed rows go at once.
    pub fn without_leave(mut self) -> Self {
        self.leave = None;
        self
    }

    /// Rows that change place slide there over `duration` (Vue
    /// `<TransitionGroup>` move).
    pub fn moves(mut self, duration: Duration) -> Self {
        self.moves = Some((duration, Easing::EaseOutCubic));
        self
    }

    /// The easing of every half set so far and of moves.
    pub fn ease(mut self, easing: Easing) -> Self {
        for half in [&mut self.enter, &mut self.leave].into_iter().flatten() {
            half.easing = easing;
        }
        if let Some((_, move_easing)) = &mut self.moves {
            *move_easing = easing;
        }
        self
    }
}
