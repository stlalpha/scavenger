use crate::models::Profile;

/// Stub: score a listing against a profile. Full implementation in the scoring work unit.
pub fn score_listing(
    _profile: &Profile,
    _title: &str,
    _description: &str,
    _price: Option<f64>,
) -> f64 {
    50.0 // stub: returns a mid-range score
}
