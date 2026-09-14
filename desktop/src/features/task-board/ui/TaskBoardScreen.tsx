import { Link } from "@tanstack/react-router";
import * as React from "react";
import { ArrowLeft, Plus } from "lucide-react";
import { toast } from "sonner";

import { useProjectQuery, type ProjectIssue } from "@/features/projects/hooks";
import { useManagedAgentsQuery } from "@/features/agents/hooks";
import { useCommunities } from "@/features/communities/useCommunities";
import { publishProjectIssue } from "@/features/projects/issueMutations";
import {
  type TaskBoardItem,
  type TaskBoardUpdate,
  useTaskBoard,
} from "@/features/task-board/hooks";
import {
  analyzeTaskDependencies,
  type ProjectedTaskJob,
  type TaskAutomation,
  type TaskBoardState,
} from "@/features/task-board/protocol";
import {
  cancelManagedCodeTask,
  managedAgentTaskCapabilities,
  startManagedCodeTask,
} from "@/features/task-board/managedAgentExecutor";
import { useIdentityQuery } from "@/shared/api/hooks";
import type { ManagedAgent } from "@/shared/api/types";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { ViewLoadingFallback } from "@/shared/ui/ViewLoadingFallback";

const COLUMNS: Array<{ state: TaskBoardState; title: string }> = [
  { state: "triage", title: "Triage" },
  { state: "backlog", title: "Backlog" },
  { state: "in_progress", title: "In progress" },
  { state: "review", title: "Review" },
  { state: "done", title: "Done" },
  { state: "closed", title: "Closed" },
];

function stateForIssue(issue: ProjectIssue): TaskBoardState {
  if (issue.status === "Triage") return "triage";
  if (issue.status === "In Progress") return "in_progress";
  if (issue.status === "In Review") return "review";
  if (issue.status === "Done") return "done";
  if (issue.status === "Closed") return "closed";
  return "backlog";
}

function updateFromIssue(
  issue: ProjectIssue,
  patch: Partial<TaskBoardUpdate> = {},
): TaskBoardUpdate {
  return {
    state: stateForIssue(issue),
    dependencies: issue.dependencies,
    baseRef: issue.baseRef,
    executor: issue.executor,
    automation: issue.automation,
    ...patch,
  };
}

