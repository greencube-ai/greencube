use crate::task_outcome::TaskOutcome;

/// Result of the grounded Judge evaluating a TaskOutcome.
pub struct Verdict {
    /// Competence delta: positive = did well, negative = did poorly, 0.0 = no signal.
    pub delta: f64,
    /// Human-readable explanation for debugging/logging.
    pub reason: String,
    /// Which signals contributed to this verdict.
    pub signals_used: Vec<String>,
}

/// Evaluate a completed task using grounded signals only. No LLM calls.
///
/// Rules (first match wins):
/// 1. No tools invoked         → 0.0  (no signal to judge)
/// 2. Zero errors, ≤3 rounds   → +0.15 (clean success)
/// 3. Zero errors, ≤6 rounds   → +0.10 (success, needed rounds)
/// 4. Zero errors, >6 rounds   → +0.05 (success, possible flailing)
/// 5. Errors >50% AND >5 rounds → -0.30 (high errors + flailing)
/// 6. Errors >50%              → -0.25 (high error rate)
/// 7. Errors ≤50%              → -0.10 (some errors)
pub fn judge_task(outcome: &TaskOutcome) -> Verdict {
    // Rule 1: no tools → neutral
    if outcome.tool_call_count == 0 {
        return Verdict {
            delta: 0.0,
            reason: "Pure text response, no tool signals to judge".into(),
            signals_used: vec!["tool_call_count=0".into()],
        };
    }

    let error_rate = outcome.tool_error_count as f64 / outcome.tool_call_count as f64;
    let rounds = outcome.llm_rounds;

    if outcome.tool_error_count == 0 {
        // Rules 2-4: graduated success
        if rounds <= 3 {
            Verdict {
                delta: 0.15,
                reason: format!("Clean success: {} tools, {} rounds, no errors", outcome.tool_call_count, rounds),
                signals_used: vec!["tool_errors=0".into(), format!("llm_rounds={}", rounds)],
            }
        } else if rounds <= 6 {
            Verdict {
                delta: 0.10,
                reason: format!("Success but needed {} rounds", rounds),
                signals_used: vec!["tool_errors=0".into(), format!("llm_rounds={}", rounds)],
            }
        } else {
            Verdict {
                delta: 0.05,
                reason: format!("Success but excessive rounds ({}), possible flailing", rounds),
                signals_used: vec!["tool_errors=0".into(), format!("llm_rounds={}", rounds)],
            }
        }
    } else {
        // Rules 5-7: graduated failure
        let pct = (error_rate * 100.0) as u32;
        if error_rate > 0.5 && rounds > 5 {
            Verdict {
                delta: -0.30,
                reason: format!("High error rate ({}/{}) with flailing ({} rounds)", outcome.tool_error_count, outcome.tool_call_count, rounds),
                signals_used: vec![format!("error_rate={}%", pct), format!("llm_rounds={}", rounds)],
            }
        } else if error_rate > 0.5 {
            Verdict {
                delta: -0.25,
                reason: format!("High error rate: {}/{} tools failed", outcome.tool_error_count, outcome.tool_call_count),
                signals_used: vec![format!("error_rate={}%", pct)],
            }
        } else {
            Verdict {
                delta: -0.10,
                reason: format!("Some tool errors: {}/{} tools failed", outcome.tool_error_count, outcome.tool_call_count),
                signals_used: vec![format!("error_rate={}%", pct)],
            }
        }
    }
}
