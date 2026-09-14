export const TASK_BOARD_STATE_TAG = "buzz-board-state";
export const TASK_DEPENDS_ON_TAG = "buzz-depends-on";
export const TASK_BASE_REF_TAG = "buzz-base-ref";
export const TASK_EXECUTOR_TAG = "buzz-executor";
export const TASK_AUTOMATION_TAG = "buzz-automation";

export type TaskBoardState =
  | "triage"
  | "backlog"
  | "in_progress"
  | "review"
  | "done"
  | "closed";

export type TaskAutomation = "supervised" | "auto_commit" | "propose_pr";

export type TaskBoardSnapshot = {
  state: TaskBoardState;
  dependencies: string[];
  baseRef: string | null;
  executor: string | null;
  automation: TaskAutomation;
};

export type TaskBoardEvent = {
  id: string;
  pubkey: string;
  kind: number;
  created_at: number;
  tags: string[][];
  content?: string;
};

export type ProjectedTaskBoardIssue = {
  issueId: string;
  snapshot: TaskBoardSnapshot;
  statusEventId: string | null;
  updatedAt: number;
  validationErrors: string[];
};

export type TaskDependencyAnalysis = {
  missing: Record<string, string[]>;
  cycleMembers: string[];
};

export type TaskJobPhase =
  | "accepted"
  | "waiting_for_capacity"
  | "waiting_for_model"
  | "running"
  | "review_ready"
  | "cancelled"
  | "failed"
  | "completed";

export type CodeTaskRequestV1 = {
  v: 1;
  operation: "code-task";
  prompt: string;
  baseRef: string;
  automation: TaskAutomation;
};

export type TaskJobUpdateV1 = {
  v: 1;
  phase: TaskJobPhase;
  summary: string;
};

export type ProjectedTaskJob = {
  requestId: string;
  issueId: string;
  executor: string;
  phase: TaskJobPhase;
  summary: string;
  updatedAt: number;
  terminalEventId: string | null;
};

const STATUS_KINDS = new Set([1630, 1631, 1632, 1633]);
const HEX_64 = /^[0-9a-f]{64}$/i;
const JOB_UPDATE_KINDS = new Set([43002, 43003, 43004, 43006]);
const MAX_BASE_REF_BYTES = 256;
const MAX_PROMPT_BYTES = 64 * 1024;
const MAX_SUMMARY_BYTES = 4 * 1024;

function tagValues(event: TaskBoardEvent, name: string): string[] {
  return event.tags
    .filter((tag) => tag[0] === name && Boolean(tag[1]))
    .map((tag) => tag[1]);
}

function singletonTag(tags: string[][], name: string): string | null {
  const matching = tags.filter((tag) => tag[0] === name);
  if (matching.length > 1) {
    throw new Error(`duplicate singleton tag: ${name}`);
  }
  if (matching.length === 0) return null;
  if (!matching[0][1]) throw new Error(`invalid ${name}: missing tag value`);
  return matching[0][1];
}

function normalizeHex64(value: string, field: string): string {
  if (!HEX_64.test(value)) {
    throw new Error(
      `invalid ${field}: must be exactly 64 hexadecimal characters`,
    );
  }
  return value.toLowerCase();
}

function utf8Length(value: string): number {
  return new TextEncoder().encode(value).byteLength;
}

function validateText(value: string, field: string, maximum: number): string {
  if (!value.trim()) throw new Error(`invalid ${field}: must not be empty`);
  if (utf8Length(value) > maximum) {
    throw new Error(`invalid ${field}: must not exceed ${maximum} bytes`);
  }
  for (const character of value) {
    const code = character.codePointAt(0) ?? 0;
    if ((code < 32 || code === 127) && ![9, 10, 13].includes(code)) {
      throw new Error(
        `invalid ${field}: contains unsupported control characters`,
      );
    }
  }
  return value;
}

export function validateTaskBaseRef(value: string): string {
  validateText(value, "base ref", MAX_BASE_REF_BYTES);
  if (
    value !== value.trim() ||
    /\s/.test(value) ||
    value.includes("..") ||
    value.startsWith("-") ||
    value.endsWith(".") ||
    value.endsWith("/") ||
    value.includes("//") ||
    value.includes("@{") ||
    [..."~^:?*[\\"].some((character) => value.includes(character))
  ) {
    throw new Error("invalid base ref: is not a safe Git ref name");
  }
  return value;
}

function validateAddress(
  value: string,
  expectedKind: number,
  field: string,
): string {
  const match = new RegExp(`^${expectedKind}:([0-9a-f]{64}):(.+)$`, "i").exec(
    value,
  );
  if (!match) {
    throw new Error(`invalid ${field}: must use kind ${expectedKind}`);
  }
  return `${expectedKind}:${match[1].toLowerCase()}:${match[2]}`;
}

