use crate::models::{KeywordGroup, Listing, Profile};

/// Format keywords as comma-separated, with "or" joining variants in groups.
fn format_keywords(keywords: &[KeywordGroup]) -> String {
    keywords
        .iter()
        .map(|kw| match kw {
            KeywordGroup::Single(s) => s.clone(),
            KeywordGroup::Any(variants) => variants.join(" or "),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Format a price range string from optional min/max.
fn format_price_range(price_min: Option<f64>, price_max: Option<f64>) -> String {
    match (price_min, price_max) {
        (Some(min), Some(max)) => format!("${:.0} \u{2013} ${:.0}", min, max),
        (None, Some(max)) => format!("up to ${:.0}", max),
        (Some(min), None) => format!("${:.0} and above", min),
        (None, None) => "any price".to_string(),
    }
}

/// Format a listing price as "$X.XX" or "price not listed".
fn format_price(price: Option<f64>) -> String {
    match price {
        Some(p) if p != 0.0 => format!("${:.2}", p),
        _ => "price not listed".to_string(),
    }
}

/// Return the description or a fallback placeholder.
fn desc_or_placeholder(description: &str) -> &str {
    if description.is_empty() {
        "(no description)"
    } else {
        description
    }
}

/// Build (system_prompt, user_prompt) for single listing filter evaluation.
pub fn build_prompt(profile: &Profile, listing: &Listing) -> (String, String) {
    let keywords_str = format_keywords(&profile.keywords);
    let price_range = format_price_range(profile.price_min, profile.price_max);
    let negatives = profile.negative_keywords.join(", ");

    let system = format!(
        r#"You are an expert in {name}. A collector is searching for items matching their profile.
Evaluate the listing and respond ONLY with valid JSON matching this exact schema:
{{"relevant": bool, "reason": "string", "notable": "string or null", "escalate": bool}}

Interest profile:
- Keywords (must match): {keywords}
- Negative keywords (any match = not relevant): {negatives}
- Price range: {price_range}

Rules:
- relevant: false if any negative keyword appears or the item clearly does not match the profile
- reason: 1-2 sentences explaining your decision; shown directly to the user
- notable: only if there is something specific worth highlighting — rare variant, seller misidentification, significant underpricing, unusual condition. Set to null if nothing notable.
- escalate: true only if this is an unusually good opportunity the collector should see immediately"#,
        name = profile.name,
        keywords = keywords_str,
        negatives = negatives,
        price_range = price_range,
    );

    let user = format!(
        "Title: {title}\nPrice: {price}\nDescription: {desc}",
        title = listing.title,
        price = format_price(listing.price),
        desc = desc_or_placeholder(&listing.description),
    );

    (system, user)
}

/// Build (system_prompt, user_prompt) for evaluating multiple listings at once.
pub fn build_batch_prompt(profile: &Profile, listings: &[Listing]) -> (String, String) {
    let keywords_str = format_keywords(&profile.keywords);
    let price_range = format_price_range(profile.price_min, profile.price_max);
    let negatives = profile.negative_keywords.join(", ");

    let system = format!(
        r#"You are an expert in {name}. A collector is searching for items matching their profile.
Evaluate EACH listing and respond ONLY with a valid JSON array. Each element must match this schema:
{{"id": "string", "relevant": bool, "reason": "string", "notable": "string or null", "escalate": bool}}

Interest profile:
- Keywords (must match): {keywords}
- Negative keywords (any match = not relevant): {negatives}
- Price range: {price_range}

Rules:
- relevant: false if any negative keyword appears or the item clearly does not match the profile
- reason: 1-2 sentences explaining your decision; shown directly to the user
- notable: only if there is something specific worth highlighting — rare variant, seller misidentification, significant underpricing, unusual condition. Set to null if nothing notable.
- escalate: true only if this is an unusually good opportunity the collector should see immediately
- Return one object per listing in the same order, using the listing's id field"#,
        name = profile.name,
        keywords = keywords_str,
        negatives = negatives,
        price_range = price_range,
    );

    let listing_blocks: Vec<String> = listings
        .iter()
        .map(|l| {
            format!(
                "[{id}] {title} \u{2014} {price}\n{desc}",
                id = l.id,
                title = l.title,
                price = format_price(l.price),
                desc = desc_or_placeholder(&l.description),
            )
        })
        .collect();

    let user = listing_blocks.join("\n\n");

    (system, user)
}

/// Build (system_prompt, user_prompt) for deep escalation analysis.
pub fn build_escalation_prompt(
    profile: &Profile,
    listing: &Listing,
    triggered_keywords: &[String],
) -> (String, String) {
    let keywords_str = format_keywords(&profile.keywords);
    let negatives = if profile.negative_keywords.is_empty() {
        "none".to_string()
    } else {
        profile.negative_keywords.join(", ")
    };
    let triggered_str = triggered_keywords
        .iter()
        .map(|kw| format!("\"{}\"", kw))
        .collect::<Vec<_>>()
        .join(", ");

    let system = format!(
        r#"You are a domain expert and insider in {name} — the kind of person who knows production histories, variant differences, regional market dynamics, and the stories behind specific models. A collector relies on your expertise to spot what others miss.

A listing matched escalation keywords: {triggered}.

Respond ONLY with valid JSON:
{{"relevant": bool, "reason": "string", "notable": "string or null", "escalate": bool}}

Interest profile:
- Keywords: {keywords}
- Negative keywords: {negatives}

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
        triggered = triggered_str,
        keywords = keywords_str,
        negatives = negatives,
    );

    let user = format!(
        "Title: {title}\nPrice: {price}\nSource: {source}\nDescription: {desc}",
        title = listing.title,
        price = format_price(listing.price),
        source = listing.source_id,
        desc = desc_or_placeholder(&listing.description),
    );

    (system, user)
}
