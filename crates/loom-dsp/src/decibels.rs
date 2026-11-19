//! Conversions between amplitude gain and decibels.

/// Converts an amplitude in decibels to a linear gain.
#[inline]
pub fn db_to_gain(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

/// Converts a nonnegative linear gain to decibels. Zero gives negative infinity.
#[inline]
pub fn gain_to_db(gain: f32) -> f32 {
    20.0 * gain.log10()
}

/// Converts a measured gain to decibels at the precision of the measurement.
/// Zero gives negative infinity.
#[inline]
pub fn gain_to_db_f64(gain: f64) -> f64 {
    20.0 * gain.log10()
}

#[cfg(test)]
mod tests;
