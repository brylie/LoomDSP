use super::*;

/// Unity, half gain, and a tenth of full scale agree in both directions.
#[test]
fn decibels_match_known_gains() {
    for (db, gain) in [(0.0, 1.0), (-6.0206, 0.5), (-20.0, 0.1)] {
        assert!((db_to_gain(db) - gain).abs() < 1e-6);
        assert!((gain_to_db(gain) - db).abs() < 1e-5);
        assert!((gain_to_db_f64(f64::from(gain)) - f64::from(db)).abs() < 1e-6);
    }
    assert_eq!(gain_to_db(0.0), f32::NEG_INFINITY);
    assert_eq!(gain_to_db_f64(0.0), f64::NEG_INFINITY);
}
