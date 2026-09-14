#![deny(unsafe_code)]
#![warn(missing_docs)]
//! Provider-neutral task-board projection and code-task job protocol.
//!
//! This crate has no I/O and no agent-provider knowledge. It projects signed
//! Nostr event envelopes supplied by a caller and builds the custom tags and
//! JSON payloads shared by Desktop, web, mobile, CLI, and executors.

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use buzz_core::kind::{
    KIND_GIT_STATUS_CLOSED, KIND_GIT_STATUS_DRAFT, KIND_GIT_STATUS_MERGED, KIND_GIT_STATUS_OPEN,
};
use serde::{Deserialize, Serialize};

/// Custom tag containing the fine-grained task-board column.
pub const TAG_BOARD_STATE: &str = "buzz-board-state";
/// Repeatable custom tag containing an issue dependency event ID.
pub const TAG_DEPENDS_ON: &str = "buzz-depends-on";
/// Custom tag containing the Git ref from which execution should branch.
pub const TAG_BASE_REF: &str = "buzz-base-ref";
/// Custom tag containing the selected executor agent pubkey.
pub const TAG_EXECUTOR: &str = "buzz-executor";
/// Custom tag containing the requested automation ceiling.
pub const TAG_AUTOMATION: &str = "buzz-automation";
/// Version of the code-task JSON payload contract.
pub const CODE_TASK_VERSION: u8 = 1;
/// Operation discriminator used by code-task request payloads.
pub const CODE_TASK_OPERATION: &str = "code-task";
/// Maximum number of dependencies carried by one task snapshot.
pub const MAX_DEPENDENCIES: usize = 100;
/// Maximum UTF-8 byte length of a base ref carried by the protocol.
pub const MAX_BASE_REF_BYTES: usize = 256;
/// Maximum UTF-8 byte length of a code-task prompt.
pub const MAX_PROMPT_BYTES: usize = 64 * 1024;
/// Maximum UTF-8 byte length of a semantic job summary.
pub const MAX_SUMMARY_BYTES: usize = 4 * 1024;

/// Errors produced while validating or projecting task-board protocol data.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TaskboardError {
    /// A required or constrained field was invalid.
    #[error("invalid {field}: {message}")]
    InvalidField {
        /// Name of the invalid field.
        field: &'static str,
        /// Human-readable validation failure.
        message: String,
    },
    /// A singleton tag occurred more than once.
    #[error("duplicate singleton tag: {0}")]
    DuplicateTag(&'static str),
    /// A task-board snapshot had custom fields but no state discriminator.
    #[error("partial task-board snapshot is missing {TAG_BOARD_STATE}")]
    PartialSnapshot,
    /// A JSON payload could not be decoded.
    #[error("invalid JSON payload: {0}")]
    InvalidJson(String),
}

/// Fine-grained task-board state projected over NIP-34 lifecycle kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoardState {
    /// Newly reported work awaiting triage.
    Triage,
    /// Accepted work that has not started.
    Backlog,
    /// Work currently being implemented.
    InProgress,
    /// Work awaiting human or agent review.
    Review,
    /// Work resolved successfully.
    Done,
    /// Work closed without the resolved semantics.
    Closed,
}

impl BoardState {
    /// Return the interoperable NIP-34 status kind for this board state.
    pub const fn status_kind(self) -> u32 {
        match self {
            Self::Triage => KIND_GIT_STATUS_DRAFT,
            Self::Backlog | Self::InProgress | Self::Review => KIND_GIT_STATUS_OPEN,
            Self::Done => KIND_GIT_STATUS_MERGED,
            Self::Closed => KIND_GIT_STATUS_CLOSED,
        }
    }

    /// Return the stable wire value used in the `buzz-board-state` tag.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Triage => "triage",
            Self::Backlog => "backlog",
            Self::InProgress => "in_progress",
            Self::Review => "review",
            Self::Done => "done",
            Self::Closed => "closed",
        }
    }

    fn legacy_from_status(kind: u32, issue_labels: &[String]) -> Option<Self> {
        match kind {
            KIND_GIT_STATUS_MERGED => Some(Self::Done),
            KIND_GIT_STATUS_CLOSED => Some(Self::Closed),
            KIND_GIT_STATUS_DRAFT => Some(Self::Triage),
            KIND_GIT_STATUS_OPEN => {
                let labels = issue_labels
                    .iter()
                    .map(|label| label.to_ascii_lowercase())
                    .collect::<BTreeSet<_>>();
                if labels.contains("in-review") || labels.contains("review") {
                    Some(Self::Review)
                } else if labels.contains("in-progress") || labels.contains("active") {
                    Some(Self::InProgress)
                } else if labels.contains("triage") {
                    Some(Self::Triage)
                } else {
                    Some(Self::Backlog)
                }
            }
            _ => None,
        }
    }
}

