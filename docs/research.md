# Research behind the workspace SDK design

Reviewed 2026-09-07. This is a focused architecture comparison, not a benchmark. External behavior below comes from project documentation and source; the recommendations are our design judgment. Subsequent Box-only [live spikes](evidence/README.md) test the important execution and transfer assumptions; the other platforms were not exercised live.

This document records the design research preceding implementation. For the delivered 0.2 API and current verification status, use [architecture](architecture.md) and [integration evidence](evidence/README.md).

## Recommendation

Build a small execution and workspace portability SDK, using Ascii Box as the first cloud backend through `box_client`. Borrow the workspace boundary from OpenHands, infrastructure independence from DevPod, and explicit conflict handling from Git and Mutagen. Keep agent frameworks, hosted account management, and infrastructure scheduling outside the core.

The distinguishing feature worth building is the complete **local files → selected machine → agent run → changes back → continue elsewhere** workflow. A generic wrapper around `exec()` alone will not deliver it.

## What comparable projects actually provide

| Project | Relevant design and evidence | Decision for this SDK |
| --- | --- | --- |
| **OpenHands Software Agent SDK** | A shared workspace interface covers local subprocesses and remote execution through Agent Server. Its agent, tools, workspace implementations, and server are separated into packages. [Workspace architecture](https://docs.openhands.dev/sdk/arch/workspace), [package architecture](https://docs.openhands.dev/sdk/arch/overview). | Borrow the boundary. Offer OpenHands as an agent integration; do not require every consumer to adopt its conversation and tool framework. |
| **OpenHands Agent Server** | Exposes conversations and workspace operations through HTTP/WebSocket. Standalone local SDK use does not require the server. The repository also supplies a TypeScript client. [Agent Server](https://docs.openhands.dev/sdk/arch/agent-server), [SDK repository](https://github.com/OpenHands/software-agent-sdk). | A good option for an application already choosing OpenHands. Running its server everywhere solely to obtain shell/file operations would add an unnecessary dependency here. |
| **DevPod** | Uses existing infrastructure tooling to provision development environments and installs helper agents for forwarding and logs. Workspaces can originate from local folders, repositories, or images. [How it works](https://devpod.sh/docs/how-it-works/overview), [workspace creation](https://devpod.sh/docs/developing-in-workspaces/create-a-workspace). | Borrow bring-your-own-infrastructure and reproducible setup. An existing reachable VM should work through SSH, without an AWS/GCP/Azure adapter. Consider a DevPod integration later if full devcontainer support becomes a requirement. |
| **Coder** | A workspace service supplies a consistent connection interface. Coder's own AI agent can run in its control plane while tools execute in a workspace; chat data stays in the control plane. [Infrastructure architecture](https://coder.com/docs/admin/infrastructure/architecture), [agent architecture](https://coder.com/docs/ai-coder/agents/architecture). | Agent placement and tool placement are separate choices. Support a whole agent process on the target as the default, and let callers use workspace primitives for remotely executed tools too. Do not require Coder's platform. |
| **Daytona** | Separates sandbox infrastructure from its execution/file API. Its platform includes an API, sandbox management, runners, metadata services, and storage. [Architecture](https://www.daytona.io/docs/en/architecture/). | A possible future backend. Its platform topology solves hosting at scale; we do not need to reproduce it to provide an SDK. |
| **E2B** | Provides sandbox lifecycle, files, commands, and reconnect-by-ID. The reviewed SDK documents pause/resume within E2B. [Sandbox reference](https://e2b.dev/docs/sdk-reference/js-sdk/v2.6.2/sandbox). | Borrow durable references and explicit lifecycle operations. Resuming a provider sandbox does not establish a portable checkpoint format for another provider. |
| **Mutagen** | Synchronizes pairs of filesystem endpoints and offers safe modes that record unresolved conflicts. It is a file replication mechanism. [Synchronization and modes](https://mutagen.io/documentation/synchronization). | Evaluate it for optional continuous sync after checkpoint transfers work. Do not build a watcher/delta-transfer engine first, or infer a coherent agent checkpoint from a successful file sync. |
| **Git** | Bundles transfer objects and refs without a continuously available Git server. Worktrees provide separate checkouts. [Git bundle](https://git-scm.com/docs/git-bundle), [Git worktree](https://git-scm.com/docs/git-worktree). | Reuse Git snapshots, ancestry, and merge machinery for code changes. No GitHub dependency, and no custom distributed filesystem. |

OpenHands persistence is relevant to **agent state**, not just environment selection: its restore flow uses a conversation ID and persistence directory. That does not establish a universal format for moving state between different agent frameworks. An integration must verify which state is exportable, whether it includes secrets, and which runtime versions can restore it. [Persistence guide](https://docs.openhands.dev/sdk/guides/convo-persistence).

## Ascii Box: use the capabilities already available

The cloud choice is fixed by the current user requirement: **Ascii Box first, through [asciibox-client-rs](https://github.com/nessalabs/asciibox-client-rs)**.

After the client update, a fresh Git checkout at `392bdd38d1ab3cf7d2fd62689b48cba525ea3e90` contains detached commands, status polling, deletion operations, snapshots, and readiness helpers. The SDK spike bridge builds against that clean, unpatched source. However, `write_file` still sends `POST`; the live API required `PUT`. The earlier successful transfer runs used an isolated corrected copy of the neighboring working tree. Pin a revision containing that correction before publishing a dependency contract. [Inspected client source](https://github.com/nessalabs/asciibox-client-rs/blob/392bdd38d1ab3cf7d2fd62689b48cba525ea3e90/src/client.rs), [build provenance](evidence/box-client-updated-build.json), [live evidence](evidence/README.md).

Specific findings that change our design:

- Box supports detached command launch with process IDs. Command submission is not automatically retried because a failed response can leave the command running. Use stable SDK run IDs and reconcile before retrying. [Command execution](https://docs.ascii.dev/box/api/reference/agent/execute-box-command).
- Command status returns log tails and can report `lost` after the Box agent restarts. Polling those tails is not a lossless event stream. Use bounded, sequenced output files for managed SDK runs; expose missing output and uncertain status honestly. [Command status](https://docs.ascii.dev/box/api/reference/agent/get-command-status).
- Box `idle`/`running` reflect the native prompt path; user processes launched through commands or SSH do not determine those states. Workspace readiness and our run status must be separate. [API lifecycle](https://docs.ascii.dev/box/api/v1).
- Hosting returns an internet-reachable HTTPS URL, protected by a token by default. It is different from a loopback SSH forward, and the SDK must label the access mode. [Hosting](https://docs.ascii.dev/box/hosting).

The live probes verified tested working directories, a 1 MiB binary transfer with the correction, detached execution across a client restart, and cancellation of an owned process group. Remaining probes include process behavior at TTL/archival, larger transfer limits, SSH availability, supervisor crash recovery, and environment setup for the first actual agent integration. Provider deletion remains blocked in the recorded observations. Do not infer arbitrary-process cancellation from the native prompt interruption endpoint.

## Workload setup and subscription authentication

The requirement is to use the user's own compute account and supported runtime subscription/API credentials now; a managed cloud offering is later work. The runtime runs on the selected target, whether it is a coding CLI, an SDK-based server, or an ordinary binary. Authentication methods differ by runtime; an application login is not a universal model credential.

| Primary-source finding | Design consequence |
| --- | --- |
| Codex documents headless device-code login and, as a fallback, copying a local file-backed `auth.json`. Credentials may instead live in an OS keyring. [Authentication](https://learn.chatgpt.com/docs/auth). | Prefer target-side login; offer explicit supported cache provisioning. Never put authentication in project sync. The installed local CLI also exposes `--device-auth` and stdin-based key login. |
| Codex App Server provides browser/device login methods. Its experimental external-token mode requires the host to supply refreshed tokens. [App Server](https://learn.chatgpt.com/docs/app-server#authentication-endpoints). | Our application can present native sign-in. An offline laptop cannot be the only refresh provider for a remotely continuing run. |
| Box has configured Codex/Claude Code prompting and returns a setup URL when the provider is unconfigured. [Box API](https://docs.ascii.dev/box/api/v1). | Use native agents when chosen; expose missing setup. An arbitrary binary remains a separate launch mode. |
| Box environments control repository, secret, and agent-credential inheritance. `noEnv` suppresses inherited owner credentials; per-Box variables can still be supplied. [Environments](https://docs.ascii.dev/box/environments). | Select the user's intended environment explicitly. Our synthetic `noEnv` runs did not prove native subscription authentication. |
| OpenCode documents ChatGPT login and API keys, but explicitly rules out Claude Pro/Max subscription plugins. [Providers](https://opencode.ai/docs/providers/). Its current source includes a headless ChatGPT device flow. [Native implementation](https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/plugin/openai/codex.ts). | Support OpenCode's own OpenAI login when the installed version exposes it. For Claude in OpenCode, request an Anthropic API key or supported inference-provider access. Do not import Codex/Claude caches as if they shared a format. |
| `opencode serve` exposes a headless API; `opencode attach` connects its terminal client, and `opencode run` supports non-interactive use. [Server](https://opencode.ai/docs/server/), [CLI](https://opencode.ai/docs/cli/). | Install/run the server on the target and return a protected endpoint. The model call and tools stay remote while the user can keep a local client. |
| Anthropic permits hosting the unmodified Claude Code binary under its stated commercial conditions, retaining native auth choices and direct end-user billing. Users can sign into that binary with their subscriptions. Own-product Agent SDK integrations should use API keys or supported cloud providers; developers may not collect Claude subscription tokens. [Published policy](https://code.claude.com/docs/en/legal-and-compliance). | Support native user sign-in to hosted Claude Code, without our own Claude OAuth/token-copy feature. Use user API credentials for our own Claude SDK integration. |
| Claude Code provides native login, `setup-token` for user-managed headless operation, and `ANTHROPIC_API_KEY`; in non-interactive mode a supplied API key takes precedence. [Authentication](https://code.claude.com/docs/en/authentication). `claude -p` produces structured output and supports session references. [Programmatic use](https://code.claude.com/docs/en/headless). | Ask for an API key when that route is selected or supported subscription access is unavailable. Do not inject an ambient API key into a selected subscription session and change its billing unintentionally. |

A small environment recipe should inspect installed tools, install missing pinned versions, declare required inputs/secret names, and verify readiness before starting work. This is our design recommendation, not a newly tested SDK feature. Prefer existing provider setup mechanisms where suitable; they can execute setup code without requiring another platform service. The same recipe can prepare an agent server or a training binary. Hardware and dataset requirements must be explicit, and unsupported targets rejected early.

No user agent credential file was read or copied for this research. The new setup, subscription-login, token-refresh, and resource-preflight probes are listed in the [plan](plan.md); the existing live results validate the execution/transfer primitives only.

The caller has OpenCode `1.18.27` and Claude Code `2.1.260`; only their version commands were executed. Source/documentation support is not a successful remote login test. OpenCode's Anthropic section still contains an older Pro/Max sentence alongside its explicit removal/prohibition notice; the proposed integration follows the latter and Anthropic's own policy. Pin and probe the chosen runtime build rather than relying on menu wording in a document.

## What the earlier experiment tells us

Reviewed `agent_cloud` at commit `d28d6613db2ab464fc8a0ff781f32e2865a34d19`. It is a private repository; these links require access.

The [HLD](https://github.com/Varuas37/agent_cloud/blob/d28d6613db2ab464fc8a0ff781f32e2865a34d19/HLD.md) expresses the useful intent: agents across devices and cloud, reusable infrastructure, and handoff. Its scope also includes accounts, organizations, device pairing, routing, grants, a developer platform, durable orchestration, and encrypted traces. The [portability contract](https://github.com/Varuas37/agent_cloud/blob/d28d6613db2ab464fc8a0ff781f32e2865a34d19/INFRA_PORTABILITY.md) expands substitution across identity, queues, runtimes, secrets, storage, and observability.

The [local runtime implementation](https://github.com/Varuas37/agent_cloud/blob/d28d6613db2ab464fc8a0ff781f32e2865a34d19/crates/adapters/local/src/local_process_runtime.rs) launches a runner using callback URLs, run tokens, and writer epochs; the [AWS runtime](https://github.com/Varuas37/agent_cloud/blob/d28d6613db2ab464fc8a0ff781f32e2865a34d19/crates/adapters/aws_runtime/src/lib.rs) maps launches onto ECS. These demonstrate real execution seams, but they belong to that control-plane design.

My diagnosis is a scope problem: the experiment tried to make a complete hosted agent platform portable before establishing the smaller workspace workflow. Keep stable run identity, adapters, cancellation, and checkpoints as ideas. Reimplement only the small portions needed here; do not transplant the service graph, callback contract, or old platform requirements.

## Build versus adopt

| Choice | Assessment |
| --- | --- |
| Adopt OpenHands as the entire foundation | Strong if every project wants OpenHands agents. Too constraining for infrastructure intended to host arbitrary agent processes and existing SDKs. |
| Wrap DevPod or deploy Coder as the foundation | Strong for a complete remote development product. Adds provisioning and product assumptions beyond the requested SDK. |
| Build directly on Box and hide it behind local/cloud flags | Fast first demo, but misses remote machines, sync semantics, and reliable continuation. |
| **Small SDK + flat backends + shared checkpoint sync** | **Recommended.** Reuses Box immediately and adds only the lifecycle, transfer, and run semantics needed across targets. |

Proceed with the [architecture](architecture.md) and [implementation plan](plan.md). Another cloud vendor is useful only after Ascii Box, a local sandbox, and one ordinary SSH machine pass the same user workflow.
