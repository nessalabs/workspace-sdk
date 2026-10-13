# Changelog

## 0.2.0 — unreleased replacement API

- Replace the prototype with a Rust SDK using explicit context boundaries and injected `I…` interfaces.
- Add workspace/run identity, durable supervision, file transfer, checkpoint reconciliation, and stopped-source handoff.
- Add runtime preparation, credential resolution, and native agent authentication actions.
- Scope login attempts to their backend, coordinate preparations through shared store leases, and expose bounded observation/input/resize/cancellation controls with explicit delivery uncertainty.
- Require independent design and adversarial reviews plus local performance qualification at every acceptance gate; retain real PTY support as pending dependency integration.
- Group related domain concepts and document their identity, value, command, and snapshot roles in Rustdoc.
- Use a native Rust supervisor with typed requests/results and explicit target artifact deployment.
- Require caller-owned storage roots instead of assuming a remote home directory.
- Remove the prototype APIs, obsolete spikes, TypeScript package, and JSONL client CLI.
- Sync the vendored Box client to upstream `main` at `69ca3c9`, remove the local file-write patch, and use its command/text-file helpers and typed create/hosting requests.

This is a breaking pre-publication API. The Box dependency is vendored; registry publication remains disabled.