impl FromStr for BoardState {
    type Err = TaskboardError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "triage" => Ok(Self::Triage),
            "backlog" => Ok(Self::Backlog),
            "in_progress" => Ok(Self::InProgress),
            "review" => Ok(Self::Review),
            "done" => Ok(Self::Done),
            "closed" => Ok(Self::Closed),
            other => Err(invalid("board state", format!("unknown value {other:?}"))),
        }
    }
}

/// Requested automation ceiling for a task.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationMode {
    /// The agent may edit and test, but a human controls durable Git actions.
    #[default]
    Supervised,
    /// The agent may create a local commit when policy also permits it.
    AutoCommit,
    /// The agent may propose a pull request when policy also permits it.
    ProposePr,
}

impl AutomationMode {
    /// Return the stable wire representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Supervised => "supervised",
            Self::AutoCommit => "auto_commit",
            Self::ProposePr => "propose_pr",
        }
    }
}

impl FromStr for AutomationMode {
    type Err = TaskboardError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "supervised" => Ok(Self::Supervised),
            "auto_commit" => Ok(Self::AutoCommit),
            "propose_pr" => Ok(Self::ProposePr),
            other => Err(invalid("automation", format!("unknown value {other:?}"))),
        }
    }
}

/// Complete Buzz task-board snapshot carried by a NIP-34 status event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardSnapshot {
    /// Fine-grained board state.
    pub state: BoardState,
    /// Issue event IDs that must complete before this task can execute.
    pub dependencies: Vec<String>,
    /// Git ref from which a task worktree should branch.
    pub base_ref: Option<String>,
    /// Selected executor agent pubkey.
    pub executor: Option<String>,
    /// Requested automation ceiling.
    pub automation: AutomationMode,
}

impl BoardSnapshot {
    /// Construct and validate a complete snapshot.
    pub fn new(
        state: BoardState,
        dependencies: Vec<String>,
        base_ref: Option<String>,
        executor: Option<String>,
        automation: AutomationMode,
    ) -> Result<Self, TaskboardError> {
        let mut dependencies = dependencies
            .into_iter()
            .map(|id| normalize_hex64(&id, "dependency"))
            .collect::<Result<Vec<_>, _>>()?;
        dependencies.sort();
        dependencies.dedup();
        if dependencies.len() > MAX_DEPENDENCIES {
            return Err(invalid(
                "dependencies",
                format!("maximum is {MAX_DEPENDENCIES}"),
            ));
        }
        let base_ref = base_ref
            .map(|value| validate_base_ref(&value).map(str::to_owned))
            .transpose()?;
        let executor = executor
            .map(|value| normalize_hex64(&value, "executor"))
            .transpose()?;
        Ok(Self {
            state,
            dependencies,
            base_ref,
            executor,
            automation,
        })
    }

