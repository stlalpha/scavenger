use crate::models::{KeywordGroup, Profile};

fn group_matches(text_lower: &str, group: &KeywordGroup) -> bool {
    match group {
        KeywordGroup::Single(term) => text_lower.contains(&term.to_lowercase() as &str),
        KeywordGroup::Any(terms) => {
            terms.iter().any(|t| text_lower.contains(&t.to_lowercase() as &str))
        }
    }
}

/// Score a listing against a profile. Returns 0-100.
///
/// Algorithm:
/// - Negative keywords in combined text -> 0
/// - All keyword groups must match in combined text (AND), else 0
/// - Title hits: 40%, desc extra hits: 20%, price in range: 20%, base: 20%
pub fn score_listing(
    profile: &Profile,
    title: &str,
    description: &str,
    price: Option<f64>,
) -> f64 {
    let combined = format!("{} {}", title, description);
    let combined_lower = combined.to_lowercase();
    let title_lower = title.to_lowercase();
    let desc_lower = description.to_lowercase();

    for neg in &profile.negative_keywords {
        if combined_lower.contains(&neg.to_lowercase() as &str) {
            return 0.0;
        }
    }

    let total = profile.keywords.len();
    if total == 0 {
        return 0.0;
    }

    let combined_hits = profile
        .keywords
        .iter()
        .filter(|kw| group_matches(&combined_lower, kw))
        .count();
    if combined_hits < total {
        return 0.0;
    }

    let title_hits = profile
        .keywords
        .iter()
        .filter(|kw| group_matches(&title_lower, kw))
        .count();
    let desc_hits = profile
        .keywords
        .iter()
        .filter(|kw| group_matches(&desc_lower, kw))
        .count();

    let title_score = (title_hits as f64 / total as f64) * 40.0;
    let extra_desc = desc_hits.saturating_sub(title_hits);
    let desc_score = (extra_desc.min(total) as f64 / total as f64) * 20.0;

    let price_score = match (price, profile.price_min, profile.price_max) {
        (None, _, _) => 20.0,
        (_, None, None) => 20.0,
        (Some(p), min, max) => {
            let above_min = min.is_none_or(|m| p >= m);
            let below_max = max.is_none_or(|m| p <= m);
            if above_min && below_max {
                20.0
            } else {
                0.0
            }
        }
    };

    (title_score + desc_score + price_score + 20.0).min(100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_profile(keywords: Vec<KeywordGroup>, negatives: Vec<String>) -> Profile {
        Profile {
            id: "test".into(),
            name: "test".into(),
            keywords,
            negative_keywords: negatives,
            sources: vec![],
            price_min: None,
            price_max: None,
            poll_interval_sec: 3600,
            alert_priority: crate::models::AlertPriority::Normal,
            enabled: true,
            tags: vec![],
            escalation_keywords: vec![],
            location_radius_mi: None,
        }
    }

    #[test]
    fn all_keywords_match_positive_score() {
        let p = make_profile(
            vec![
                KeywordGroup::Single("guitar".into()),
                KeywordGroup::Single("fender".into()),
            ],
            vec![],
        );
        let score = score_listing(&p, "Fender Stratocaster Guitar", "", None);
        assert!(score > 0.0);
    }

    #[test]
    fn missing_keyword_returns_zero() {
        let p = make_profile(
            vec![
                KeywordGroup::Single("guitar".into()),
                KeywordGroup::Single("fender".into()),
            ],
            vec![],
        );
        let score = score_listing(&p, "Gibson Les Paul Guitar", "", None);
        assert_eq!(score, 0.0);
    }

    #[test]
    fn negative_keyword_returns_zero() {
        let p = make_profile(
            vec![KeywordGroup::Single("guitar".into())],
            vec!["broken".into()],
        );
        let score = score_listing(&p, "Guitar for sale", "broken neck", None);
        assert_eq!(score, 0.0);
    }

    #[test]
    fn price_in_range_gives_bonus() {
        let mut p = make_profile(
            vec![KeywordGroup::Single("guitar".into())],
            vec![],
        );
        p.price_min = Some(100.0);
        p.price_max = Some(500.0);

        let in_range = score_listing(&p, "Guitar", "", Some(200.0));
        let out_range = score_listing(&p, "Guitar", "", Some(1000.0));
        assert!(in_range > out_range);
    }

    #[test]
    fn title_hits_weighted_higher_than_desc() {
        let p = make_profile(
            vec![
                KeywordGroup::Single("guitar".into()),
                KeywordGroup::Single("fender".into()),
            ],
            vec![],
        );
        // Both in title
        let title_score = score_listing(&p, "Fender Guitar", "some description", None);
        // One in title, one only in desc
        let split_score = score_listing(&p, "Guitar for sale", "fender brand", None);
        assert!(title_score > split_score);
    }

    #[test]
    fn or_group_matches_any_variant() {
        let p = make_profile(
            vec![KeywordGroup::Any(vec![
                "stratocaster".into(),
                "strat".into(),
            ])],
            vec![],
        );
        let score = score_listing(&p, "Fender Strat", "", None);
        assert!(score > 0.0);
    }

    #[test]
    fn no_keywords_returns_zero() {
        let p = make_profile(vec![], vec![]);
        let score = score_listing(&p, "anything", "anything", None);
        assert_eq!(score, 0.0);
    }
}
