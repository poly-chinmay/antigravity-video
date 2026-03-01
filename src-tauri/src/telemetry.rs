// src-tauri/src/telemetry.rs
//! Execution Telemetry & Budget Tracking
//!
//! Tracks timing for all phases of AI edit execution.
//! Warns if total execution exceeds budget (100ms).

use std::time::{Duration, Instant};

/// Budget threshold - warn if total execution exceeds this
pub const EXECUTION_BUDGET_MS: u64 = 100;

/// Telemetry data for a single AI edit execution
#[derive(Debug, Clone)]
pub struct ExecutionTelemetry {
    /// Time spent calling LLM
    pub ai_latency_ms: u64,
    /// Time spent parsing LLM response
    pub parse_time_ms: u64,
    /// Time spent validating EditPlan
    pub validation_time_ms: u64,
    /// Time spent executing actions
    pub execution_time_ms: u64,
    /// Time spent enforcing invariants
    pub invariant_time_ms: u64,
    /// Time spent serializing state
    pub serialization_time_ms: u64,
    /// Time spent in WAL/persistence
    pub persistence_time_ms: u64,
    /// Total wall-clock time
    pub total_time_ms: u64,
    /// Whether the operation succeeded
    pub success: bool,
    /// Number of actions executed
    pub action_count: usize,
}

impl ExecutionTelemetry {
    /// Create a new telemetry instance
    pub fn new() -> Self {
        Self {
            ai_latency_ms: 0,
            parse_time_ms: 0,
            validation_time_ms: 0,
            execution_time_ms: 0,
            invariant_time_ms: 0,
            serialization_time_ms: 0,
            persistence_time_ms: 0,
            total_time_ms: 0,
            success: false,
            action_count: 0,
        }
    }

    /// Calculate total backend time (excluding AI latency)
    pub fn backend_time_ms(&self) -> u64 {
        self.parse_time_ms
            + self.validation_time_ms
            + self.execution_time_ms
            + self.invariant_time_ms
            + self.serialization_time_ms
            + self.persistence_time_ms
    }

    /// Check if execution exceeded budget
    pub fn exceeded_budget(&self) -> bool {
        self.backend_time_ms() > EXECUTION_BUDGET_MS
    }

    /// Log telemetry summary
    pub fn log(&self) {
        let status = if self.success { "✅" } else { "❌" };

        println!(
            "{} [Telemetry] Total: {}ms (AI: {}ms, Backend: {}ms)",
            status,
            self.total_time_ms,
            self.ai_latency_ms,
            self.backend_time_ms()
        );

        println!(
            "   📊 Parse: {}ms | Validate: {}ms | Execute: {}ms | Invariants: {}ms | Persist: {}ms",
            self.parse_time_ms,
            self.validation_time_ms,
            self.execution_time_ms,
            self.invariant_time_ms,
            self.persistence_time_ms
        );

        if self.exceeded_budget() {
            eprintln!(
                "   ⚠️ [BUDGET EXCEEDED] Backend took {}ms (budget: {}ms)",
                self.backend_time_ms(),
                EXECUTION_BUDGET_MS
            );
        }
    }

    /// Generate JSON for artifact logging
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "ai_latency_ms": self.ai_latency_ms,
            "parse_time_ms": self.parse_time_ms,
            "validation_time_ms": self.validation_time_ms,
            "execution_time_ms": self.execution_time_ms,
            "invariant_time_ms": self.invariant_time_ms,
            "serialization_time_ms": self.serialization_time_ms,
            "persistence_time_ms": self.persistence_time_ms,
            "total_time_ms": self.total_time_ms,
            "backend_time_ms": self.backend_time_ms(),
            "exceeded_budget": self.exceeded_budget(),
            "success": self.success,
            "action_count": self.action_count
        })
    }
}

impl Default for ExecutionTelemetry {
    fn default() -> Self {
        Self::new()
    }
}

/// Timer utility for measuring execution phases
#[derive(Debug)]
pub struct Timer {
    start: Instant,
    name: String,
}

impl Timer {
    /// Start a new timer with a name
    pub fn start(name: &str) -> Self {
        Self {
            start: Instant::now(),
            name: name.to_string(),
        }
    }

