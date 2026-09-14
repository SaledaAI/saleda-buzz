#![deny(unsafe_code)]
#![warn(missing_docs)]
//! Provider-neutral execution contracts for task-board code tasks.
//!
//! Provider selection, credentials, local paths, worktree ownership, and
//! runtime process control deliberately remain behind an adapter implementing
//! [`TaskExecutor`]. They are never fields in this public boundary.

use std::future::Future;
use std::pin::Pin;

use buzz_taskboard::AutomationMode;
use serde::{Deserialize, Serialize};

/// Boxed asynchronous executor operation.
pub type ExecutorFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, ExecutorError>> + Send + 'a>>;

/// Where an executor can perform filesystem-backed work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionLocality {
    /// Runs on the Desktop host that owns the managed agent configuration.
    Desktop,
    /// Runs on a remote host advertised by a managed executor.
    Remote,
}

/// Readiness exposed without provider or model details.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutorReadiness {
    /// The executor can accept a task immediately.
    Ready,
    /// The executor is valid but must be started first.
    Stopped,
    /// The executor is temporarily out of task capacity.
    AtCapacity,
    /// The executor is offline or otherwise unavailable.
    Unavailable,
}

/// Provider-neutral facts needed to decide whether a task can start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutorCapabilities {
    /// Host relationship for worktree execution.
    pub locality: ExecutionLocality,
    /// Current availability without leaking the provider state machine.
    pub readiness: ExecutorReadiness,
    /// Whether the configured runtime has confirmed developer-tool support.
    pub supports_tools: bool,
    /// Maximum simultaneous task sessions allowed by the runtime owner.
    pub capacity: usize,
    /// Number of task sessions currently owned by this executor.
    pub active_sessions: usize,
}

impl ExecutorCapabilities {
    /// Return whether an unattended coding session is eligible to start now.
    pub const fn can_start(&self) -> bool {
        self.supports_tools
            && matches!(self.readiness, ExecutorReadiness::Ready)
            && self.active_sessions < self.capacity
    }
}

/// Provider-neutral inputs handed to the executor adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSessionRequest {
    /// Signed kind:43001 event ID.
    pub request_id: String,
    /// NIP-34 issue event ID.
    pub issue_id: String,
    /// NIP-34 repository address.
    pub repository_address: String,
    /// Validated base ref from the versioned request.
    pub base_ref: String,
    /// Human task instructions, with no secrets or local paths.
    pub prompt: String,
    /// Effective automation after task, agent, and persona policy narrowing.
    pub automation: AutomationMode,
}

/// Opaque session identity safe to retain outside the provider adapter.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskSessionHandle(
    /// Adapter-owned opaque identifier; it must not encode a local path.
    pub String,
);

/// Session state translated into task-job progress events by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskSessionState {
    /// Accepted but waiting for an executor capacity slot.
    WaitingForCapacity,
    /// Accepted but waiting for the configured runtime to become ready.
    WaitingForModel,
    /// The supervised ACP session is running.
    Running,
    /// The working tree contains a reviewable result.
    ReviewReady,
    /// Only this task session was cancelled; worktree retention is separate.
    Cancelled,
    /// The task session failed.
    Failed,
    /// The task completed under its effective automation policy.
    Completed,
}

/// Errors returned by a task executor adapter.
#[derive(Debug, thiserror::Error)]
pub enum ExecutorError {
    /// The selected executor cannot currently accept this task.
    #[error("executor unavailable: {0}")]
    Unavailable(String),
    /// The request exceeded executor or persona policy.
    #[error("task policy rejected: {0}")]
    Policy(String),
    /// Starting or controlling the isolated session failed.
    #[error("task session failed: {0}")]
    Session(String),
}

/// Boundary implemented by Desktop or remote managed-agent adapters.
pub trait TaskExecutor: Send + Sync {
    /// Return current provider-neutral readiness and capacity.
    fn capabilities(&self) -> ExecutorCapabilities;

    /// Start an isolated session through existing managed-agent configuration.
    fn start_session<'a>(
        &'a self,
        request: &'a TaskSessionRequest,
    ) -> ExecutorFuture<'a, TaskSessionHandle>;

    /// Cancel only the referenced task session.
    fn cancel_session<'a>(&'a self, handle: &'a TaskSessionHandle) -> ExecutorFuture<'a, ()>;
}

/// Narrow a task request by both agent and persona automation ceilings.
pub fn effective_automation(
    requested: AutomationMode,
    agent_ceiling: AutomationMode,
    persona_ceiling: AutomationMode,
) -> AutomationMode {
    let mut effective = requested;
    for candidate in [agent_ceiling, persona_ceiling] {
        if automation_rank(candidate) < automation_rank(effective) {
            effective = candidate;
        }
    }
    effective
}

const fn automation_rank(mode: AutomationMode) -> u8 {
    match mode {
        AutomationMode::Supervised => 0,
        AutomationMode::AutoCommit => 1,
        AutomationMode::ProposePr => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automation_can_only_be_narrowed() {
        assert_eq!(
            effective_automation(
                AutomationMode::ProposePr,
                AutomationMode::AutoCommit,
                AutomationMode::Supervised,
            ),
            AutomationMode::Supervised
        );
    }

    #[test]
    fn tools_and_capacity_are_required() {
        let mut capabilities = ExecutorCapabilities {
            locality: ExecutionLocality::Desktop,
            readiness: ExecutorReadiness::Ready,
            supports_tools: true,
            capacity: 1,
            active_sessions: 0,
        };
        assert!(capabilities.can_start());
        capabilities.active_sessions = 1;
        assert!(!capabilities.can_start());
    }
}