function parseBoardState(value: string): TaskBoardState {
  if (
    value === "triage" ||
    value === "backlog" ||
    value === "in_progress" ||
    value === "review" ||
    value === "done" ||
    value === "closed"
  ) {
    return value;
  }
  throw new Error(
    `invalid board state: unknown value ${JSON.stringify(value)}`,
  );
}

function parseAutomation(value: string | null): TaskAutomation {
  if (value === null || value === "supervised") return "supervised";
  if (value === "auto_commit" || value === "propose_pr") return value;
  throw new Error(`invalid automation: unknown value ${JSON.stringify(value)}`);
}

function statusKindForState(state: TaskBoardState): number {
  if (state === "triage") return 1633;
  if (state === "done") return 1631;
  if (state === "closed") return 1632;
  return 1630;
}

export function parseTaskBoardSnapshot(
  tags: string[][],
): TaskBoardSnapshot | null {
  const stateValue = singletonTag(tags, TASK_BOARD_STATE_TAG);
  const hasOtherSnapshotTag = tags.some((tag) =>
    [
      TASK_DEPENDS_ON_TAG,
      TASK_BASE_REF_TAG,
      TASK_EXECUTOR_TAG,
      TASK_AUTOMATION_TAG,
    ].includes(tag[0]),
  );
  if (stateValue === null) {
    if (hasOtherSnapshotTag) {
      throw new Error(
        `partial task-board snapshot is missing ${TASK_BOARD_STATE_TAG}`,
      );
    }
    return null;
  }

  const dependencies = [
    ...new Set(
      tags
        .filter((tag) => tag[0] === TASK_DEPENDS_ON_TAG)
        .map((tag) => normalizeHex64(tag[1] ?? "", "dependency")),
    ),
  ].sort();
  if (dependencies.length > 100) {
    throw new Error("invalid dependencies: maximum is 100");
  }
  const executorValue = singletonTag(tags, TASK_EXECUTOR_TAG);
  const baseRefValue = singletonTag(tags, TASK_BASE_REF_TAG);
  return {
    state: parseBoardState(stateValue),
    dependencies,
    baseRef: baseRefValue === null ? null : validateTaskBaseRef(baseRefValue),
    executor:
      executorValue === null ? null : normalizeHex64(executorValue, "executor"),
    automation: parseAutomation(singletonTag(tags, TASK_AUTOMATION_TAG)),
  };
}

export function buildTaskBoardSnapshotTags(
  snapshot: TaskBoardSnapshot,
): string[][] {
  const parsed = parseTaskBoardSnapshot([
    [TASK_BOARD_STATE_TAG, snapshot.state],
    ...snapshot.dependencies.map((dependency) => [
      TASK_DEPENDS_ON_TAG,
      dependency,
    ]),
    ...(snapshot.baseRef ? [[TASK_BASE_REF_TAG, snapshot.baseRef]] : []),
    ...(snapshot.executor ? [[TASK_EXECUTOR_TAG, snapshot.executor]] : []),
    [TASK_AUTOMATION_TAG, snapshot.automation],
  ]);
  if (!parsed) throw new Error("Task-board snapshot is required.");
  return [
    [TASK_BOARD_STATE_TAG, parsed.state],
    ...parsed.dependencies.map((dependency) => [
      TASK_DEPENDS_ON_TAG,
      dependency,
    ]),
    ...(parsed.baseRef ? [[TASK_BASE_REF_TAG, parsed.baseRef]] : []),
    ...(parsed.executor ? [[TASK_EXECUTOR_TAG, parsed.executor]] : []),
    [TASK_AUTOMATION_TAG, parsed.automation],
  ];
}

function repoOwner(issue: TaskBoardEvent): string | null {
  for (const address of tagValues(issue, "a")) {
    const match = /^30617:([0-9a-f]{64}):/i.exec(address);
    if (match) return match[1].toLowerCase();
  }
  return null;
}

function legacyState(kind: number, labels: string[]): TaskBoardState {
  if (kind === 1631) return "done";
  if (kind === 1632) return "closed";
  if (kind === 1633) return "triage";
  const normalized = new Set(labels.map((label) => label.toLowerCase()));
  if (normalized.has("in-review") || normalized.has("review")) return "review";
  if (normalized.has("in-progress") || normalized.has("active")) {
    return "in_progress";
  }
  if (normalized.has("triage")) return "triage";
  return "backlog";
}

