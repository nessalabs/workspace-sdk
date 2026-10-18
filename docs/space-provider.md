# SPACE workspace attachment

The optional `space` feature attaches to an existing SPACE sandbox with a
preinstalled `workspace-supervisor`. The application supplies the API origin,
data-plane origin, management token, guest user, supervisor executable and
existing sandbox ID. This adapter does not read credentials from environment
variables or provision resources.

The adapter targets the [SPACE API at commit
3f0ed159](https://github.com/perplexityai/SPACE/tree/3f0ed159f6249fc9029f6f44d7f7db7d954d1f14).
Its command and SSE routes carry bounded supervisor RPCs. Private request
payloads use that revision's **incubating** `files:write` route. This is a pinned
transport dependency, not a claim of stable file API compatibility.

The native SDK supervisor remains the authority for managed run identity,
retained output, cancellation and checkpoint transfer. SPACE command sessions
are transient: creating one does not execute it until attached, SSE output is
not a durable journal, and platform idempotency records expire. Reconnecting
checks existing native identity; it must not initialize missing control state
or infer that a disconnected workload has stopped.

The requested root is an exact absolute POSIX project directory. Control state
lives outside it in its parent's reserved `.workspace-sdk/{workspace_id}`
namespace. Workspace references bind both origins, sandbox, user, supervisor
and namespace, while credentials remain transient. Applications must keep the
parent and control directory trusted. This does not isolate mutually untrusted
processes running under the same guest user.

Each RPC has one deadline covering command creation and the event stream:
30 seconds by default, configurable from 1 millisecond to 120 seconds. A private
operation shares that deadline across preparation, upload, invocation and any
remaining cleanup; it does not restart the deadline at each step.
Private requests first reserve a private UUID file, upload one bounded payload,
then invoke the supervisor against that file. An ambiguous upload is not
retried or executed. Its private file is retained: deleting it while a late
server write is still possible could recreate it with different permissions.
Such retained input requires owner cleanup after transport activity has ceased.
Tokens and private input do not appear in command arguments or references;
SPACE itself may retain uploaded data, and workload output can expose secrets.

Attachments do not support creation, resume, stop, destroy or port publication.
Sandbox lifecycle and cost management remain with its owner. Local HTTP/SSE
fixtures exercising the real SDK supervisor qualify the adapter contract;
they do not qualify a live SPACE deployment, Linux/KVM isolation, billing,
network durability or VM lifecycle policies.
