import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import * as React from "react";

import type {
  Project,
  ProjectIssue,
  Repository,
} from "@/features/projects/hooks";
import { useProjectsWorkItemsQuery } from "@/features/projects/hooks";
import { buildGitStatusTags } from "@/features/projects/projectIssues.mjs";
import { relayClient } from "@/shared/api/relayClient";
import { signRelayEvent } from "@/shared/api/tauri";
import {
  KIND_GIT_STATUS_CLOSED,
  KIND_GIT_STATUS_DRAFT,
  KIND_GIT_STATUS_MERGED,
  KIND_GIT_STATUS_OPEN,
} from "@/shared/constants/kinds";
import {
  analyzeTaskDependencies,
  buildTaskBoardSnapshotTags,
  statusKindForTaskBoardState,
  type TaskAutomation,
  type TaskBoardState,
  projectTaskJob,
  type ProjectedTaskJob,
} from "./protocol";

export type TaskBoardItem = {
  issue: ProjectIssue;
  project: Project;
  repository: Repository;
};

export type TaskBoardUpdate = {
  automation: TaskAutomation;
  baseRef: string | null;
  dependencies: string[];
  executor: string | null;
  state: TaskBoardState;
};

const STATUS_KINDS = [
  KIND_GIT_STATUS_OPEN,
  KIND_GIT_STATUS_MERGED,
  KIND_GIT_STATUS_CLOSED,
  KIND_GIT_STATUS_DRAFT,
];
const JOB_KINDS = [43001, 43002, 43003, 43004, 43005, 43006];

function latestJobsByIssue(
  items: TaskBoardItem[],
  events: Array<Parameters<typeof projectTaskJob>[0]>,
): Record<string, ProjectedTaskJob> {
  const jobs: Record<string, ProjectedTaskJob> = {};
  for (const item of items) {
    const allowed = new Set(
      [
        item.issue.author,
        item.repository.owner,
        ...(item.repository.maintainers ?? []),
      ].map((pubkey) => pubkey.toLowerCase()),
    );
    const requests = events
      .filter(
        (event) =>
          event.kind === 43001 &&
          allowed.has(event.pubkey.toLowerCase()) &&
          event.tags.some(
            (tag) =>
              tag[0] === "e" &&
              tag[1]?.toLowerCase() === item.issue.id.toLowerCase() &&
              tag[3] === "issue",
          ) &&
          event.tags.some(
            (tag) => tag[0] === "a" && tag[1] === item.repository.repoAddress,
          ),
      )
      .sort(
        (left, right) =>
          right.created_at - left.created_at ||
          right.id.toLowerCase().localeCompare(left.id.toLowerCase()),
      );
    for (const request of requests) {
      try {
        jobs[item.issue.id] = projectTaskJob(request, events);
        break;
      } catch {
        // Ignore malformed job history and continue to an older valid request.
      }
    }
  }
  return jobs;
}

function canUpdateTask(
  item: TaskBoardItem,
  currentPubkey: string | null | undefined,
): boolean {
  const actor = currentPubkey?.toLowerCase();
  if (!actor) return false;
  return [
    item.issue.author,
    item.repository.owner,
    ...(item.repository.maintainers ?? []),
  ]
    .map((pubkey) => pubkey.toLowerCase())
    .includes(actor);
}

async function publishTaskBoardUpdate(
  item: TaskBoardItem,
  update: TaskBoardUpdate,
  currentPubkey: string | null | undefined,
) {
  if (!canUpdateTask(item, currentPubkey)) {
    throw new Error(
      "Only the task author, repository owner, or a maintainer can update this task.",
    );
  }
  if (item.issue.boardValidationErrors.length > 0) {
    throw new Error(
      "Repair the malformed task-board snapshot before updating it.",
    );
  }
  const event = await signRelayEvent({
    kind: statusKindForTaskBoardState(update.state),
    content: "",
    tags: [
      ...buildGitStatusTags({
        issueId: item.issue.id,
        repoAddress: item.repository.repoAddress,
        repoOwner: item.issue.author,
      }),
      ...(item.repository.owner.toLowerCase() !==
      item.issue.author.toLowerCase()
        ? [["p", item.repository.owner.toLowerCase()]]
        : []),
      ...buildTaskBoardSnapshotTags(update),
    ],
  });
  await relayClient.publishEvent(
    event,
    "Timed out updating task board.",
    "Failed to update task board.",
  );
}

