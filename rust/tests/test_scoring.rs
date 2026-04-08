use scavenger::models::{KeywordGroup, Profile};
use scavenger::scoring::score_listing;

fn make_profile(keywords: Vec<KeywordGroup>, negatives: Vec<String>) -> Profile {
    Profile {
        id: "test".into(),
        name: "test".into(),
        keywords,
        negative_keywords: negatives,
        sources: vec![],
        price_min: None,
        price_max: None,
    }
}

#[test]
fn all_keywords_match_gives_positive_score() {
    let p = make_profile(
        vec![
            KeywordGroup::Single("guitar".into()),
            KeywordGroup::Single("fender".into()),
        ],
        vec![],
    );
    let score = score_listing(&p, "Fender Stratocaster Guitar", "", None);
    assert!(score > 0.0, "expected positive score, got {}", score);
}

#[test]
fn missing_keyword_gives_zero() {
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
fn negative_keyword_gives_zero() {
    let p = make_profile(
        vec![KeywordGroup::Single("guitar".into())],
        vec!["broken".into()],
    );
    let score = score_listing(&p, "Guitar for sale", "slightly broken", None);
    assert_eq!(score, 0.0);
}

#[test]
fn price_in_range_gives_bonus() {
    let mut p = make_profile(vec![KeywordGroup::Single("widget".into())], vec![]);
    p.price_min = Some(10.0);
    p.price_max = Some(100.0);

    let in_range = score_listing(&p, "Widget", "", Some(50.0));
    let out_of_range = score_listing(&p, "Widget", "", Some(200.0));
    assert!(
        in_range > out_of_range,
        "in_range ({}) should exceed out_of_range ({})",
        in_range,
        out_of_range
    );
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
    let both_title = score_listing(&p, "Fender Guitar", "unrelated text", None);
    let split = score_listing(&p, "Guitar listing", "fender brand", None);
    assert!(
        both_title > split,
        "title score ({}) should exceed split score ({})",
        both_title,
        split
    );
}

#[test]
fn or_group_any_variant_matches() {
    let p = make_profile(
        vec![KeywordGroup::Variants(vec![
            "stratocaster".into(),
            "strat".into(),
        ])],
        vec![],
    );
    let score = score_listing(&p, "Fender Strat for sale", "", None);
    assert!(score > 0.0);
}

#[test]
fn empty_keywords_gives_zero() {
    let p = make_profile(vec![], vec![]);
    assert_eq!(score_listing(&p, "anything", "anything", None), 0.0);
}
