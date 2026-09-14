import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo } from "react";

import { signNostrEvent } from "@/shared/lib/nostr-signer";
import type { Repo } from "@/features/repos/use-repos";
import {
  analyzeTaskDependencies,
  buildCodeTaskRequest,
  buildCodeTaskRequestTags,
  buildTaskBoardSnapshotTags,
  buildTaskJobLifecycleTags,
  projectTaskBoardIssue,
  projectTaskJob,
  type ProjectedTaskJob,
  statusKindForTaskBoardState,
  type ProjectedTaskBoardIssue,
  type TaskBoardEvent,
  type TaskBoardSnapshot,
  type TaskBoardState,
} from "./protocol";
import {
  publishTaskBoardEvent,
  queryTaskBoardEvents,
  queryTaskJobEvents,
  subscribeToTaskBoard,
} from "./relay";

export type WebTaskBoardItem = {
  event: TaskBoardEvent & { content: string };
  projection: ProjectedTaskBoardIssue;
  title: string;
  job?: ProjectedTaskJob;
};

function repoAddress(repo: Repo): string {
  return `30617:${repo.owner.toLowerCase()}:${repo.id}`;
}

function taskTitle(event: TaskBoardEvent & { content: string }): string {
  return (
    event.tags.find((tag) => tag[0] === "subject")?.[1] ||
    event.content.split("\n")[0] ||
    "Untitled task"
  );
}

async function fetchTaskBoard(repo: Repo): Promise<WebTaskBoardItem[]> {
  const events = await queryTaskBoardEvents(repoAddress(repo));
  const statuses = events.filter(
    (event) => event.kind >= 1630 && event.kind <= 1633,
  );
  const items = events
    .filter((event) => event.kind === 1621)
    .map((event) => ({
      event,
      projection: projectTaskBoardIssue(event, statuses),
      title: taskTitle(event),
    }))
    .sort(
      (left, right) =>
        right.projection.updatedAt - left.projection.updatedAt ||
        left.title.localeCompare(right.title),
    );
  const jobs = await queryTaskJobEvents(items.map((item) => item.event.id));
  return items.map((item) => {
    const allowed = new Set([
      item.event.pubkey.toLowerCase(),
      repo.owner.toLowerCase(),
    ]);
    const requests = jobs
      .filter(
        (event) =>
          event.kind === 43001 &&
          allowed.has(event.pubkey.toLowerCase()) &&
          event.tags.some(
            (tag) =>
              tag[0] === "e" &&
              tag[1]?.toLowerCase() === item.event.id.toLowerCase() &&
              tag[3] === "issue",
          ),
      )
      .sort(
        (left, right) =>
          right.created_at - left.created_at ||
          right.id.toLowerCase().localeCompare(left.id.toLowerCase()),
      );
    for (const request of requests) {
      try {
        return { ...item, job: projectTaskJob(request, jobs) };
      } catch {
        // Fall through to older valid requests.
      }
    }
    return item;
  });
}