    /// Parse a complete snapshot from raw Nostr tags.
    ///
    /// Returns `Ok(None)` for a legacy status event with no task-board tags.
    /// When a state tag is present, a missing automation tag defaults to the
    /// safe `supervised` mode for forward compatibility with early writers.
    pub fn from_tags(tags: &[Vec<String>]) -> Result<Option<Self>, TaskboardError> {
        let state = singleton_tag(tags, TAG_BOARD_STATE)?;
        let has_other_snapshot_tag = tags.iter().any(|tag| {
            matches!(
                tag.first().map(String::as_str),
                Some(TAG_DEPENDS_ON | TAG_BASE_REF | TAG_EXECUTOR | TAG_AUTOMATION)
            )
        });
        let Some(state) = state else {
            return if has_other_snapshot_tag {
                Err(TaskboardError::PartialSnapshot)
            } else {
                Ok(None)
            };
        };

        let dependencies = tags
            .iter()
            .filter(|tag| tag.first().is_some_and(|name| name == TAG_DEPENDS_ON))
            .map(|tag| {
                tag.get(1)
                    .ok_or_else(|| invalid("dependency", "missing tag value"))
                    .and_then(|value| normalize_hex64(value, "dependency"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let base_ref = singleton_tag(tags, TAG_BASE_REF)?.map(str::to_owned);
        let executor = singleton_tag(tags, TAG_EXECUTOR)?.map(str::to_owned);
        let automation = singleton_tag(tags, TAG_AUTOMATION)?
            .map(AutomationMode::from_str)
            .transpose()?
            .unwrap_or_default();

        Self::new(
            BoardState::from_str(state)?,
            dependencies,
            base_ref,
            executor,
            automation,
        )
        .map(Some)
    }

    /// Build the custom tags for this complete snapshot.
    pub fn to_tags(&self) -> Vec<Vec<String>> {
        let mut tags = vec![vec![TAG_BOARD_STATE.into(), self.state.as_str().into()]];
        tags.extend(
            self.dependencies
                .iter()
                .map(|id| vec![TAG_DEPENDS_ON.into(), id.clone()]),
        );
        if let Some(base_ref) = &self.base_ref {
            tags.push(vec![TAG_BASE_REF.into(), base_ref.clone()]);
        }
        if let Some(executor) = &self.executor {
            tags.push(vec![TAG_EXECUTOR.into(), executor.clone()]);
        }
        tags.push(vec![TAG_AUTOMATION.into(), self.automation.as_str().into()]);
        tags
    }
}

/// Minimal signed-event envelope required by task-board projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskboardEvent {
    /// Lowercase hexadecimal event ID.
    pub id: String,
    /// Lowercase hexadecimal signer public key.
    pub pubkey: String,
    /// Nostr event kind.
    pub kind: u32,
    /// Unix timestamp in seconds.
    pub created_at: u64,
    /// Raw Nostr tags.
    pub tags: Vec<Vec<String>>,
    /// Event content. Board fixtures may omit it; code-task events require it.
    #[serde(default)]
    pub content: String,
}

impl TaskboardEvent {
    /// Validate and normalize the event ID and signer fields.
    pub fn normalized(mut self) -> Result<Self, TaskboardError> {
        self.id = normalize_hex64(&self.id, "event id")?;
        self.pubkey = normalize_hex64(&self.pubkey, "event pubkey")?;
        Ok(self)
    }

    /// Return all non-empty values for a named tag.
    pub fn tag_values<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> {
        self.tags.iter().filter_map(move |tag| {
            (tag.first().is_some_and(|value| value == name))
                .then(|| tag.get(1).map(String::as_str))
                .flatten()
                .filter(|value| !value.is_empty())
        })
    }
}

/// Projected board state for one NIP-34 issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectedIssue {
    /// Issue event ID.
    pub issue_id: String,
    /// Effective complete board snapshot.
    pub snapshot: BoardSnapshot,
    /// Status event selected by authorization and LWW projection.
    pub status_event_id: Option<String>,
    /// Timestamp of the selected status, or the issue timestamp when absent.
    pub updated_at: u64,
    /// Validation errors that block task execution but not rendering.
    pub validation_errors: Vec<String>,
}

impl ProjectedIssue {
    /// Whether the projected issue is structurally eligible for execution.
    pub fn is_executable(&self) -> bool {
        self.validation_errors.is_empty()
    }
}

/// Project one issue and its candidate status events deterministically.
///
/// The issue author and repository owner are automatically authorized. The
/// caller may provide additional maintainer pubkeys resolved from the current
/// repository announcement. Unauthorized or unrelated statuses are ignored.
pub fn project_issue(
    issue: &TaskboardEvent,
    status_events: &[TaskboardEvent],
    maintainers: impl IntoIterator<Item = String>,
) -> Result<ProjectedIssue, TaskboardError> {
    let issue_id = normalize_hex64(&issue.id, "issue id")?;
    let mut authorized = BTreeSet::from([normalize_hex64(&issue.pubkey, "issue author")?]);
    if let Some(owner) = issue
        .tag_values("a")
        .find_map(repo_owner_from_address)
        .transpose()?
    {
        authorized.insert(owner);
    }
    for maintainer in maintainers {
        authorized.insert(normalize_hex64(&maintainer, "maintainer")?);
    }

    let latest = status_events
        .iter()
        .filter(|event| is_status_kind(event.kind))
        .filter(|event| {
            normalize_hex64(&event.pubkey, "status author")
                .is_ok_and(|pubkey| authorized.contains(&pubkey))
        })
        .filter(|event| {
            event
                .tag_values("e")
                .any(|referenced| referenced.eq_ignore_ascii_case(&issue_id))
        })
        .max_by(|left, right| {
            left.created_at.cmp(&right.created_at).then_with(|| {
                left.id
                    .to_ascii_lowercase()
                    .cmp(&right.id.to_ascii_lowercase())
            })
        });

    let labels = issue.tag_values("t").map(str::to_owned).collect::<Vec<_>>();
    let default_state = latest
        .and_then(|event| BoardState::legacy_from_status(event.kind, &labels))
        .unwrap_or_else(|| legacy_state_from_issue_labels(&labels));
    let mut errors = Vec::new();
    let snapshot = if let Some(status) = latest {
        match BoardSnapshot::from_tags(&status.tags) {
            Ok(Some(snapshot)) => {
                if snapshot.state.status_kind() != status.kind {
                    errors.push(format!(
                        "board state {} conflicts with NIP-34 status kind {}",
                        snapshot.state.as_str(),
                        status.kind
                    ));
                }
                snapshot
            }
            Ok(None) => BoardSnapshot::new(
                default_state,
                Vec::new(),
                None,
                None,
                AutomationMode::Supervised,
            )?,
            Err(error) => {
                errors.push(error.to_string());
                BoardSnapshot::new(
                    default_state,
                    Vec::new(),
                    None,
                    None,
                    AutomationMode::Supervised,
                )?
            }
        }
    } else {
        BoardSnapshot::new(
            default_state,
            Vec::new(),
            None,
            None,
            AutomationMode::Supervised,
        )?
    };

    Ok(ProjectedIssue {
        issue_id,
        snapshot,
        status_event_id: latest.map(|event| event.id.to_ascii_lowercase()),
        updated_at: latest.map_or(issue.created_at, |event| event.created_at),
        validation_errors: errors,
    })
}

/// Result of analyzing dependencies across a projected board.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyAnalysis {
    /// Missing dependency IDs keyed by the issue that references them.
    pub missing: BTreeMap<String, Vec<String>>,
    /// All issue IDs participating in at least one dependency cycle.
    pub cycle_members: BTreeSet<String>,
}

impl DependencyAnalysis {
    /// Whether every referenced task exists and the graph is acyclic.
    pub fn is_valid(&self) -> bool {
        self.missing.is_empty() && self.cycle_members.is_empty()
    }
}

/// Analyze missing dependencies and cycle membership for a complete board.
pub fn analyze_dependencies(issues: &[ProjectedIssue]) -> DependencyAnalysis {
    let graph = issues
        .iter()
        .map(|issue| (issue.issue_id.clone(), issue.snapshot.dependencies.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut analysis = DependencyAnalysis::default();
    for (issue_id, dependencies) in &graph {
        let missing = dependencies
            .iter()
            .filter(|dependency| !graph.contains_key(*dependency))
            .cloned()
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            analysis.missing.insert(issue_id.clone(), missing);
        }
    }

    let mut visited = BTreeSet::new();
    let mut active = Vec::new();
    for issue_id in graph.keys() {
        visit_dependencies(
            issue_id,
            &graph,
            &mut visited,
            &mut active,
            &mut analysis.cycle_members,
        );
    }
    analysis
}

/// Versioned payload for a kind `43001` code-task request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodeTaskRequestV1 {
    /// Protocol version; always `1` for this shape.
    pub v: u8,
    /// Operation discriminator; always `code-task`.
    pub operation: String,
    /// Human-authored task prompt.
    pub prompt: String,
    /// Git ref from which the executor should branch.
    pub base_ref: String,
    /// Requested automation ceiling.
    pub automation: AutomationMode,
}

impl CodeTaskRequestV1 {
    /// Construct and validate a v1 request.
    pub fn new(
        prompt: impl Into<String>,
        base_ref: impl Into<String>,
        automation: AutomationMode,
    ) -> Result<Self, TaskboardError> {
        let prompt = prompt.into();
        validate_text(&prompt, "prompt", MAX_PROMPT_BYTES)?;
        let base_ref = validate_base_ref(&base_ref.into())?.to_owned();
        Ok(Self {
            v: CODE_TASK_VERSION,
            operation: CODE_TASK_OPERATION.into(),
            prompt,
            base_ref,
            automation,
        })
    }

