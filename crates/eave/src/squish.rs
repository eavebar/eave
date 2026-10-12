//! How squished the items being drawn are, so they can squish along one axis at a time.
//!
//! The renderer only scales the same both ways: iced_tiny_skia reads one scale factor, and draws
//! text at a font size. So an island can't squish its items from outside. It sets the squish
//! around drawing them instead, and each thing the host draws (a symbol icon, an app icon, a
//! label, a counter) squishes itself about its own center, where it can draw stretched: in a
//! canvas frame, or into stretched image bounds.
//!
//! It's ambient, a thread-local, because it changes every frame and only while drawing, and it has
//! to reach leaves inside alternates, buttons and gulps. Widgets have no way to hand something
//! down the tree per draw. Drawing happens on the one UI thread.

use std::cell::Cell;

use iced::widget::canvas::Frame;
use iced::{Point, Rectangle, Size, Vector};

/// How much something is stretched across and up and down: 1 is as it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Squish {
    pub x: f32,
    pub y: f32,
}

impl Squish {
    pub const NONE: Self = Self { x: 1.0, y: 1.0 };

    /// Too little to see, so it's drawn the plain way.
    pub fn is_none(self) -> bool {
        (self.x - 1.0).abs() < 0.002 && (self.y - 1.0).abs() < 0.002
    }

    /// Squishes what a canvas frame draws from now on, about `center` in the frame.
    pub fn apply(self, frame: &mut Frame, center: Point) {
        let center = Vector::new(center.x, center.y);
        frame.translate(center);
        frame.scale_nonuniform(Vector::new(self.x, self.y));
        frame.translate(-center);
    }

    /// `bounds`, squished about their center.
    pub fn rect(self, bounds: Rectangle) -> Rectangle {
        let size = Size::new(bounds.width * self.x, bounds.height * self.y);
        let center = bounds.center();
        Rectangle::new(
            Point::new(center.x - size.width / 2.0, center.y - size.height / 2.0),
            size,
        )
    }
}

thread_local! {
    static CURRENT: Cell<Squish> = const { Cell::new(Squish::NONE) };
}

/// How squished what's being drawn right now is.
pub fn current() -> Squish {
    CURRENT.with(Cell::get)
}

/// Runs `draw` with everything it draws squished by `squish`, then puts back what was there.
pub fn with<R>(squish: Squish, draw: impl FnOnce() -> R) -> R {
    struct Restore(Squish);
    impl Drop for Restore {
        fn drop(&mut self) {
            CURRENT.with(|current| current.set(self.0));
        }
    }
    let _restore = Restore(CURRENT.with(|current| current.replace(squish)));
    draw()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_sets_it_and_puts_it_back() {
        assert_eq!(current(), Squish::NONE);
        let wide = Squish { x: 1.1, y: 0.9 };
        let tall = Squish { x: 0.9, y: 1.1 };
        with(wide, || {
            assert_eq!(current(), wide);
            with(tall, || assert_eq!(current(), tall));
            assert_eq!(current(), wide);
        });
        assert_eq!(current(), Squish::NONE);
    }

    #[test]
    fn rect_squishes_about_the_center() {
        let bounds = Rectangle::new(Point::new(10.0, 20.0), Size::new(20.0, 10.0));
        let squished = Squish { x: 1.5, y: 0.5 }.rect(bounds);
        assert_eq!(squished.center(), bounds.center());
        assert_eq!(squished.size(), Size::new(30.0, 5.0));
        assert!(Squish::NONE.is_none());
        assert!(!Squish { x: 1.01, y: 1.0 }.is_none());
    }
}
