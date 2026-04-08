use crate::models::{Keyword, Listing, Profile};

fn format_keywords(keywords: &[Keyword]) -> String {
    keywords
        .iter()
        .map(|kw| match kw {
            Keyword::Single(s) => s.clone(),
            Keyword::AnyOf(variants) => variants.join(" or "),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_price_range(price_min: Option<f64>, price_max: Option<f64>) -> String {
    match (price_min, price_max) {
        (Some(min), Some(max)) => format!("${min:.0} \u{2013} ${max:.0}"),
        (None, Some(max)) => format!("up to ${max:.0}"),
        (Some(min), None) => format!("${min:.0} and above"),
        (None, None) => "any price".to_string(),
    }
}

fn format_listing_price(price: Option<f64>) -> String {
    match price {
        Some(p) if p > 0.0 => format!("${p:.2}"),
        _ => "price not listed".to_string(),
    }
}

/// Build (system, user) prompts for single-listing filter evaluation.
pub fn build_prompt(profile: &Profile, listing: &Listing) -> (String, String) {
    let keywords_str = format_keywords(&profile.keywords);
    let neg_str = profile.negative_keywords.join(", ");
    let price_range = format_price_range(profile.price_min, profile.price_max);

    let system = format!(
        r#"You are an expert in {name}. A collector is searching for items matching their profile.
Evaluate the listing and respond ONLY with valid JSON matching this exact schema:
{{"relevant": bool, "reason": "string", "notable": "string or null", "escalate": bool}}

Interest profile:
- Keywords (must match): {keywords_str}
- Negative keywords (any match = not relevant): {neg_str}
- Price range: {price_range}

Rules:
- relevant: false if any negative keyword appears or the item clearly does not match the profile
- reason: 1-2 sentences explaining your decision; shown directly to the user
- notable: only if there is something specific worth highlighting — rare variant, seller misidentification, significant underpricing, unusual condition. Set to null if nothing notable.
- escalate: true only if this is an unusually good opportunity the collector should see immediately"#,
        name = profile.name,
    );

    let price_str = format_listing_price(listing.price);
    let desc = if listing.description.is_empty() {
        "(no description)"
    } else {
        &listing.description
    };
    let user = format!(
        "Title: {title}\nPrice: {price_str}\nDescription: {desc}",
        title = listing.title,
    );

    (system, user)
}

/// Build (system, user) prompts for escalation evaluation.
pub fn build_escalation_prompt(
    profile: &Profile,
    listing: &Listing,
    triggered_keywords: &[String],
) -> (String, String) {
    let keywords_str = format_keywords(&profile.keywords);
    let neg_str = if profile.negative_keywords.is_empty() {
        "none".to_string()
    } else {
        profile.negative_keywords.join(", ")
    };
    let triggered_str = triggered_keywords
        .iter()
        .map(|kw| format!("\"{kw}\""))
        .collect::<Vec<_>>()
        .join(", ");

    let system = format!(
        r#"You are a domain expert and insider in {name} — the kind of person who knows production histories, variant differences, regional market dynamics, and the stories behind specific models. A collector relies on your expertise to spot what others miss.

A listing matched escalation keywords: {triggered_str}.

Respond ONLY with valid JSON:
{{"relevant": bool, "reason": "string", "notable": "string or null", "escalate": bool}}

Interest profile:
- Keywords: {keywords_str}
- Negative keywords: {neg_str}

Your job — go deep:
- IDENTIFY the exact item. Not just "an AS/400" but which model, which generation, what config. If you can tell from the title/description, say so. If there are clues the seller missed, call them out.
- INSIDER KNOWLEDGE: Share what a knowledgeable collector would know — production numbers, years manufactured, what makes one variant more desirable than another, known issues with specific models, which accessories or configs are hard to find.
- MARKET CONTEXT: What does this typically sell for? Is this price good, fair, or inflated? Are prices trending up or down? Is there a specific market (Japan, Europe, niche forums) where this commands a premium?
- CREDIBILITY CHECK: If the seller claims rare/mint/NOS — is that plausible? What would you look for to verify?
- WHAT TO ASK THE SELLER: If this is interesting, what questions should the buyer ask before committing?

Be specific and opinionated. Name actual model numbers, years, specs, and dollar amounts.
- relevant: false only if this clearly doesn't match the profile
- reason: 2-4 sentences of your expert take — this is shown directly to the collector as insider notes
- notable: the single most important thing to know about this listing — could be "this is the rare late-production variant with the improved coating" or "this is the most common model, seller calling it rare is BS" or "at this price this is a steal, they typically go for 2x". null ONLY if genuinely nothing interesting.
- escalate: true if you'd tell a friend "jump on this now before someone else does""#,
        name = profile.name,
    );

    let price_str = format_listing_price(listing.price);
    let desc = if listing.description.is_empty() {
        "(no description)"
    } else {
        &listing.description
    };
    let user = format!(
        "Title: {title}\nPrice: {price_str}\nSource: {source}\nDescription: {desc}",
        title = listing.title,
        source = listing.source_id,
    );

    (system, user)
}

/// Build (system, user) prompts for batch evaluation.
pub fn build_batch_prompt(profile: &Profile, listings: &[Listing]) -> (String, String) {
    let keywords_str = format_keywords(&profile.keywords);
    let neg_str = profile.negative_keywords.join(", ");
    let price_range = format_price_range(profile.price_min, profile.price_max);

    let system = format!(
        r#"You are an expert in {name}. A collector is searching for items matching their profile.
Evaluate EACH listing and respond ONLY with a valid JSON array. Each element must match this schema:
{{"id": "string", "relevant": bool, "reason": "string", "notable": "string or null", "escalate": bool}}

Interest profile:
- Keywords (must match): {keywords_str}
- Negative keywords (any match = not relevant): {neg_str}
- Price range: {price_range}

Rules:
- relevant: false if any negative keyword appears or the item clearly does not match the profile
- reason: 1-2 sentences explaining your decision; shown directly to the user
- notable: only if there is something specific worth highlighting — rare variant, seller misidentification, significant underpricing, unusual condition. Set to null if nothing notable.
- escalate: true only if this is an unusually good opportunity the collector should see immediately
- Return one object per listing in the same order, using the listing's id field"#,
        name = profile.name,
    );

    let mut blocks = Vec::with_capacity(listings.len());
    for listing in listings {
        let price_str = format_listing_price(listing.price);
        let desc = if listing.description.is_empty() {
            "(no description)"
        } else {
            &listing.description
        };
        blocks.push(format!(
            "[{id}] {title} \u{2014} {price_str}\n{desc}",
            id = listing.id,
            title = listing.title,
        ));
    }
    let user = blocks.join("\n\n");

    (system, user)
}