    /// Decode and validate a request payload.
    pub fn from_json(json: &str) -> Result<Self, TaskboardError> {
        let request: Self = serde_json::from_str(json)
            .map_err(|error| TaskboardError::InvalidJson(error.to_string()))?;
        if request.v != CODE_TASK_VERSION {
            return Err(invalid("v", format!("unsupported version {}", request.v)));
        }
        if request.operation != CODE_TASK_OPERATION {
            return Err(invalid(
                "operation",
                format!("expected {CODE_TASK_OPERATION:?}"),
            ));
        }
        Self::new(request.prompt, request.base_ref, request.automation)
    }

    /// Serialize the request payload.
    pub fn to_json(&self) -> Result<String, TaskboardError> {
        serde_json::to_string(self).map_err(|error| TaskboardError::InvalidJson(error.to_string()))
    }
}

/// Provider-neutral semantic phase for job progress and outcome payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobPhase {
    /// The targeted executor has accepted the request.
    Accepted,
    /// The request is waiting behind the executor's capacity limit.
    WaitingForCapacity,
    /// The selected model or provider is not ready yet.
    WaitingForModel,
    /// The agent is actively working.
    Running,
    /// The supervised result is ready for human review.
    ReviewReady,
    /// The request was cancelled.
    Cancelled,
    /// The task failed.
    Failed,
    /// The task completed within its effective automation policy.
    Completed,
}

/// Versioned semantic job update safe for activity feeds and remote clients.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobUpdateV1 {
    /// Protocol version; always `1` for this shape.
    pub v: u8,
    /// Semantic lifecycle phase.
    pub phase: JobPhase,
    /// Human-readable, sanitized outcome-first summary.
    pub summary: String,
}

impl JobUpdateV1 {
    /// Construct a validated semantic update.
    pub fn new(phase: JobPhase, summary: impl Into<String>) -> Result<Self, TaskboardError> {
        let summary = summary.into();
        validate_text(&summary, "summary", MAX_SUMMARY_BYTES)?;
        Ok(Self {
            v: CODE_TASK_VERSION,
            phase,
            summary,
        })
    }