export function useTaskBoard(
  project: Project | null | undefined,
  currentPubkey: string | null | undefined,
) {
  const queryClient = useQueryClient();
  const workItemsQuery = useProjectsWorkItemsQuery(project ? [project] : []);
  const items = React.useMemo(
    () => workItemsQuery.data?.issues.items ?? [],
    [workItemsQuery.data?.issues.items],
  );
  const dependencyAnalysis = React.useMemo(
    () =>
      analyzeTaskDependencies(
        items.map(({ issue }) => ({
          issueId: issue.id,
          snapshot: {
            state:
              issue.status === "Triage"
                ? "triage"
                : issue.status === "Backlog"
                  ? "backlog"
                  : issue.status === "In Progress"
                    ? "in_progress"
                    : issue.status === "In Review"
                      ? "review"
                      : issue.status === "Done"
                        ? "done"
                        : "closed",
            dependencies: issue.dependencies,
            baseRef: issue.baseRef,
            executor: issue.executor,
            automation: issue.automation,
          },
          statusEventId: issue.statusEventId,
          updatedAt: issue.updatedAt,
          validationErrors: issue.boardValidationErrors,
        })),
      ),
    [items],
  );
  const issueIds = React.useMemo(
    () => items.map((item) => item.issue.id).sort(),
    [items],
  );
  const jobsQuery = useQuery({
    enabled: issueIds.length > 0,
    queryKey: ["task-board", "jobs", project?.id ?? "none", issueIds],
    queryFn: () =>
      relayClient.fetchEvents({
        kinds: JOB_KINDS,
        "#e": issueIds,
        limit: 2_000,
      }),
    staleTime: 10_000,
  });
  const jobsByIssue = React.useMemo(
    () => latestJobsByIssue(items, jobsQuery.data ?? []),
    [items, jobsQuery.data],
  );

  React.useEffect(() => {
    if (!project || project.repositoryAddresses.length === 0) return;
    let disposed = false;
    let unsubscribe: (() => void | Promise<void>) | undefined;
    void relayClient
      .subscribeLive(
        {
          kinds: [1621, ...STATUS_KINDS],
          "#a": project.repositoryAddresses,
          since: Math.floor(Date.now() / 1_000),
          limit: 100,
        },
        () => {
          void queryClient.invalidateQueries({
            queryKey: ["projects", "work-items"],
          });
        },
      )
      .then((nextUnsubscribe) => {
        if (disposed) {
          void nextUnsubscribe();
        } else {
          unsubscribe = nextUnsubscribe;
        }
      });
    return () => {
      disposed = true;
      void unsubscribe?.();
    };
  }, [project, queryClient]);

  React.useEffect(() => {
    if (issueIds.length === 0) return;
    let disposed = false;
    let unsubscribe: (() => void | Promise<void>) | undefined;
    void relayClient
      .subscribeLive(
        {
          kinds: JOB_KINDS,
          "#e": issueIds,
          since: Math.floor(Date.now() / 1_000),
          limit: 100,
        },
        () => {
          void queryClient.invalidateQueries({
            queryKey: ["task-board", "jobs", project?.id ?? "none"],
          });
        },
      )
      .then((nextUnsubscribe) => {
        if (disposed) void nextUnsubscribe();
        else unsubscribe = nextUnsubscribe;
      });
    return () => {
      disposed = true;
      void unsubscribe?.();
    };
  }, [issueIds, project?.id, queryClient]);

  const mutation = useMutation({
    mutationFn: ({
      item,
      update,
    }: {
      item: TaskBoardItem;
      update: TaskBoardUpdate;
    }) => publishTaskBoardUpdate(item, update, currentPubkey),
    onSuccess: async () => {
      await queryClient.invalidateQueries({
        queryKey: ["projects", "work-items"],
      });
    },
  });

  return {
    canUpdate: (item: TaskBoardItem) => canUpdateTask(item, currentPubkey),
    dependencyAnalysis,
    items,
    jobsByIssue,
    jobsQuery,
    mutation,
    query: workItemsQuery,
  };
}
