import type { ManagedAgent } from "@/shared/api/types";
import { relayClient } from "@/shared/api/relayClient";
import { signRelayEvent } from "@/shared/api/tauri";
import { startManagedAgent } from "@/shared/api/tauriManagedAgents";
import {
  buildCodeTaskRequest,
  buildCodeTaskRequestTags,
  buildTaskJobLifecycleTags,
  type TaskAutomation,
} from "./protocol";

export type TaskExecutorCapabilities = {
  executionLocality: "desktop";
  ready: boolean;
  readiness: "ready" | "stopped" | "unavailable";
  supportsTools: boolean;
};

export type StartManagedCodeTaskInput = {
  agent: ManagedAgent;
  automation: TaskAutomation;
  baseRef: string;
  channelId: string;
  executor: string;
  expectedRelayUrl: string;
  expectedSignerPubkey: string;
  issueBody: string;
  issueId: string;
  issueTitle: string;
  projectAddress: string;
  repoAddress: string;
};

/** Resolve only task-execution facts; provider details remain backend-owned. */
export function managedAgentTaskCapabilities(
  agent: ManagedAgent,
): TaskExecutorCapabilities {
  const isLocal = agent.backend.type === "local";
  return {
    executionLocality: "desktop",
    ready: isLocal && agent.status === "running",
    readiness: !isLocal
      ? "unavailable"
      : agent.status === "running"
        ? "ready"
        : "stopped",
    supportsTools: isLocal && agent.mcpCommand.trim().length > 0,
  };
}

/** Publish a scoped cancellation without stopping the shared model runtime. */
export async function cancelManagedCodeTask(input: {
  channelId: string;
  executor: string;
  issueId: string;
  requestId: string;
}): Promise<void> {
  const event = await signRelayEvent({
    kind: 43005,
    content: JSON.stringify({
      v: 1,
      operation: "cancel",
      summary: "Cancellation requested by the task owner.",
    }),
    tags: [
      ...buildTaskJobLifecycleTags({
        channelId: input.channelId,
        issueId: input.issueId,
        requestId: input.requestId,
      }),
      ["p", input.executor],
    ],
  });
  await relayClient.publishEvent(
    event,
    "Timed out cancelling the code task.",
    "Failed to cancel the code task.",
  );
}

function codeTaskPrompt(input: StartManagedCodeTaskInput): string {
  const body = input.issueBody.trim();
  return [
    `Implement task: ${input.issueTitle.trim()}`,
    body ? `\nTask details:\n${body}` : "",
    `\nIssue event: ${input.issueId}`,
    `Repository: ${input.repoAddress}`,
    `Base ref: ${input.baseRef}`,
    "\nThis is a supervised run. Work in an isolated Git worktree, inspect the repository instructions before editing, and run relevant checks. Do not commit, push, open a pull request, merge, resolve the issue, or delete the worktree. Leave the diff ready for human review.",
  ].join("\n");
}

/**
 * Start the selected Desktop-managed agent through its existing provider-aware
 * runtime, then publish a provider-neutral code-task request into the repo's
 * bound channel. The request contains no local path or provider configuration.
 */
export async function startManagedCodeTask(
  input: StartManagedCodeTaskInput,
): Promise<string> {
  const capabilities = managedAgentTaskCapabilities(input.agent);
  if (!capabilities.supportsTools) {
    throw new Error(
      "This managed agent does not have a confirmed developer-tool connection.",
    );
  }
  if (input.automation !== "supervised") {
    throw new Error("Task-board v1 starts supervised runs only.");
  }
  if (!capabilities.ready) {
    await startManagedAgent(input.agent.pubkey, {
      expectedRelayUrl: input.expectedRelayUrl,
      expectedSignerPubkey: input.expectedSignerPubkey,
    });
  }

  const request = buildCodeTaskRequest({
    automation: "supervised",
    baseRef: input.baseRef,
    prompt: codeTaskPrompt(input),
  });
  const event = await signRelayEvent({
    kind: 43001,
    content: JSON.stringify(request),
    tags: buildCodeTaskRequestTags({
      channelId: input.channelId,
      executor: input.executor,
      issueId: input.issueId,
      projectAddress: input.projectAddress,
      repoAddress: input.repoAddress,
    }),
  });
  await relayClient.publishEvent(
    event,
    "Timed out starting the code task.",
    "Failed to start the code task.",
  );
  return event.id;
}