    /// Decode and validate a semantic lifecycle update.
    pub fn from_json(json: &str) -> Result<Self, TaskboardError> {
        let update: Self = serde_json::from_str(json)
            .map_err(|error| TaskboardError::InvalidJson(error.to_string()))?;
        if update.v != CODE_TASK_VERSION {
            return Err(invalid("v", format!("unsupported version {}", update.v)));
        }
        Self::new(update.phase, update.summary)
    }

    /// Serialize this lifecycle update.
    pub fn to_json(&self) -> Result<String, TaskboardError> {
        serde_json::to_string(self).map_err(|error| TaskboardError::InvalidJson(error.to_string()))
    }
}

/// References required on a kind `43001` code-task request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRequestRefs {
    /// NIP-34 issue event ID.
    pub issue_id: String,
    /// NIP-34 repository address (`30617:<owner>:<d>`).
    pub repo_address: String,
    /// Optional NIP-MP project address (`30621:<owner>:<d>`).
    pub project_address: Option<String>,
    /// Target executor agent pubkey.
    pub executor_pubkey: String,
}

impl JobRequestRefs {
    /// Validate references and build their Nostr tags.
    pub fn to_tags(&self) -> Result<Vec<Vec<String>>, TaskboardError> {
        let issue = normalize_hex64(&self.issue_id, "issue id")?;
        validate_address(&self.repo_address, 30617, "repo address")?;
        if let Some(project) = &self.project_address {
            validate_address(project, 30621, "project address")?;
        }
        let executor = normalize_hex64(&self.executor_pubkey, "executor pubkey")?;
        let mut tags = vec![
            vec!["e".into(), issue, String::new(), "issue".into()],
            vec!["a".into(), self.repo_address.clone()],
            vec!["p".into(), executor],
            vec!["t".into(), CODE_TASK_OPERATION.into()],
        ];
        if let Some(project) = &self.project_address {
            tags.push(vec!["a".into(), project.clone()]);
        }
        Ok(tags)
    }
}

/// References required on a lifecycle event for an existing code-task job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobLifecycleRefs {
    /// Root kind `43001` request event ID.
    pub request_id: String,
    /// NIP-34 issue event ID.
    pub issue_id: String,
}

impl JobLifecycleRefs {
    /// Validate references and build their Nostr tags.
    pub fn to_tags(&self) -> Result<Vec<Vec<String>>, TaskboardError> {
        Ok(vec![
            vec![
                "e".into(),
                normalize_hex64(&self.request_id, "request id")?,
                String::new(),
                "root".into(),
            ],
            vec![
                "e".into(),
                normalize_hex64(&self.issue_id, "issue id")?,
                String::new(),
                "issue".into(),
            ],
        ])
    }
}

/// Latest valid executor-authored lifecycle state for a code-task request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectedJob {
    /// Root kind `43001` request event ID.
    pub request_id: String,
    /// Referenced NIP-34 issue event ID.
    pub issue_id: String,
    /// Target executor and required lifecycle signer.
    pub executor: String,
    /// Latest valid semantic phase.
    pub phase: JobPhase,
    /// Latest valid sanitized summary.
    pub summary: String,
    /// Timestamp of the selected update, or the request when none exists.
    pub updated_at: u64,
    /// Selected lifecycle event ID, when one exists.
    pub terminal_event_id: Option<String>,
}

/// Project a request and its lifecycle history using executor authorization
/// and deterministic timestamp/event-ID last-write-wins ordering.
pub fn project_job(
    request: &TaskboardEvent,
    lifecycle: &[TaskboardEvent],
) -> Result<ProjectedJob, TaskboardError> {
    if request.kind != 43001 {
        return Err(invalid("job request kind", "must be 43001"));
    }
    CodeTaskRequestV1::from_json(&request.content)?;
    let request_id = normalize_hex64(&request.id, "request id")?;
    let issue_id = request
        .tags
        .iter()
        .find(|tag| {
            tag.first().is_some_and(|value| value == "e")
                && tag.get(3).is_some_and(|value| value == "issue")
        })
        .and_then(|tag| tag.get(1))
        .ok_or_else(|| invalid("job issue reference", "missing issue e-tag"))
        .and_then(|value| normalize_hex64(value, "issue id"))?;
    let executor = request
        .tag_values("p")
        .next()
        .ok_or_else(|| invalid("job executor", "missing p-tag"))
        .and_then(|value| normalize_hex64(value, "executor pubkey"))?;
    let mut candidates = lifecycle
        .iter()
        .filter(|event| matches!(event.kind, 43002 | 43003 | 43004 | 43006))
        .filter(|event| event.pubkey.eq_ignore_ascii_case(&executor))
        .filter(|event| has_marked_event_ref(event, &request_id, "root"))
        .filter(|event| has_marked_event_ref(event, &issue_id, "issue"))
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right.created_at.cmp(&left.created_at).then_with(|| {
            right
                .id
                .to_ascii_lowercase()
                .cmp(&left.id.to_ascii_lowercase())
        })
    });
    let latest = candidates.into_iter().find_map(|event| {
        JobUpdateV1::from_json(&event.content)
            .ok()
            .map(|update| (event, update))
    });
    let (phase, summary, updated_at, terminal_event_id) = latest.map_or_else(
        || {
            (
                JobPhase::Accepted,
                "Task request published.".to_owned(),
                request.created_at,
                None,
            )
        },
        |(event, update)| {
            (
                update.phase,
                update.summary,
                event.created_at,
                Some(event.id.to_ascii_lowercase()),
            )
        },
    );
    Ok(ProjectedJob {
        request_id,
        issue_id,
        executor,
        phase,
        summary,
        updated_at,
        terminal_event_id,
    })
}

