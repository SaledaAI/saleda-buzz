import { ArrowLeft, Columns3, Link2, Play } from "lucide-react";
import { Link } from "@tanstack/react-router";
import { useEffect, useState } from "react";
import { toast } from "sonner";

import { useRepo } from "@/features/repos/use-repos";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { useTaskBoard, type WebTaskBoardItem } from "../hooks";
import type { TaskBoardSnapshot, TaskBoardState } from "../protocol";

const COLUMNS: Array<{ state: TaskBoardState; label: string }> = [
  { state: "triage", label: "Triage" },
  { state: "backlog", label: "Backlog" },
  { state: "in_progress", label: "In progress" },
  { state: "review", label: "Review" },
  { state: "done", label: "Done" },
  { state: "closed", label: "Closed" },
];

function TaskCard({
  item,
  items,
  isSaving,
  onMove,
  onSetDependencies,
  onStart,
  onCancel,
}: {
  item: WebTaskBoardItem;
  items: WebTaskBoardItem[];
  isSaving: boolean;
  onMove: (state: TaskBoardState) => Promise<void>;
  onSetDependencies: (dependencies: string[]) => Promise<void>;
  onStart: (snapshot: TaskBoardSnapshot) => Promise<void>;
  onCancel: () => Promise<void>;
}) {
  const dependencies = new Set(item.projection.snapshot.dependencies);
  const [executor, setExecutor] = useState(
    item.projection.snapshot.executor ?? "",
  );
  const [baseRef, setBaseRef] = useState(
    item.projection.snapshot.baseRef ?? "main",
  );
  useEffect(
    () => setExecutor(item.projection.snapshot.executor ?? ""),
    [item.projection.snapshot.executor],
  );
  useEffect(
    () => setBaseRef(item.projection.snapshot.baseRef ?? "main"),
    [item.projection.snapshot.baseRef],
  );
  const run = async (operation: () => Promise<void>) => {
    try {
      await operation();
    } catch (error) {
      toast.error(
        error instanceof Error ? error.message : "Task update failed.",
      );
    }
  };

  return (
    <article className="rounded-lg border border-black/10 bg-white p-3 shadow-xs dark:border-white/10 dark:bg-white/5">
      <h3 className="text-sm font-medium text-black dark:text-white">
        {item.title}
      </h3>
      <p className="mt-1 font-mono text-xs text-black/45 dark:text-white/45">
        #{item.event.id.slice(0, 8)}
      </p>
      <label className="mt-3 block text-xs text-black/55 dark:text-white/55">
        Status
        <select
          className="mt-1 w-full rounded-md border border-black/10 bg-transparent px-2 py-1.5 text-sm dark:border-white/10"
          disabled={isSaving}
          onChange={(event) =>
            void run(() => onMove(event.target.value as TaskBoardState))
          }
          value={item.projection.snapshot.state}
        >
          {COLUMNS.map((column) => (
            <option key={column.state} value={column.state}>
              {column.label}
            </option>
          ))}
        </select>
      </label>
      {items.length > 1 ? (
        <details className="mt-3 text-xs text-black/60 dark:text-white/60">
          <summary className="flex cursor-pointer items-center gap-1">
            <Link2 className="h-3 w-3" /> Dependencies ({dependencies.size})
          </summary>
          <div className="mt-2 space-y-1.5">
            {items
              .filter((candidate) => candidate.event.id !== item.event.id)
              .map((candidate) => (
                <label
                  className="flex items-start gap-2"
                  key={candidate.event.id}
                >
                  <input
                    checked={dependencies.has(candidate.event.id)}
                    disabled={isSaving}
                    onChange={(event) => {
                      const next = new Set(dependencies);
                      if (event.target.checked) next.add(candidate.event.id);
                      else next.delete(candidate.event.id);
                      void run(() => onSetDependencies([...next]));
                    }}
                    type="checkbox"
                  />
                  <span className="line-clamp-2">{candidate.title}</span>
                </label>
              ))}
          </div>
        </details>
      ) : null}
      <details className="mt-3 text-xs text-black/60 dark:text-white/60">
        <summary className="cursor-pointer">Supervised execution</summary>
        <div className="mt-2 space-y-2">
          <label className="block" htmlFor={`executor-${item.event.id}`}>
            Executor pubkey
            <Input
              className="mt-1 font-mono"
              disabled={isSaving}
              id={`executor-${item.event.id}`}
              onChange={(event) => setExecutor(event.target.value)}
              placeholder="64-character agent pubkey"
              value={executor}
            />
          </label>
          <label className="block" htmlFor={`base-ref-${item.event.id}`}>
            Base ref
            <Input
              className="mt-1"
              disabled={isSaving}
              id={`base-ref-${item.event.id}`}
              onChange={(event) => setBaseRef(event.target.value)}
              placeholder="main"
              value={baseRef}
            />
          </label>
          <Button
            className="w-full"
            disabled={isSaving || !/^[0-9a-f]{64}$/i.test(executor.trim())}
            onClick={() =>
              void run(() =>
                onStart({
                  ...item.projection.snapshot,
                  automation: "supervised",
                  baseRef: baseRef.trim() || "main",
                  executor: executor.trim().toLowerCase(),
                  state: "in_progress",
                }),
              )
            }
            size="sm"
            type="button"
          >
            <Play className="h-3.5 w-3.5" /> Start supervised run
          </Button>
          {item.job ? (
            <div
              aria-live="polite"
              className="rounded-md bg-black/5 p-2 dark:bg-white/10"
            >
              <p className="font-medium">{item.job.phase.replace(/_/g, " ")}</p>
              <p className="mt-0.5 opacity-75">{item.job.summary}</p>
              {item.job.phase !== "cancelled" &&
              item.job.phase !== "failed" &&
              item.job.phase !== "completed" ? (
                <Button
                  className="mt-2 w-full"
                  disabled={isSaving}
                  onClick={() => void run(onCancel)}
                  size="sm"
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
      {item.projection.validationErrors.length > 0 ? (
        <p className="mt-2 text-xs text-red-600 dark:text-red-400">
          {item.projection.validationErrors[0]}
        </p>
      ) : null}
    </article>
  );
}

export function TaskBoardPage({ repoId }: { repoId: string }) {
  const repoQuery = useRepo(repoId);
  const board = useTaskBoard(repoQuery.data);
  const items = board.query.data ?? [];

  if (repoQuery.isLoading || board.query.isLoading) {
    return (
      <div className="p-8 text-sm text-black/60 dark:text-white/60">
        Loading board…
      </div>
    );
  }
  if (!repoQuery.data) {
    return (
      <div className="p-8 text-sm text-red-600">Repository not found.</div>
    );
  }

  return (
    <div className="min-h-dvh bg-[#F3F3F3] px-4 py-6 text-black dark:bg-[#171717] dark:text-white">
      <header className="mb-5 flex flex-wrap items-center justify-between gap-3">
        <div>
          <Link
            className="mb-2 inline-flex items-center gap-1 text-xs text-black/55 hover:underline dark:text-white/55"
            params={{ repoId }}
            to="/repos/$repoId"
          >
            <ArrowLeft className="h-3 w-3" /> {repoQuery.data.name}
          </Link>
          <h1 className="flex items-center gap-2 text-xl font-semibold">
            <Columns3 className="h-5 w-5" /> Task board
          </h1>
        </div>
        <Button asChild variant="outline">
          <Link params={{ repoId }} to="/repos/$repoId">
            Browse repository
          </Link>
        </Button>
      </header>

      {items.length === 0 ? (
        <div className="rounded-lg border border-dashed border-black/15 p-10 text-center text-sm text-black/55 dark:border-white/15 dark:text-white/55">
          No NIP-34 tasks have been published for this repository.
        </div>
      ) : (
        <div className="grid gap-3 md:grid-cols-3 xl:grid-cols-6">
          {COLUMNS.map((column) => {
            const columnItems = items.filter(
              (item) => item.projection.snapshot.state === column.state,
            );
            return (
              <section key={column.state}>
                <h2 className="mb-2 flex items-center justify-between text-xs font-semibold uppercase tracking-wide text-black/55 dark:text-white/55">
                  {column.label}
                  <span>{columnItems.length}</span>
                </h2>
                <div className="space-y-2">
                  {columnItems.map((item) => (
                    <TaskCard
                      isSaving={board.isSaving}
                      item={item}
                      items={items}
                      key={item.event.id}
                      onMove={(state) => board.move(item, state)}
                      onSetDependencies={(dependencies) =>
                        board.setDependencies(item, dependencies)
                      }
                      onStart={(snapshot) => board.start(item, snapshot)}
                      onCancel={() => board.cancel(item)}
                    />
                  ))}
                </div>
              </section>
            );
          })}
        </div>
      )}
    </div>
  );
}
