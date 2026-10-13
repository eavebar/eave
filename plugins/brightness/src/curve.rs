//! The perceptual curve between a backlight's raw level and the percent shown ([D-80](https://github.com/eaveshell/eave/blob/main/docs/decisions.md)).
//!
//! A backlight's raw level is linear in light, but the eye isn't: 1 → 10 % of the light looks
//! like a far bigger jump than 60 → 100 %. Squaring the percent gives even steps to the eye, fine
//! ones at the dark end.

const EXPONENT: f64 = 2.0;

/// The percent a raw level shows as.
pub fn percent(raw: u32, max: u32) -> u32 {
    if max == 0 {
        return 0;
    }
    let fraction = (f64::from(raw.min(max)) / f64::from(max)).powf(EXPONENT.recip());
    (fraction * 100.0).round() as u32
}

/// The raw level for a percent. Any percent above zero gives some light: the lowest raw level is
/// 1, since 0 turns some panels off.
pub fn raw(percent: u32, max: u32) -> u32 {
    if percent == 0 {
        return 0;
    }
    let fraction = (f64::from(percent.min(100)) / 100.0).powf(EXPONENT);
    ((fraction * f64::from(max)).round() as u32).clamp(1.min(max), max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ends_map_to_the_ends() {
        for max in [1, 15, 255, 500, 1023, 96_000] {
            assert_eq!(raw(0, max), 0);
            assert_eq!(raw(100, max), max);
            assert_eq!(percent(0, max), 0);
            assert_eq!(percent(max, max), 100);
        }
        assert_eq!(percent(0, 0), 0);
    }

    #[test]
    fn any_percent_gives_some_light() {
        for max in [15, 255, 500] {
            assert!((1..=100).all(|p| raw(p, max) >= 1));
        }
    }

    #[test]
    fn it_rises_and_is_finer_in_the_dark() {
        let max = 500;
        assert!((1..100).all(|p| raw(p, max) <= raw(p + 1, max)));
        assert!(raw(20, max) - raw(10, max) < raw(90, max) - raw(80, max));
        assert_eq!(
            raw(50, max),
            125,
            "half as bright looks like a quarter of the light"
        );
    }

    #[test]
    fn a_fine_backlight_shows_every_percent_it_was_set_to() {
        let max = 96_000;
        assert!((0..=100).all(|p| percent(raw(p, max), max) == p));
    }
}
