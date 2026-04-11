use serde::{Deserialize, Serialize};

/// Aggregates grounded signals from a completed task.
/// Built incrementally during task execution, finalized in run_post_task.
/// The Judge (self_verify) will consume this in a future change.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskOutcome {
    /// Overall success: true if no tool errors occurred.
    pub success: bool,
    /// Number of tool calls made across all loop iterations.
    pub tool_call_count: u32,
    /// Number of tool calls that returned errors.
    pub tool_error_count: u32,
    /// Number of LLM round-trips (tool-call loop iterations).
    pub llm_rounds: u32,
    /// Wall-clock duration from task start to finish, in milliseconds.
    pub duration_ms: u64,
    /// Estimated cost in cents.
    pub cost_cents: i64,
}

impl TaskOutcome {
    pub fn new() -> Self {
        Self {
            success: true,
            tool_call_count: 0,
            tool_error_count: 0,
            llm_rounds: 0,
            duration_ms: 0,
            cost_cents: 0,
        }
    }

    /// Record a tool call result. Updates counts and sticky error flag.
    pub fn record_tool_call(&mut self, is_error: bool) {
        self.tool_call_count += 1;
        if is_error {
            self.tool_error_count += 1;
            self.success = false;
        }
    }

    /// Record completion of one LLM round-trip.
    pub fn record_llm_round(&mut self) {
        self.llm_rounds += 1;
    }

    /// Finalize with elapsed time and cost.
    pub fn finalize(&mut self, elapsed: std::time::Duration, cost_cents: i64) {
        self.duration_ms = elapsed.as_millis() as u64;
        self.cost_cents = cost_cents;
    }
}