export function projectTaskBoardIssue(
  issue: TaskBoardEvent,
  statuses: TaskBoardEvent[],
  maintainers: string[] = [],
): ProjectedTaskBoardIssue {
  const issueId = normalizeHex64(issue.id, "issue id");
  const authorized = new Set([
    normalizeHex64(issue.pubkey, "issue author"),
    ...maintainers.map((pubkey) => normalizeHex64(pubkey, "maintainer")),
  ]);
  const owner = repoOwner(issue);
  if (owner) authorized.add(owner);

  const latest = statuses
    .filter(
      (event) =>
        STATUS_KINDS.has(event.kind) &&
        HEX_64.test(event.pubkey) &&
        authorized.has(event.pubkey.toLowerCase()) &&
        tagValues(event, "e").some(
          (referenced) => referenced.toLowerCase() === issueId,
        ),
    )
    .sort(
      (left, right) =>
        right.created_at - left.created_at ||
        right.id.toLowerCase().localeCompare(left.id.toLowerCase()),
    )[0];

  const fallbackState = legacyState(
    latest?.kind ?? 1630,
    tagValues(issue, "t"),
  );
  const validationErrors: string[] = [];
  let snapshot: TaskBoardSnapshot = {
    state: fallbackState,
    dependencies: [],
    baseRef: null,
    executor: null,
    automation: "supervised",
  };
  if (latest) {
    try {
      const parsed = parseTaskBoardSnapshot(latest.tags);
      if (parsed) {
        snapshot = parsed;
        if (statusKindForState(parsed.state) !== latest.kind) {
          validationErrors.push(
            `board state ${parsed.state} conflicts with NIP-34 status kind ${latest.kind}`,
          );
        }
      }
    } catch (error) {
      validationErrors.push(
        error instanceof Error ? error.message : "Invalid task-board snapshot.",
      );
    }
  }

  return {
    issueId,
    snapshot,
    statusEventId: latest?.id.toLowerCase() ?? null,
    updatedAt: latest?.created_at ?? issue.created_at,
    validationErrors,
  };
}

export function analyzeTaskDependencies(
  issues: ProjectedTaskBoardIssue[],
): TaskDependencyAnalysis {
  const graph = new Map(
    issues.map((issue) => [issue.issueId, issue.snapshot.dependencies]),
  );
  const missing: Record<string, string[]> = {};
  for (const [issueId, dependencies] of graph) {
    const absent = dependencies.filter((dependency) => !graph.has(dependency));
    if (absent.length > 0) missing[issueId] = absent;
  }

  const visited = new Set<string>();
  const active: string[] = [];
  const cycleMembers = new Set<string>();
  const visit = (issueId: string) => {
    const cycleStart = active.indexOf(issueId);
    if (cycleStart >= 0) {
      for (const member of active.slice(cycleStart)) cycleMembers.add(member);
      return;
    }
    if (visited.has(issueId)) return;
    visited.add(issueId);
    active.push(issueId);
    for (const dependency of graph.get(issueId) ?? []) {
      if (graph.has(dependency)) visit(dependency);
    }
    active.pop();
  };
  for (const issueId of [...graph.keys()].sort()) visit(issueId);

  return { missing, cycleMembers: [...cycleMembers].sort() };
}

export function buildCodeTaskRequest(input: {
  prompt: string;
  baseRef: string;
  automation?: TaskAutomation;
}): CodeTaskRequestV1 {
  return {
    v: 1,
    operation: "code-task",
    prompt: validateText(input.prompt, "prompt", MAX_PROMPT_BYTES),
    baseRef: validateTaskBaseRef(input.baseRef),
    automation: parseAutomation(input.automation ?? "supervised"),
  };
}

export function parseCodeTaskRequest(content: string): CodeTaskRequestV1 {
  const value = JSON.parse(content) as Record<string, unknown>;
  const allowed = new Set([
    "v",
    "operation",
    "prompt",
    "baseRef",
    "automation",
  ]);
  if (Object.keys(value).some((key) => !allowed.has(key))) {
    throw new Error("invalid code-task request: unsupported field");
  }
  if (value.v !== 1 || value.operation !== "code-task") {
    throw new Error(
      "invalid code-task request: unsupported version or operation",
    );
  }
  if (
    typeof value.prompt !== "string" ||
    typeof value.baseRef !== "string" ||
    typeof value.automation !== "string"
  ) {
    throw new Error("invalid code-task request: malformed payload");
  }
  return buildCodeTaskRequest({
    prompt: value.prompt,
    baseRef: value.baseRef,
    automation: parseAutomation(value.automation),
  });
}

