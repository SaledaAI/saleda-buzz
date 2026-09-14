import { makeAuthEvent } from "nostr-tools/nip42";

import { queryEvents, type NostrEvent } from "@/shared/lib/nostr-client";
import { makeNip98AuthHeader } from "@/shared/lib/nip98";
import {
  signNostrEvent,
  type SignedNostrEvent,
} from "@/shared/lib/nostr-signer";
import { relayHttpBaseUrl, relayWsUrl } from "@/shared/lib/relay-url";

const TASK_BOARD_KINDS = [1621, 1630, 1631, 1632, 1633];
const TASK_JOB_KINDS = [43001, 43002, 43003, 43004, 43005, 43006];

export async function queryTaskBoardEvents(
  repoAddress: string,
): Promise<NostrEvent[]> {
  return queryEvents(relayWsUrl(), {
    kinds: TASK_BOARD_KINDS,
    "#a": [repoAddress],
    limit: 2_000,
  });
}

export async function queryTaskJobEvents(
  issueIds: string[],
): Promise<NostrEvent[]> {
  if (issueIds.length === 0) return [];
  return queryEvents(relayWsUrl(), {
    kinds: TASK_JOB_KINDS,
    "#e": issueIds,
    limit: 2_000,
  });
}

export async function publishTaskBoardEvent(
  event: SignedNostrEvent,
): Promise<void> {
  const url = `${relayHttpBaseUrl().replace(/\/$/, "")}/events`;
  const body = JSON.stringify(event);
  const authorization = await makeNip98AuthHeader(url, "POST", {
    body,
    requireNip07: true,
  });
  const response = await fetch(url, {
    method: "POST",
    headers: {
      Authorization: authorization,
      "Content-Type": "application/json",
    },
    body,
  });
  const payload = (await response.json().catch(() => null)) as {
    accepted?: boolean;
    message?: string;
  } | null;
  if (!response.ok || payload?.accepted === false) {
    throw new Error(
      payload?.message || `Relay rejected event (${response.status}).`,
    );
  }
}

/**
 * Subscribe to live issue/status updates for one repository.
 *
 * This deliberately owns no cached state: callers invalidate their canonical
 * NIP-34 query and re-project from signed history on each notification.
 */
export function subscribeToTaskBoard(
  repoAddress: string,
  issueIds: string[],
  onEvent: (event: NostrEvent) => void,
): () => void {
  const wsUrl = relayWsUrl();
  const ws = new WebSocket(wsUrl);
  const subscriptionId = `task-board-${crypto.randomUUID()}`;
  let closed = false;
  let requestSent = false;
  let authEventId: string | null = null;
  let unauthenticatedTimer: ReturnType<typeof setTimeout> | null = null;

  const sendRequest = () => {
    if (closed || requestSent || ws.readyState !== WebSocket.OPEN) return;
    requestSent = true;
    const since = Math.floor(Date.now() / 1_000);
    ws.send(
      JSON.stringify([
        "REQ",
        subscriptionId,
        {
          kinds: [...TASK_BOARD_KINDS, ...TASK_JOB_KINDS],
          "#a": [repoAddress],
          since,
          limit: 100,
        },
        ...(issueIds.length > 0
          ? [
              {
                kinds: TASK_JOB_KINDS,
                "#e": issueIds,
                since,
                limit: 100,
              },
            ]
          : []),
      ]),
    );
  };

  ws.addEventListener("open", () => {
    unauthenticatedTimer = setTimeout(sendRequest, 100);
  });
  ws.addEventListener("message", async (message) => {
    let frame: unknown;
    try {
      frame = JSON.parse(String(message.data));
    } catch {
      return;
    }
    if (!Array.isArray(frame)) return;
    if (frame[0] === "AUTH" && typeof frame[1] === "string") {
      if (unauthenticatedTimer) clearTimeout(unauthenticatedTimer);
      try {
        const auth = await signNostrEvent(makeAuthEvent(wsUrl, frame[1]));
        if (closed) return;
        authEventId = auth.id;
        ws.send(JSON.stringify(["AUTH", auth]));
      } catch {
        ws.close();
      }
      return;
    }
    if (frame[0] === "OK" && frame[1] === authEventId) {
      if (frame[2] === true) {
        sendRequest();
      } else {
        ws.close();
      }
      return;
    }
    if (frame[0] === "EVENT" && frame[1] === subscriptionId && frame[2]) {
      onEvent(frame[2] as NostrEvent);
    }
  });

  return () => {
    closed = true;
    if (unauthenticatedTimer) clearTimeout(unauthenticatedTimer);
    if (ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify(["CLOSE", subscriptionId]));
    }
    ws.close();
  };
}
