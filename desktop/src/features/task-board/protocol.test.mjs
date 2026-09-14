import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import {
  analyzeTaskDependencies,
  buildCodeTaskRequest,
  buildCodeTaskRequestTags,
  buildTaskBoardSnapshotTags,
  projectTaskJob,
  projectTaskBoardIssue,
} from "./protocol.ts";

test("shared task-board projection fixtures match Desktop", async () => {
  const fixtureUrl = new URL(
    "../../../../crates/buzz-taskboard/fixtures/projection-v1.json",
    import.meta.url,
  );
  const fixtures = JSON.parse(await readFile(fixtureUrl, "utf8"));
  assert.equal(fixtures.version, 1);
  for (const fixture of fixtures.cases) {
    assert.deepEqual(
      projectTaskBoardIssue(
        fixture.issue,
        fixture.statuses,
        fixture.maintainers,
      ),
      fixture.expected,
      fixture.name,
    );
  }
});

test("snapshot writer canonicalizes dependencies and stays provider neutral", () => {
  const dependency = "b".repeat(64);
  const tags = buildTaskBoardSnapshotTags({
    state: "in_progress",
    dependencies: [dependency.toUpperCase(), dependency],
    baseRef: "main",
    executor: "e".repeat(64),
    automation: "supervised",
  });
  assert.equal(tags.filter((tag) => tag[0] === "buzz-depends-on").length, 1);
  assert.equal(
    tags.some((tag) => tag.join(" ").includes("provider")),
    false,
  );
  assert.equal(
    tags.some((tag) => tag.join(" ").includes("model")),
    false,
  );
});

test("dependency analysis reports missing and cyclic tasks", () => {
  const first = "a".repeat(64);
  const second = "b".repeat(64);
  const missing = "c".repeat(64);
  const result = analyzeTaskDependencies([
    {
      issueId: first,
      snapshot: {
        state: "backlog",
        dependencies: [second],
        baseRef: null,
        executor: null,
        automation: "supervised",
      },
      statusEventId: null,
      updatedAt: 1,
      validationErrors: [],
    },
    {
      issueId: second,
      snapshot: {
        state: "backlog",
        dependencies: [first, missing],
        baseRef: null,
        executor: null,
        automation: "supervised",
      },
      statusEventId: null,
      updatedAt: 1,
      validationErrors: [],
    },
  ]);
  assert.deepEqual(result.cycleMembers, [first, second]);
  assert.deepEqual(result.missing, { [second]: [missing] });
});

test("code-task lifecycle stays provider neutral and ignores malformed updates", () => {
  const issue = "a".repeat(64);
  const requestId = "b".repeat(64);
  const executor = "c".repeat(64);
  const owner = "d".repeat(64);
  const request = buildCodeTaskRequest({
    prompt: "Implement the issue",
    baseRef: "main",
    automation: "supervised",
  });
  const tags = buildCodeTaskRequestTags({
    issueId: issue,
    repoAddress: `30617:${owner}:zorro`,
    executor,
    channelId: "d2719f58-511c-4b9e-a400-73b44c4b8b1d",
  });
  const projected = projectTaskJob(
    {
      id: requestId,
      pubkey: owner,
      kind: 43001,
      created_at: 1,
      tags,
      content: JSON.stringify(request),
    },
    [
      {
        id: "f".repeat(64),
        pubkey: executor,
        kind: 43003,
        created_at: 3,
        tags: [
          ["e", requestId, "", "root"],
          ["e", issue, "", "issue"],
        ],
        content: "not json",
      },
      {
        id: "e".repeat(64),
        pubkey: executor,
        kind: 43003,
        created_at: 2,
        tags: [
          ["e", requestId, "", "root"],
          ["e", issue, "", "issue"],
        ],
        content: JSON.stringify({ v: 1, phase: "running", summary: "Testing" }),
      },
    ],
  );
  assert.equal(projected.phase, "running");
  assert.equal(projected.summary, "Testing");
  assert.equal(JSON.stringify(request).includes("provider"), false);
  assert.equal(JSON.stringify(request).includes("model"), false);
  assert.throws(() =>
    buildCodeTaskRequest({ prompt: "work", baseRef: "../main" }),
  );
});
