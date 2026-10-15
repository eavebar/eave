//! What the hand does to a shape, the same for an island and the notch or the edge (D-26, D-45,
//! D-106): a drag gives less and less the further it goes, and stretches the shape most where it
//! was grabbed.

use crate::theme;

/// Moves almost one to one at first, then gives less and less, approaching `limit` without ever
/// reaching it.
pub fn rubber_band(distance: f32, limit: f32) -> f32 {
    if limit <= 0.0 {
        return 0.0;
    }
    let pulled = distance.abs() * theme::DRAG_RESISTANCE;
    (limit * (1.0 - 1.0 / (1.0 + pulled / limit))).copysign(distance)
}

/// How much of a drag stretches a point `distance` from where it was grabbed: all of it there,
/// easing to nothing at `DRAG_REACH`. Flat at both ends, so neither the grab point nor where the
/// stretch fades out shows as a bend.
pub fn near(distance: f32) -> f32 {
    let near = (1.0 - distance / theme::DRAG_REACH).clamp(0.0, 1.0);
    near * near * near * (near * (6.0 * near - 15.0) + 10.0)
}