function TaskCard({
  allItems,
  canUpdate,
  item,
  job,
  managedAgents,
  onStart,
  onCancel,
  onUpdate,
}: {
  allItems: TaskBoardItem[];
  canUpdate: boolean;
  item: TaskBoardItem;
  job: ProjectedTaskJob | undefined;
  managedAgents: ManagedAgent[];
  onStart: (item: TaskBoardItem, update: TaskBoardUpdate) => Promise<void>;
  onCancel: (item: TaskBoardItem, job: ProjectedTaskJob) => Promise<void>;
  onUpdate: (item: TaskBoardItem, update: TaskBoardUpdate) => Promise<void>;
}) {
  const { issue } = item;
  const [executor, setExecutor] = React.useState(issue.executor ?? "");
  const [baseRef, setBaseRef] = React.useState(issue.baseRef ?? "");
  const [automation, setAutomation] = React.useState<TaskAutomation>(
    issue.automation,
  );
  React.useEffect(() => setExecutor(issue.executor ?? ""), [issue.executor]);
  React.useEffect(() => setBaseRef(issue.baseRef ?? ""), [issue.baseRef]);
  React.useEffect(() => setAutomation(issue.automation), [issue.automation]);

  const saveSettings = () =>
    onUpdate(
      item,
      updateFromIssue(issue, {
        automation,
        baseRef: baseRef.trim() || null,
        executor: executor.trim() || null,
      }),
    );
  const selectedAgent = managedAgents.find(
    (agent) => agent.pubkey.toLowerCase() === executor.trim().toLowerCase(),
  );
  const executionBlocked =
    !canUpdate ||
    issue.boardValidationErrors.length > 0 ||
    !item.repository.channelId ||
    !selectedAgent ||
    !managedAgentTaskCapabilities(selectedAgent).supportsTools ||
    automation !== "supervised";

  return (
    <article
      className="rounded-lg border border-border/60 bg-background p-3 shadow-xs"
      draggable={canUpdate}
      onDragStart={(event) => {
        event.dataTransfer.setData("text/plain", issue.id);
        event.dataTransfer.effectAllowed = "move";
      }}
    >
      <h3 className="text-sm font-medium leading-5">{issue.title}</h3>
      <p className="mt-1 truncate text-xs text-muted-foreground">
        {item.repository.name} · {issue.id.slice(0, 8)}
      </p>
      {issue.boardValidationErrors.length > 0 ? (
        <p className="mt-2 text-xs text-destructive">
          Invalid snapshot: {issue.boardValidationErrors.join("; ")}
        </p>
      ) : null}
      <details className="mt-2 text-xs">
        <summary className="cursor-pointer text-muted-foreground">
          Dependencies and execution
        </summary>
        <div className="mt-2 space-y-2">
          <fieldset disabled={!canUpdate}>
            <legend className="mb-1 font-medium">Depends on</legend>
            <div className="max-h-28 space-y-1 overflow-y-auto">
              {allItems
                .filter((candidate) => candidate.issue.id !== issue.id)
                .map((candidate) => {
                  const checked = issue.dependencies.includes(
                    candidate.issue.id,
                  );
                  return (
                    <label
                      className="flex items-start gap-2"
                      key={candidate.issue.id}
                    >
                      <input
                        checked={checked}
                        onChange={() => {
                          const dependencies = checked
                            ? issue.dependencies.filter(
                                (dependency) =>
                                  dependency !== candidate.issue.id,
                              )
                            : [...issue.dependencies, candidate.issue.id];
                          const projected = allItems.map((entry) => ({
                            issueId: entry.issue.id,
                            snapshot: updateFromIssue(
                              entry.issue,
                              entry.issue.id === issue.id
                                ? { dependencies }
                                : {},
                            ),
                            statusEventId: entry.issue.statusEventId,
                            updatedAt: entry.issue.updatedAt,
                            validationErrors: entry.issue.boardValidationErrors,
                          }));
                          const analysis = analyzeTaskDependencies(projected);
                          if (analysis.cycleMembers.length > 0) {
                            toast.error(
                              "That dependency would create a cycle.",
                            );
                            return;
                          }
                          void onUpdate(
                            item,
                            updateFromIssue(issue, { dependencies }),
                          );
                        }}
                        type="checkbox"
                      />
                      <span>{candidate.issue.title}</span>
                    </label>
                  );
                })}
            </div>
          </fieldset>
          <label className="block" htmlFor={`base-ref-${issue.id}`}>
            <span className="mb-1 block font-medium">Base ref</span>
            <Input
              disabled={!canUpdate}
              id={`base-ref-${issue.id}`}
              onChange={(event) => setBaseRef(event.target.value)}
              placeholder="main"
              value={baseRef}
            />
          </label>
          <label className="block" htmlFor={`executor-${issue.id}`}>
            <span className="mb-1 block font-medium">Desktop executor</span>
            <select
              className="h-8 w-full rounded-md border border-input bg-background px-2"
              disabled={!canUpdate}
              id={`executor-${issue.id}`}
              onChange={(event) => setExecutor(event.target.value)}
              value={executor}
            >
              <option value="">Not selected</option>
              {managedAgents
                .filter((agent) => agent.backend.type === "local")
                .map((agent) => (
                  <option key={agent.pubkey} value={agent.pubkey}>
                    {agent.name}
                  </option>
                ))}
            </select>
          </label>
          <label className="block">
            <span className="mb-1 block font-medium">Automation ceiling</span>
            <select
              className="h-8 w-full rounded-md border border-input bg-background px-2"
              disabled={!canUpdate}
              onChange={(event) =>
                setAutomation(event.target.value as TaskAutomation)
              }
              value={automation}
            >
              <option value="supervised">Supervised</option>
              <option value="auto_commit">May commit</option>
              <option value="propose_pr">May propose review</option>
            </select>
          </label>
          <div className="flex flex-wrap gap-2">
            <Button
              disabled={!canUpdate}
              onClick={() => void saveSettings()}
              size="xs"
              type="button"
              variant="outline"
            >
              Save settings
            </Button>
            <Button
              disabled={executionBlocked}
              onClick={() =>
                void onStart(
                  item,
                  updateFromIssue(issue, {
                    automation,
                    baseRef: baseRef.trim() || item.repository.defaultBranch,
                    executor: executor.trim() || null,
                    state: "in_progress",
                  }),
                )
              }
              size="xs"
              type="button"
            >
              Start supervised run
            </Button>
          </div>
          {!item.repository.channelId ? (
            <p className="text-muted-foreground">
              Bind this repository to a channel before starting an executor.
            </p>
          ) : null}
          {job ? (
            <div className="rounded-md bg-muted p-2" aria-live="polite">
              <p className="font-medium">{job.phase.replaceAll("_", " ")}</p>
              <p className="mt-0.5 text-muted-foreground">{job.summary}</p>
              {job.phase !== "cancelled" &&
              job.phase !== "failed" &&
              job.phase !== "completed" ? (
                <Button
                  className="mt-2"
                  disabled={!canUpdate}
                  onClick={() => void onCancel(item, job)}
                  size="xs"
                  type="button"
                  variant="outline"
                >
                  Cancel task
                </Button>
              ) : null}
            </div>
          ) : null}
        </div>
      </details>
    </article>
  );
}