export function useTaskBoard(repo: Repo | undefined) {
  const queryClient = useQueryClient();
  const address = repo ? repoAddress(repo) : null;
  const query = useQuery({
    enabled: Boolean(repo),
    queryKey: ["task-board", address],
    queryFn: () => {
      if (!repo) throw new Error("Repository not found.");
      return fetchTaskBoard(repo);
    },
    staleTime: 30_000,
  });

  useEffect(() => {
    if (!address) return undefined;
    return subscribeToTaskBoard(
      address,
      query.data?.map((item) => item.event.id) ?? [],
      () => {
        void queryClient.invalidateQueries({
          queryKey: ["task-board", address],
        });
      },
    );
  }, [address, query.data, queryClient]);

  const dependencyAnalysis = useMemo(
    () =>
      analyzeTaskDependencies(query.data?.map((item) => item.projection) ?? []),
    [query.data],
  );

  const mutation = useMutation({
    mutationFn: async ({
      item,
      snapshot,
    }: {
      item: WebTaskBoardItem;
      snapshot: TaskBoardSnapshot;
    }) => {
      if (!repo || !address) throw new Error("Repository not found.");
      const event = await signNostrEvent(
        {
          kind: statusKindForTaskBoardState(snapshot.state),
          content: `Moved to ${snapshot.state.replace(/_/g, " ")}`,
          tags: [
            ["e", item.event.id, "", "root"],
            ["a", address],
            ["p", repo.owner.toLowerCase()],
            ...buildTaskBoardSnapshotTags(snapshot),
          ],
        },
        { requireNip07: true },
      );
      const allowed = new Set([
        item.event.pubkey.toLowerCase(),
        repo.owner.toLowerCase(),
      ]);
      if (!allowed.has(event.pubkey.toLowerCase())) {
        throw new Error(
          "Only the task author or repository owner can move this task.",
        );
      }
      await publishTaskBoardEvent(event);
    },
    onSuccess: async () => {
      await queryClient.invalidateQueries({
        queryKey: ["task-board", address],
      });
    },
  });

  const move = (item: WebTaskBoardItem, state: TaskBoardState) =>
    mutation.mutateAsync({
      item,
      snapshot: { ...item.projection.snapshot, state },
    });

  const setDependencies = (item: WebTaskBoardItem, dependencies: string[]) => {
    const candidate = (query.data ?? []).map((current) =>
      current.event.id === item.event.id
        ? {
            ...current.projection,
            snapshot: { ...current.projection.snapshot, dependencies },
          }
        : current.projection,
    );
    const analysis = analyzeTaskDependencies(candidate);
    if (analysis.cycleMembers.includes(item.event.id)) {
      return Promise.reject(new Error("That dependency would create a cycle."));
    }
    return mutation.mutateAsync({
      item,
      snapshot: { ...item.projection.snapshot, dependencies },
    });
  };

  const start = async (item: WebTaskBoardItem, snapshot: TaskBoardSnapshot) => {
    if (!repo || !address || !repo.channelId) {
      throw new Error(
        "Bind this repository to a channel before starting a task.",
      );
    }
    if (!snapshot.executor) throw new Error("Select an executor first.");
    if (snapshot.automation !== "supervised") {
      throw new Error("Task-board v1 starts supervised runs only.");
    }
    const analysis = analyzeTaskDependencies(
      (query.data ?? []).map((current) =>
        current.event.id === item.event.id
          ? { ...current.projection, snapshot }
          : current.projection,
      ),
    );
    if (
      analysis.cycleMembers.includes(item.event.id) ||
      analysis.missing[item.event.id]?.length
    ) {
      throw new Error(
        "Resolve this task's dependency errors before starting it.",
      );
    }
    await mutation.mutateAsync({
      item,
      snapshot: { ...snapshot, state: "in_progress" },
    });
    const request = buildCodeTaskRequest({
      automation: "supervised",
      baseRef: snapshot.baseRef ?? "main",
      prompt: [
        `Implement task: ${item.title}`,
        item.event.content.trim(),
        "This is a supervised run. Work in an isolated Git worktree and leave a tested diff for human review. Do not commit, push, open a pull request, merge, resolve the issue, or remove the worktree.",
      ]
        .filter(Boolean)
        .join("\n\n"),
    });
    const event = await signNostrEvent(
      {
        kind: 43001,
        content: JSON.stringify(request),
        tags: buildCodeTaskRequestTags({
          channelId: repo.channelId,
          executor: snapshot.executor,
          issueId: item.event.id,
          repoAddress: address,
        }),
      },
      { requireNip07: true },
    );
    const allowed = new Set([
      item.event.pubkey.toLowerCase(),
      repo.owner.toLowerCase(),
    ]);
    if (!allowed.has(event.pubkey.toLowerCase())) {
      throw new Error(
        "Only the task author or repository owner can start this task.",
      );
    }
    await publishTaskBoardEvent(event);
    await queryClient.invalidateQueries({ queryKey: ["task-board", address] });
  };

  const cancel = async (item: WebTaskBoardItem) => {
    if (!repo || !address || !repo.channelId || !item.job) {
      throw new Error("This task does not have an active executor request.");
    }
    const event = await signNostrEvent(
      {
        kind: 43005,
        content: JSON.stringify({
          v: 1,
          operation: "cancel",
          summary: "Cancellation requested by the task owner.",
        }),
        tags: [
          ...buildTaskJobLifecycleTags({
            channelId: repo.channelId,
            issueId: item.event.id,
            requestId: item.job.requestId,
          }),
          ["p", item.job.executor],
        ],
      },
      { requireNip07: true },
    );
    const allowed = new Set([
      item.event.pubkey.toLowerCase(),
      repo.owner.toLowerCase(),
    ]);
    if (!allowed.has(event.pubkey.toLowerCase())) {
      throw new Error(
        "Only the task author or repository owner can cancel this task.",
      );
    }
    await publishTaskBoardEvent(event);
    await queryClient.invalidateQueries({ queryKey: ["task-board", address] });
  };

  return {
    query,
    dependencyAnalysis,
    move,
    setDependencies,
    start,
    cancel,
    isSaving: mutation.isPending,
  };
}