    /// Stop the timer and return elapsed milliseconds
    pub fn stop(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    /// Stop the timer with debug logging
    pub fn stop_with_log(&self) -> u64 {
        let elapsed = self.stop();
        println!("   ⏱️ [Timer] {}: {}ms", self.name, elapsed);
        elapsed
    }
}

/// Wrapper to time a closure and return (result, duration_ms)
pub fn timed<F, T>(f: F) -> (T, u64)
where
    F: FnOnce() -> T,
{
    let start = Instant::now();
    let result = f();
    let duration_ms = start.elapsed().as_millis() as u64;
    (result, duration_ms)
}

/// Wrapper to time a fallible closure and return (result, duration_ms)
pub fn timed_result<F, T, E>(f: F) -> (Result<T, E>, u64)
where
    F: FnOnce() -> Result<T, E>,
{
    let start = Instant::now();
    let result = f();
    let duration_ms = start.elapsed().as_millis() as u64;
    (result, duration_ms)
}

/// Aggregate telemetry for reporting
#[derive(Debug, Default)]
pub struct TelemetryAggregator {
    /// Total executions
    pub total_executions: u64,
    /// Successful executions
    pub successful_executions: u64,
    /// Total AI latency across all executions
    pub total_ai_latency_ms: u64,
    /// Total backend time across all executions
    pub total_backend_time_ms: u64,
    /// Number of executions that exceeded budget
    pub budget_exceeded_count: u64,
    /// Maximum backend time seen
    pub max_backend_time_ms: u64,
}

impl TelemetryAggregator {
    /// Create a new aggregator
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a telemetry entry
    pub fn record(&mut self, telemetry: &ExecutionTelemetry) {
        self.total_executions += 1;
        if telemetry.success {
            self.successful_executions += 1;
        }
        self.total_ai_latency_ms += telemetry.ai_latency_ms;
        self.total_backend_time_ms += telemetry.backend_time_ms();
        if telemetry.exceeded_budget() {
            self.budget_exceeded_count += 1;
        }
        self.max_backend_time_ms = self.max_backend_time_ms.max(telemetry.backend_time_ms());
    }

    /// Get average AI latency
    pub fn avg_ai_latency_ms(&self) -> u64 {
        if self.total_executions == 0 {
            0
        } else {
            self.total_ai_latency_ms / self.total_executions
        }
    }

    /// Get average backend time
    pub fn avg_backend_time_ms(&self) -> u64 {
        if self.total_executions == 0 {
            0
        } else {
            self.total_backend_time_ms / self.total_executions
        }
    }

    /// Log aggregate summary
    pub fn log_summary(&self) {
        println!("📈 [Telemetry Summary]");
        println!(
            "   Total: {} | Success: {} ({:.1}%)",
            self.total_executions,
            self.successful_executions,
            if self.total_executions > 0 {
                (self.successful_executions as f64 / self.total_executions as f64) * 100.0
            } else {
                0.0
            }
        );
        println!(
            "   Avg AI: {}ms | Avg Backend: {}ms | Max Backend: {}ms",
            self.avg_ai_latency_ms(),
            self.avg_backend_time_ms(),
            self.max_backend_time_ms
        );
        println!(
            "   Budget Exceeded: {} ({:.1}%)",
            self.budget_exceeded_count,
            if self.total_executions > 0 {
                (self.budget_exceeded_count as f64 / self.total_executions as f64) * 100.0
            } else {
                0.0
            }
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execution_telemetry() {
        let mut telemetry = ExecutionTelemetry::new();
        telemetry.ai_latency_ms = 500;
        telemetry.parse_time_ms = 10;
        telemetry.validation_time_ms = 5;
        telemetry.execution_time_ms = 20;
        telemetry.invariant_time_ms = 3;
        telemetry.persistence_time_ms = 15;
        telemetry.success = true;

        assert_eq!(telemetry.backend_time_ms(), 53);
        assert!(!telemetry.exceeded_budget());
    }

    #[test]
    fn test_budget_exceeded() {
        let mut telemetry = ExecutionTelemetry::new();
        telemetry.execution_time_ms = 150; // Exceeds 100ms budget

        assert!(telemetry.exceeded_budget());
    }

    #[test]
    fn test_timer() {
        let timer = Timer::start("test");
        std::thread::sleep(std::time::Duration::from_millis(10));
        let elapsed = timer.stop();
        assert!(elapsed >= 10);
    }

    #[test]
    fn test_timed() {
        let (result, duration) = timed(|| {
            std::thread::sleep(std::time::Duration::from_millis(10));
            42
        });
        assert_eq!(result, 42);
        assert!(duration >= 10);
    }

    #[test]
    fn test_aggregator() {
        let mut aggregator = TelemetryAggregator::new();

        let mut t1 = ExecutionTelemetry::new();
        t1.ai_latency_ms = 100;
        t1.execution_time_ms = 50;
        t1.success = true;
        aggregator.record(&t1);

        let mut t2 = ExecutionTelemetry::new();
        t2.ai_latency_ms = 200;
        t2.execution_time_ms = 150; // Exceeds budget
        t2.success = false;
        aggregator.record(&t2);

        assert_eq!(aggregator.total_executions, 2);
        assert_eq!(aggregator.successful_executions, 1);
        assert_eq!(aggregator.budget_exceeded_count, 1);
        assert_eq!(aggregator.avg_ai_latency_ms(), 150);
    }
}