export function buildCodeTaskRequestTags(input: {
  issueId: string;
  repoAddress: string;
  projectAddress?: string | null;
  executor: string;
  channelId?: string | null;
}): string[][] {
  const tags = [
    ["e", normalizeHex64(input.issueId, "issue id"), "", "issue"],
    ["a", validateAddress(input.repoAddress, 30617, "repo address")],
    ["p", normalizeHex64(input.executor, "executor pubkey")],
    ["t", "code-task"],
  ];
  if (input.projectAddress) {
    tags.push([
      "a",
      validateAddress(input.projectAddress, 30621, "project address"),
    ]);
  }
  if (input.channelId) tags.push(["h", input.channelId]);
  return tags;
}

export function buildTaskJobUpdate(input: {
  phase: TaskJobPhase;
  summary: string;
}): TaskJobUpdateV1 {
  const phases: TaskJobPhase[] = [
    "accepted",
    "waiting_for_capacity",
    "waiting_for_model",
    "running",
    "review_ready",
    "cancelled",
    "failed",
    "completed",
  ];
  if (!phases.includes(input.phase)) {
    throw new Error(`invalid job phase: ${JSON.stringify(input.phase)}`);
  }
  return {
    v: 1,
    phase: input.phase,
    summary: validateText(input.summary, "summary", MAX_SUMMARY_BYTES),
  };
}

export function parseTaskJobUpdate(content: string): TaskJobUpdateV1 {
  const value = JSON.parse(content) as Record<string, unknown>;
  if (
    Object.keys(value).some(
      (key) => !["v", "phase", "summary"].includes(key),
    ) ||
    value.v !== 1 ||
    typeof value.phase !== "string" ||
    typeof value.summary !== "string"
  ) {
    throw new Error("invalid task-job update: malformed payload");
  }
  return buildTaskJobUpdate({
    phase: value.phase as TaskJobPhase,
    summary: value.summary,
  });
}

export function buildTaskJobLifecycleTags(input: {
  requestId: string;
  issueId: string;
  channelId?: string | null;
}): string[][] {
  const tags = [
    ["e", normalizeHex64(input.requestId, "request id"), "", "root"],
    ["e", normalizeHex64(input.issueId, "issue id"), "", "issue"],
  ];
  if (input.channelId) tags.push(["h", input.channelId]);
  return tags;
}

export function projectTaskJob(
  request: TaskBoardEvent,
  lifecycle: TaskBoardEvent[],
): ProjectedTaskJob {
  if (request.kind !== 43001) throw new Error("invalid job request kind");
  parseCodeTaskRequest(request.content ?? "");
  const requestId = normalizeHex64(request.id, "request id");
  const issueId = request.tags.find(
    (tag) => tag[0] === "e" && tag[3] === "issue",
  )?.[1];
  const executor = request.tags.find((tag) => tag[0] === "p")?.[1];
  if (!issueId || !executor) throw new Error("invalid job request references");
  const normalizedExecutor = normalizeHex64(executor, "executor pubkey");
  const candidates = lifecycle
    .filter(
      (event) =>
        JOB_UPDATE_KINDS.has(event.kind) &&
        event.pubkey.toLowerCase() === normalizedExecutor &&
        event.tags.some(
          (tag) =>
            tag[0] === "e" &&
            tag[1]?.toLowerCase() === requestId &&
            tag[3] === "root",
        ) &&
        event.tags.some(
          (tag) =>
            tag[0] === "e" &&
            tag[1]?.toLowerCase() === issueId.toLowerCase() &&
            tag[3] === "issue",
        ),
    )
    .sort(
      (left, right) =>
        right.created_at - left.created_at ||
        right.id.toLowerCase().localeCompare(left.id.toLowerCase()),
    );
  let phase: TaskJobPhase = "accepted";
  let summary = "Task request published.";
  let latest: TaskBoardEvent | undefined;
  for (const candidate of candidates) {
    try {
      const parsed = parseTaskJobUpdate(candidate.content ?? "");
      phase = parsed.phase;
      summary = parsed.summary;
      latest = candidate;
      break;
    } catch {
      // Ignore malformed lifecycle events and retain the latest valid update.
    }
  }
  return {
    requestId,
    issueId: normalizeHex64(issueId, "issue id"),
    executor: normalizedExecutor,
    phase,
    summary,
    updatedAt: latest?.created_at ?? request.created_at,
    terminalEventId: latest?.id.toLowerCase() ?? null,
  };
}

export function projectIssueStatusLabel(
  state: TaskBoardState,
): "Triage" | "Backlog" | "In Progress" | "In Review" | "Done" | "Closed" {
  if (state === "triage") return "Triage";
  if (state === "backlog") return "Backlog";
  if (state === "in_progress") return "In Progress";
  if (state === "review") return "In Review";
  if (state === "done") return "Done";
  return "Closed";
}

export function statusKindForTaskBoardState(state: TaskBoardState): number {
  return statusKindForState(state);
}
