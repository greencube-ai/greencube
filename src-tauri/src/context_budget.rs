/// Maximum tokens for all injected context combined (commandments excluded).
pub const MAX_INJECTION_TOKENS: usize = 2000;

/// Rough token estimate. ~4 chars per token for English.
/// TODO v0.7: use tiktoken-rs for accurate token counting.
pub fn estimate_tokens(text: &str) -> usize {
    text.len() / 4
}

/// Trim injections to fit within budget. Returns total estimated tokens.
/// Trim order: patterns first, then feedback, then knowledge (reduce), then context (truncate), then profile.
/// Commandments are NEVER trimmed.
pub fn trim_to_budget(
    profile: &mut String,
    goals: &mut String,
    context: &mut String,
    knowledge: &mut String,
    feedback: &mut String,
    patterns: &mut String,
) -> usize {
    let calc_total = |p: &str, g: &str, c: &str, k: &str, f: &str, pt: &str| {
        estimate_tokens(p) + estimate_tokens(g) + estimate_tokens(c)
            + estimate_tokens(k) + estimate_tokens(f) + estimate_tokens(pt)
    };

    if calc_total(profile, goals, context, knowledge, feedback, patterns) <= MAX_INJECTION_TOKENS {
        return calc_total(profile, goals, context, knowledge, feedback, patterns);
    }

    // 1. Clear patterns
    patterns.clear();
    if calc_total(profile, goals, context, knowledge, feedback, patterns) <= MAX_INJECTION_TOKENS {
        return calc_total(profile, goals, context, knowledge, feedback, patterns);
    }

    // 2. Clear feedback
    feedback.clear();
    if calc_total(profile, goals, context, knowledge, feedback, patterns) <= MAX_INJECTION_TOKENS {
        return calc_total(profile, goals, context, knowledge, feedback, patterns);
    }

    // 3. Trim knowledge (keep first half of lines)
    let lines: Vec<&str> = knowledge.lines().collect();
    let keep = (lines.len() / 2).max(1);
    *knowledge = lines[..keep].join("\n");
    if calc_total(profile, goals, context, knowledge, feedback, patterns) <= MAX_INJECTION_TOKENS {
        return calc_total(profile, goals, context, knowledge, feedback, patterns);
    }

    // 4. Truncate context to 500 chars
    *context = context.chars().take(500).collect();
    if calc_total(profile, goals, context, knowledge, feedback, patterns) <= MAX_INJECTION_TOKENS {
        return calc_total(profile, goals, context, knowledge, feedback, patterns);
    }

    // 5. Truncate profile to 200 chars
    *profile = profile.chars().take(200).collect();

    calc_total(profile, goals, context, knowledge, feedback, patterns)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens("hello world"), 2);
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn test_trim_under_budget() {
        let mut profile = "short".to_string();
        let mut goals = "goal".to_string();
        let mut context = "ctx".to_string();
        let mut knowledge = "know".to_string();
        let mut feedback = "fb".to_string();
        let mut patterns = "pat".to_string();
        trim_to_budget(&mut profile, &mut goals, &mut context, &mut knowledge, &mut feedback, &mut patterns);
        assert_eq!(patterns, "pat"); // Nothing trimmed
    }

    #[test]
    fn test_trim_over_budget() {
        let mut profile = "x".repeat(800);  // 200 tokens
        let mut goals = "x".repeat(400);    // 100 tokens
        let mut context = "x".repeat(4000); // 1000 tokens
        let mut knowledge = "line1\nline2\nline3\nline4\nline5\nline6\nline7\nline8".to_string();
        let mut feedback = "x".repeat(2000);  // 500 tokens
        let mut patterns = "x".repeat(2000);  // 500 tokens
        // Total ~2350 tokens, over 2000 budget
        let total = trim_to_budget(&mut profile, &mut goals, &mut context, &mut knowledge, &mut feedback, &mut patterns);
        assert!(patterns.is_empty()); // Trimmed first
        assert!(total <= MAX_INJECTION_TOKENS + 100); // Close to budget (approximate)
    }
}
