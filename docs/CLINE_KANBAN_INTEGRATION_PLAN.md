# Cline Kanban Integration Feasibility and Spike Plan

## Summary and Recommendation

Investigation baselines:

- Buzz `origin/main`: `7d082f755584a67f4e4b54cad7feeb847220973c`
- Cline Kanban: `14e371ffcaa8a929b4d4b2d23843f17506ecd2aa`
- Local-model design: [First-Class Ollama and Hugging Face Support](OLLAMA_HUGGINGFACE_PLAN.md)

| Approach | Feasibility | Recommendation |
| --- | --- | --- |
| Bundle Cline Kanban unchanged | Low | No-go |
| Synchronize Buzz with a Kanban sidecar | Medium-low | Do not pursue beyond a compatibility probe |
| Build a Buzz-native board and selectively reuse Kanban concepts or pure code | High | Recommended |

Cline Kanban is a local Node runtime and React UI whose canonical state,
worktrees, terminals, and sessions live outside Buzz. Its package exports only
a narrow API contract, depends on native `node-pty`, assumes local filesystem
access, and is marked as a research preview. Literal embedding would require a
private tRPC contract, bundled Node and native binaries, or a maintained fork.
It would also create a second source of truth and conflict with Buzz's
relay-first, cross-client direction. See the upstream
[architecture](https://github.com/cline/kanban/blob/14e371ffcaa8a929b4d4b2d23843f17506ecd2aa/docs/architecture.md),
[package manifest](https://github.com/cline/kanban/blob/14e371ffcaa8a929b4d4b2d23843f17506ecd2aa/package.json),
and [public export](https://github.com/cline/kanban/blob/14e371ffcaa8a929b4d4b2d23843f17506ecd2aa/src/index.ts).

The Apache-2.0 license permits selective reuse with notice and modification
obligations, subject to normal legal review. See the upstream
[license](https://github.com/cline/kanban/blob/14e371ffcaa8a929b4d4b2d23843f17506ecd2aa/LICENSE).

The recommended integration is a Buzz-native task board using existing NIP-34
issues and job events, borrowing Kanban's strongest concepts: per-task
worktrees, dependencies, compact agent status, review gates, and checkpoints.

## Protocol and Public Interfaces

- Keep kind `1621` as the task root and kinds `1630`-`1633` as its
  [NIP-34](https://github.com/nostr-protocol/nips/blob/master/34.md) status
  history. Add no relay endpoint, database migration, or new event kind.
- Store a complete board snapshot on each authorized status event:
  - `buzz-board-state`: `triage`, `backlog`, `in_progress`, `review`, `done`,
    or `closed`
  - zero or more `buzz-depends-on` issue IDs
  - optional `buzz-base-ref`
  - optional `buzz-executor` agent identity
  - `buzz-automation`: `supervised`, `auto_commit`, or `propose_pr`
- Map board states to standard NIP-34 statuses:
  - `triage` -> Draft `1633`
  - `backlog`, `in_progress`, and `review` -> Open `1630`
  - `done` -> Resolved `1631`
  - `closed` -> Closed `1632`
- Project the latest authorized status by `created_at`, breaking ties by event
  ID. Existing issue-author and maintainer authorization remains authoritative.
- Reject dependency cycles before publishing. If concurrent events nevertheless
  produce a cycle, render it as invalid and block execution until corrected
  instead of silently discarding an edge.
- Extend job kinds `43001`-`43006` with a versioned `code-task` request. The
  request references the issue, project or repository, community, and target
  executor. Progress and result events reference both the request and issue.
- Do not include model IDs, provider IDs, base URLs, credentials, local paths,
  or terminal output in signed events. The board selects an agent; that agent's
  existing configuration selects Ollama, Hugging Face, Mesh, or a cloud
  provider.
- Add a provider-neutral executor boundary:
  - `capabilities(agent)` returns readiness, tool support, execution locality,
    and capacity.
  - `start_session(agent, worktree, task)` starts an ACP session using the
    agent's existing effective configuration.
  - `cancel_session(handle)` cancels only that task session.
  - Provider resolution and credential injection remain behind the
    managed-agent adapter.

## Local-Model Compatibility

- Treat the Rust provider-profile catalog from the Ollama and Hugging Face plan
  as the sole provider authority. Do not modify or duplicate it from task-board
  code.
- The task executor must not launch `buzz-agent` with self-constructed provider
  environment variables. A thin adapter launches through the existing
  managed-agent configuration path so provider defaults, keyring resolution,
  tuning, and restart behavior remain intact.
- Desktop-managed loopback Ollama is eligible only for an executor on that
  Desktop host. Web and mobile may request work from that executor but cannot
  redirect the loopback runtime to another host.
- Hugging Face tokens remain backend and keyring owned. They must never enter
  task events, frontend IPC payloads, worktrees, logs, or remote executor
  configuration.
- Starting a task may trigger the existing managed-Ollama startup path.
  Cancelling or finishing a task must not stop Ollama; only the owner defined by
  the local-model lifecycle may stop it.
- Queue task sessions using the runtime's existing parallelism and capacity
  contract. Publish `waiting_for_capacity` or `waiting_for_model` progress
  instead of spawning excess concurrent sessions.
- Require confirmed tool capability for unattended coding. Unknown capability
  may be used only after a supervised warning; models known not to support
  tools are ineligible.
- The task's automation setting can only narrow the existing agent and persona
  policy. Local models receive no broader filesystem, Git, or network authority
  than cloud-backed agents.

## Modular Implementation

- Add isolated `buzz-taskboard` and `buzz-task-executor` modules for projection
  and execution.
- Add standalone `task-board` feature directories and routes for Desktop, web,
  and mobile.
- Put the Desktop integration in a new managed-agent adapter file. Keep
  provider profiles, readiness logic, keyring commands, Ollama process control,
  and Mesh transport unchanged.
- Define task-specific frontend types inside the task-board feature instead of
  expanding shared runtime-catalog types.
- Reuse Buzz's existing project queries, assignment operations, diff and PR
  surfaces, activity feed, identity, authorization, and ACP integration through
  narrow adapters. Avoid substantive edits to existing project detail screens,
  agent configuration dialogs, and managed-agent lifecycle files.
- Support the same cross-client v1 behavior: view and filter the board, move
  issues, manage dependencies, select an executor, start a supervised run, and
  observe summarized progress. Worktree execution occurs only on an online
  executor host.
- Keep the integration behind one preview capability.
- Target at least 80 percent of functional code in new files and no more than
  ten existing hand-edited integration files, excluding locks and generated
  routes.

## Bounded Ten-Engineer-Day Spike

### Days 1-2: Protocol and Projection

- Implement event builders, projection, authorization, dependencies, cycle
  handling, and shared Rust, TypeScript, and Dart fixtures.
- Cover legacy NIP-34 events, LWW ordering, invalid authors, missing
  dependencies, concurrent cycles, and community isolation.

### Days 3-5: Cross-Client Board

- Add independent board routes to Desktop, web, and mobile.
- Demonstrate a move and dependency update made on one client appearing
  consistently on the other two.
- Keep existing project detail screens unchanged; link to the standalone board
  through a small route-level integration.

### Days 6-8: Supervised Executor

- Create an executor-local detached Git worktree for one selected repository
  and start one ACP session through the managed-agent adapter.
- Support accepted, queued, waiting-for-model, progress, review-ready,
  cancellation, failure, and result events.
- Default to supervised operation. The executor may edit and test in its
  worktree but may not commit, push, open a PR, merge, or resolve the issue.
- Retain recoverable worktrees until explicit cleanup. Never remove them merely
  because the app or executor shuts down.
- Test first with an existing stable provider, then with Ollama after the named
  provider transport is available.

### Day 9: Literal-Package Compatibility Probe

- Pin the upstream Kanban SHA listed above and invoke it only as a disposable
  development child process.
- Block Cline Sentry, PostHog, and feedback egress during the probe. Upstream
  currently initializes external telemetry in its
  [Node runtime](https://github.com/cline/kanban/blob/14e371ffcaa8a929b4d4b2d23843f17506ecd2aa/src/telemetry/sentry-node.ts)
  and [web UI](https://github.com/cline/kanban/blob/14e371ffcaa8a929b4d4b2d23843f17506ecd2aa/web-ui/src/telemetry/sentry.ts).
- Record API stability, startup and memory cost, Node and native packaging
  requirements, process ownership, and compatibility with Buzz-managed agents.
- Do not add Kanban to production manifests or synchronize production Buzz
  events with Kanban's `board.json`.
- Close this track as no-go if it requires private imports or tRPC, separate
  provider configuration, system Node, unsigned native binaries, third-party
  telemetry, or Kanban-owned canonical state. Existing upstream reports already
  indicate [remote-mode limitations](https://github.com/cline/kanban/issues/494).

### Day 10: Evidence and Decision

- Record architecture evidence, protocol examples, screenshots from all three
  clients, execution transcripts, resource measurements, dependency and
  telemetry inventory, and a merge-conflict audit.
- Produce a production estimate only for the native path.

## Test and Acceptance Criteria

- All Rust, TypeScript, and Dart projections produce identical board state from
  the same fixtures.
- Exercise the real workflow: create an issue, move it, link a dependency,
  start a supervised task, receive live progress, inspect its diff, approve or
  reject it, merge manually, resolve the issue, and explicitly clean up the
  worktree.
- A task started from web or mobile can execute through a Desktop-hosted Ollama
  agent without exposing the loopback URL or provider metadata.
- Hugging Face credentials never appear in relay events, IPC responses, logs,
  errors, worktrees, or task output.
- Ollama offline or loading, missing model, exhausted capacity, and agent
  restart states produce recoverable task states.
- Multiple queued tasks respect runtime parallelism and do not independently
  start or stop the managed Ollama daemon.
- Cancelling a task stops only its ACP session; worktree and model-runtime
  ownership remain separate.
- Unauthorized status and execution events are ignored, dependency cycles
  cannot execute, automation cannot exceed agent policy, and community
  switching cannot leak board or executor state.
- Confirm there is no third-party Cline telemetry or network traffic in the
  native path.
- Run focused protocol, executor, SDK, and client tests; local-model integration
  tests; real Ollama tool-call acceptance; applicable relay integration tests;
  Desktop typecheck and E2E; web build and tests; mobile analyze and widget
  tests; and finally `just ci`.

## Delivery Order and Assumptions

1. Land the local-model plan's provider catalog and Desktop projection.
2. Land named Ollama and Hugging Face transports and credential handling.
3. Land the task-board protocol and cross-client read/write UI, which remain
   provider independent.
4. Land the thin managed-agent executor adapter and supervised worktree
   execution.
5. Add managed Ollama and gated Hugging Face acceptance coverage as their
   lifecycle work becomes available.

Additional assumptions:

- Cross-client v1 includes equivalent board state, editing, dependencies, task
  initiation, and progress summaries. Local worktree and detailed diff
  operations remain executor or Desktop capabilities.
- Existing NIP-34 issues, projects, assignments, agent identities, and job kinds
  remain canonical.
- Kanban code is reused only when it is pure, dependency-light, and materially
  better than a small native implementation. Required notices and modification
  markings are retained.
- Upstream's research-preview autonomy defaults and permission-bypass mechanisms
  are not adopted. See the upstream
  [README](https://github.com/cline/kanban/blob/14e371ffcaa8a929b4d4b2d23843f17506ecd2aa/README.md).
- Before implementation, rebase the Ollama and Hugging Face plan's older
  baseline onto current `origin/main` and resolve any changed runtime contracts.