fn has_marked_event_ref(event: &TaskboardEvent, id: &str, marker: &str) -> bool {
    event.tags.iter().any(|tag| {
        tag.first().is_some_and(|value| value == "e")
            && tag
                .get(1)
                .is_some_and(|value| value.eq_ignore_ascii_case(id))
            && tag.get(3).is_some_and(|value| value == marker)
    })
}

fn singleton_tag<'a>(
    tags: &'a [Vec<String>],
    name: &'static str,
) -> Result<Option<&'a str>, TaskboardError> {
    let mut values = tags
        .iter()
        .filter(|tag| tag.first().is_some_and(|value| value == name))
        .map(|tag| {
            tag.get(1)
                .map(String::as_str)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| invalid(name, "missing tag value"))
        });
    let first = values.next().transpose()?;
    if values.next().is_some() {
        return Err(TaskboardError::DuplicateTag(name));
    }
    Ok(first)
}

fn invalid(field: &'static str, message: impl Into<String>) -> TaskboardError {
    TaskboardError::InvalidField {
        field,
        message: message.into(),
    }
}

fn normalize_hex64(value: &str, field: &'static str) -> Result<String, TaskboardError> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid(field, "must be exactly 64 hexadecimal characters"));
    }
    Ok(value.to_ascii_lowercase())
}

fn validate_text(value: &str, field: &'static str, maximum: usize) -> Result<(), TaskboardError> {
    if value.trim().is_empty() {
        return Err(invalid(field, "must not be empty"));
    }
    if value.len() > maximum {
        return Err(invalid(field, format!("must not exceed {maximum} bytes")));
    }
    if value
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err(invalid(field, "contains unsupported control characters"));
    }
    Ok(())
}

fn validate_base_ref(value: &str) -> Result<&str, TaskboardError> {
    validate_text(value, "base ref", MAX_BASE_REF_BYTES)?;
    if value != value.trim()
        || value.contains(char::is_whitespace)
        || value.contains("..")
        || value.starts_with('-')
        || value.ends_with('.')
        || value.ends_with('/')
        || value.contains("//")
        || value.contains("@{")
        || value
            .bytes()
            .any(|byte| matches!(byte, b'~' | b'^' | b':' | b'?' | b'*' | b'[' | b'\\'))
    {
        return Err(invalid("base ref", "is not a safe Git ref name"));
    }
    Ok(value)
}

fn validate_address(
    value: &str,
    expected_kind: u32,
    field: &'static str,
) -> Result<(), TaskboardError> {
    let mut parts = value.splitn(3, ':');
    let kind = parts.next().and_then(|part| part.parse::<u32>().ok());
    let owner = parts.next();
    let identifier = parts.next();
    if kind != Some(expected_kind) {
        return Err(invalid(field, format!("must use kind {expected_kind}")));
    }
    let owner = owner.ok_or_else(|| invalid(field, "missing owner"))?;
    normalize_hex64(owner, field)?;
    if identifier.is_none_or(str::is_empty) {
        return Err(invalid(field, "missing identifier"));
    }
    Ok(())
}

fn repo_owner_from_address(value: &str) -> Option<Result<String, TaskboardError>> {
    value
        .strip_prefix("30617:")
        .and_then(|rest| rest.split(':').next())
        .map(|owner| normalize_hex64(owner, "repo owner"))
}

fn is_status_kind(kind: u32) -> bool {
    matches!(
        kind,
        KIND_GIT_STATUS_OPEN
            | KIND_GIT_STATUS_MERGED
            | KIND_GIT_STATUS_CLOSED
            | KIND_GIT_STATUS_DRAFT
    )
}

fn legacy_state_from_issue_labels(labels: &[String]) -> BoardState {
    BoardState::legacy_from_status(KIND_GIT_STATUS_OPEN, labels).unwrap_or(BoardState::Backlog)
}