export function TaskBoardScreen({ projectId }: { projectId: string }) {
  const projectQuery = useProjectQuery(projectId);
  const identityQuery = useIdentityQuery();
  const managedAgentsQuery = useManagedAgentsQuery();
  const { activeCommunity } = useCommunities();
  const project = projectQuery.data;
  const board = useTaskBoard(project, identityQuery.data?.pubkey);
  const [title, setTitle] = React.useState("");
  const [creating, setCreating] = React.useState(false);

  if (projectQuery.isLoading) return <ViewLoadingFallback kind="projects" />;
  if (!project) {
    return <p className="p-6 text-sm text-destructive">Project not found.</p>;
  }

  const update = async (item: TaskBoardItem, next: TaskBoardUpdate) => {
    try {
      await board.mutation.mutateAsync({ item, update: next });
    } catch (error) {
      toast.error(
        error instanceof Error ? error.message : "Task update failed.",
      );
    }
  };
  const createTask = async () => {
    const repository = project.repositories[0];
    if (!repository || !title.trim()) return;
    setCreating(true);
    try {
      await publishProjectIssue(repository, {
        title,
        body: "",
        category: "issue",
      });
      setTitle("");
      await board.query.refetch();
      toast.success("Task created.");
    } catch (error) {
      toast.error(
        error instanceof Error ? error.message : "Task creation failed.",
      );
    } finally {
      setCreating(false);
    }
  };
  const startTask = async (item: TaskBoardItem, next: TaskBoardUpdate) => {
    const executor = next.executor;
    const identity = identityQuery.data?.pubkey;
    const relayUrl = activeCommunity?.relayUrl;
    const channelId = item.repository.channelId;
    const agent = managedAgentsQuery.data?.find(
      (candidate) => candidate.pubkey.toLowerCase() === executor?.toLowerCase(),
    );
    if (!executor || !identity || !relayUrl || !channelId || !agent) {
      toast.error("The selected Desktop executor is not available.");
      return;
    }
    if (
      board.dependencyAnalysis.cycleMembers.includes(item.issue.id) ||
      board.dependencyAnalysis.missing[item.issue.id]?.length
    ) {
      toast.error("Resolve this task's dependency errors before starting it.");
      return;
    }
    try {
      await board.mutation.mutateAsync({ item, update: next });
      await startManagedCodeTask({
        agent,
        automation: next.automation,
        baseRef: next.baseRef ?? item.repository.defaultBranch,
        channelId,
        executor,
        expectedRelayUrl: relayUrl,
        expectedSignerPubkey: identity,
        issueBody: item.issue.content,
        issueId: item.issue.id,
        issueTitle: item.issue.title,
        projectAddress: item.project.projectAddress,
        repoAddress: item.repository.repoAddress,
      });
      await board.jobsQuery.refetch();
      toast.success("Supervised task sent to the executor.");
    } catch (error) {
      toast.error(
        error instanceof Error ? error.message : "Could not start the task.",
      );
    }
  };
  const cancelTask = async (item: TaskBoardItem, job: ProjectedTaskJob) => {
    const channelId = item.repository.channelId;
    if (!channelId) return;
    try {
      await cancelManagedCodeTask({
        channelId,
        executor: job.executor,
        issueId: item.issue.id,
        requestId: job.requestId,
      });
      await board.jobsQuery.refetch();
      toast.success("Cancellation sent to the executor.");
    } catch (error) {
      toast.error(
        error instanceof Error ? error.message : "Could not cancel task.",
      );
    }
  };

  return (
    <main className="flex min-h-0 flex-1 flex-col overflow-hidden p-4">
      <header className="mb-4 flex flex-wrap items-center justify-between gap-3">
        <div>
          <Link
            className="mb-1 inline-flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground"
            params={{ projectId }}
            search={{
              commitHash: undefined,
              issueId: undefined,
              pullRequestId: undefined,
              repositoryId: undefined,
              tab: undefined,
            }}
            to="/projects/$projectId"
          >
            <ArrowLeft className="h-3.5 w-3.5" />
            {project.name}
          </Link>
          <h1 className="text-xl font-semibold">Task board</h1>
        </div>
        <div className="flex items-center gap-2">
          <Input
            aria-label="Task title"
            className="w-64"
            onChange={(event) => setTitle(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") void createTask();
            }}
            placeholder="New task"
            value={title}
          />
          <Button
            disabled={
              creating || !title.trim() || project.repositories.length === 0
            }
            onClick={() => void createTask()}
            size="sm"
          >
            <Plus />
            Add
          </Button>
        </div>
      </header>
      {board.dependencyAnalysis.cycleMembers.length > 0 ? (
        <p className="mb-3 rounded-lg border border-destructive/40 bg-destructive/10 p-2 text-xs text-destructive">
          Dependency cycle detected. Execution is blocked until the linked tasks
          are corrected.
        </p>
      ) : null}
      {board.query.isError ? (
        <p className="text-sm text-destructive">Could not load task board.</p>
      ) : (
        <div className="grid min-h-0 flex-1 grid-cols-6 gap-3 overflow-x-auto pb-2">
          {COLUMNS.map((column) => {
            const items = board.items.filter(
              ({ issue }) => stateForIssue(issue) === column.state,
            );
            return (
              <section
                aria-label={`${column.title} tasks`}
                className="min-w-56 rounded-xl bg-muted/30 p-2"
                key={column.state}
                onDragOver={(event) => event.preventDefault()}
                onDrop={(event) => {
                  const issueId = event.dataTransfer.getData("text/plain");
                  const item = board.items.find(
                    (candidate) => candidate.issue.id === issueId,
                  );
                  if (item && board.canUpdate(item)) {
                    void update(
                      item,
                      updateFromIssue(item.issue, { state: column.state }),
                    );
                  }
                }}
              >
                <h2 className="mb-2 flex items-center justify-between px-1 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                  {column.title}
                  <span>{items.length}</span>
                </h2>
                <div className="space-y-2">
                  {items.map((item) => (
                    <TaskCard
                      allItems={board.items}
                      canUpdate={board.canUpdate(item)}
                      item={item}
                      job={board.jobsByIssue[item.issue.id]}
                      key={item.issue.id}
                      managedAgents={managedAgentsQuery.data ?? []}
                      onStart={startTask}
                      onCancel={cancelTask}
                      onUpdate={update}
                    />
                  ))}
                </div>
              </section>
            );
          })}
        </div>
      )}
    </main>
  );
}