fn visit_dependencies(
    issue_id: &str,
    graph: &BTreeMap<String, Vec<String>>,
    visited: &mut BTreeSet<String>,
    active: &mut Vec<String>,
    cycle_members: &mut BTreeSet<String>,
) {
    if let Some(cycle_start) = active.iter().position(|active_id| active_id == issue_id) {
        cycle_members.extend(active[cycle_start..].iter().cloned());
        return;
    }
    if !visited.insert(issue_id.to_owned()) {
        return;
    }
    active.push(issue_id.to_owned());
    if let Some(dependencies) = graph.get(issue_id) {
        for dependency in dependencies {
            if graph.contains_key(dependency) {
                visit_dependencies(dependency, graph, visited, active, cycle_members);
            }
        }
    }
    active.pop();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct ProjectionFixtures {
        version: u8,
        cases: Vec<ProjectionFixture>,
    }

    #[derive(Deserialize)]
    struct ProjectionFixture {
        name: String,
        issue: TaskboardEvent,
        statuses: Vec<TaskboardEvent>,
        maintainers: Vec<String>,
        expected: serde_json::Value,
    }

    const ISSUE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const DEPENDENCY: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const AUTHOR: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    const OWNER: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
    const EXECUTOR: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

    fn issue(id: &str, labels: &[&str]) -> TaskboardEvent {
        let mut tags = vec![vec!["a".into(), format!("30617:{OWNER}:buzz")]];
        tags.extend(labels.iter().map(|label| vec!["t".into(), (*label).into()]));
        TaskboardEvent {
            id: id.into(),
            pubkey: AUTHOR.into(),
            kind: 1621,
            created_at: 1,
            tags,
            content: String::new(),
        }
    }

    fn status(
        id: &str,
        author: &str,
        kind: u32,
        created_at: u64,
        tags: Vec<Vec<String>>,
    ) -> TaskboardEvent {
        let mut all_tags = vec![vec!["e".into(), ISSUE.into(), String::new(), "root".into()]];
        all_tags.extend(tags);
        TaskboardEvent {
            id: id.into(),
            pubkey: author.into(),
            kind,
            created_at,
            tags: all_tags,
            content: String::new(),
        }
    }

    #[test]
    fn snapshot_round_trips_in_canonical_order() {
        let snapshot = BoardSnapshot::new(
            BoardState::InProgress,
            vec![DEPENDENCY.to_ascii_uppercase(), DEPENDENCY.into()],
            Some("main".into()),
            Some(EXECUTOR.to_ascii_uppercase()),
            AutomationMode::Supervised,
        )
        .unwrap();
        let tags = snapshot.to_tags();
        assert_eq!(BoardSnapshot::from_tags(&tags).unwrap(), Some(snapshot));
        assert_eq!(
            tags.iter().filter(|tag| tag[0] == TAG_DEPENDS_ON).count(),
            1
        );
    }

    #[test]
    fn projection_ignores_unauthorized_status_and_breaks_timestamp_ties_by_id() {
        let snapshot = BoardSnapshot::new(
            BoardState::Review,
            vec![],
            None,
            Some(EXECUTOR.into()),
            AutomationMode::Supervised,
        )
        .unwrap();
        let unauthorized = status(
            &"f".repeat(64),
            EXECUTOR,
            KIND_GIT_STATUS_MERGED,
            100,
            BoardSnapshot::new(
                BoardState::Done,
                vec![],
                None,
                None,
                AutomationMode::Supervised,
            )
            .unwrap()
            .to_tags(),
        );
        let earlier_id = status(
            &"1".repeat(64),
            AUTHOR,
            KIND_GIT_STATUS_OPEN,
            10,
            BoardSnapshot::new(
                BoardState::InProgress,
                vec![],
                None,
                None,
                AutomationMode::Supervised,
            )
            .unwrap()
            .to_tags(),
        );
        let later_id = status(
            &"2".repeat(64),
            OWNER,
            KIND_GIT_STATUS_OPEN,
            10,
            snapshot.to_tags(),
        );
        let projected = project_issue(
            &issue(ISSUE, &[]),
            &[unauthorized, earlier_id, later_id],
            Vec::new(),
        )
        .unwrap();
        assert_eq!(projected.snapshot.state, BoardState::Review);
        assert_eq!(projected.status_event_id, Some("2".repeat(64)));
        assert!(projected.is_executable());
    }

    #[test]
    fn malformed_snapshot_falls_back_but_blocks_execution() {
        let malformed = status(
            &"1".repeat(64),
            AUTHOR,
            KIND_GIT_STATUS_OPEN,
            10,
            vec![vec![TAG_BASE_REF.into(), "main".into()]],
        );
        let projected = project_issue(&issue(ISSUE, &[]), &[malformed], Vec::new()).unwrap();
        assert_eq!(projected.snapshot.state, BoardState::Backlog);
        assert!(!projected.is_executable());
    }

    #[test]
    fn legacy_labels_preserve_existing_intermediate_columns() {
        let projected = project_issue(&issue(ISSUE, &["in-progress"]), &[], Vec::new()).unwrap();
        assert_eq!(projected.snapshot.state, BoardState::InProgress);
        assert_eq!(projected.snapshot.automation, AutomationMode::Supervised);
    }

    #[test]
    fn dependency_analysis_reports_missing_and_all_cycle_members() {
        let third = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
        let projected = |id: &str, dependencies: Vec<String>| ProjectedIssue {
            issue_id: id.into(),
            snapshot: BoardSnapshot::new(
                BoardState::Backlog,
                dependencies,
                None,
                None,
                AutomationMode::Supervised,
            )
            .unwrap(),
            status_event_id: None,
            updated_at: 1,
            validation_errors: vec![],
        };
        let analysis = analyze_dependencies(&[
            projected(ISSUE, vec![DEPENDENCY.into()]),
            projected(DEPENDENCY, vec![ISSUE.into(), third.into()]),
        ]);
        assert_eq!(
            analysis.cycle_members,
            BTreeSet::from([ISSUE.into(), DEPENDENCY.into()])
        );
        assert_eq!(analysis.missing.get(DEPENDENCY), Some(&vec![third.into()]));
    }

    #[test]
    fn code_task_payload_and_refs_are_provider_neutral() {
        let request =
            CodeTaskRequestV1::new("Implement the issue", "main", AutomationMode::Supervised)
                .unwrap();
        let json = request.to_json().unwrap();
        assert_eq!(CodeTaskRequestV1::from_json(&json).unwrap(), request);
        assert!(!json.contains("provider"));
        assert!(!json.contains("model"));

        let tags = JobRequestRefs {
            issue_id: ISSUE.into(),
            repo_address: format!("30617:{OWNER}:buzz"),
            project_address: Some(format!("30621:{OWNER}:platform")),
            executor_pubkey: EXECUTOR.into(),
        }
        .to_tags()
        .unwrap();
        assert!(tags.contains(&vec!["p".into(), EXECUTOR.into()]));
        assert!(!tags
            .iter()
            .flatten()
            .any(|value| value.contains("provider")));
    }

    #[test]
    fn job_projection_uses_latest_valid_executor_update() {
        let request_id = "1".repeat(64);
        let request = TaskboardEvent {
            id: request_id.clone(),
            pubkey: OWNER.into(),
            kind: 43001,
            created_at: 1,
            tags: JobRequestRefs {
                issue_id: ISSUE.into(),
                repo_address: format!("30617:{OWNER}:zorro"),
                project_address: None,
                executor_pubkey: EXECUTOR.into(),
            }
            .to_tags()
            .unwrap(),
            content: CodeTaskRequestV1::new(
                "Implement the issue",
                "main",
                AutomationMode::Supervised,
            )
            .unwrap()
            .to_json()
            .unwrap(),
        };
        let lifecycle = |id: String, created_at, content: String| TaskboardEvent {
            id,
            pubkey: EXECUTOR.into(),
            kind: 43003,
            created_at,
            tags: JobLifecycleRefs {
                request_id: request_id.clone(),
                issue_id: ISSUE.into(),
            }
            .to_tags()
            .unwrap(),
            content,
        };
        let valid = JobUpdateV1::new(JobPhase::Running, "Testing")
            .unwrap()
            .to_json()
            .unwrap();
        let projected = project_job(
            &request,
            &[
                lifecycle("f".repeat(64), 3, "not json".into()),
                lifecycle("e".repeat(64), 2, valid),
            ],
        )
        .unwrap();
        assert_eq!(projected.phase, JobPhase::Running);
        assert_eq!(projected.summary, "Testing");
        assert_eq!(projected.terminal_event_id, Some("e".repeat(64)));
    }

    #[test]
    fn unsafe_base_refs_are_rejected() {
        for value in ["../main", "refs/heads/bad ref", "topic..branch", "-branch"] {
            assert!(BoardSnapshot::new(
                BoardState::Backlog,
                vec![],
                Some(value.into()),
                None,
                AutomationMode::Supervised,
            )
            .is_err());
        }
    }

    #[test]
    fn shared_projection_fixtures_match_rust_projection() {
        let fixtures: ProjectionFixtures =
            serde_json::from_str(include_str!("../fixtures/projection-v1.json")).unwrap();
        assert_eq!(fixtures.version, 1);
        for fixture in fixtures.cases {
            let projected = project_issue(&fixture.issue, &fixture.statuses, fixture.maintainers)
                .unwrap_or_else(|error| panic!("fixture {:?} failed: {error}", fixture.name));
            assert_eq!(
                serde_json::to_value(projected).unwrap(),
                fixture.expected,
                "fixture {:?}",
                fixture.name
            );
        }
    }
}
