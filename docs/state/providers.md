---
id: "providers"
title: "Choose a provider and understand its boundary"
kind: "contract"
status: "implemented"
summary: "Providers adapt target access while the managed backend shares the supervisor protocol."
parent: "workspace-sdk"
sourceRevision: "867a4c15e97e025a1066b3c0cd133ffa81f374f1"
sources:
  [
    "src/providers/local/provider.rs",
    "src/providers/ssh/provider.rs",
    "src/providers/container/provider.rs",
    "src/providers/sandbox/provider.rs",
    "src/providers/managed/managed_backend.rs",
  ]
diagramLinks: {}
---

Providers adapt target access while the managed backend shares the supervisor protocol.

| Provider  | Creation and ownership                                              | Capabilities and limits                                                                       |
| --------- | ------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| Local     | SDK-owned project/control directories or an attached root           | Trusted host, private ports, stop/destroy only owned targets                                  |
| SSH       | Configured host and namespace, optional attached root               | Trusted remote host, native supervisor; no general remote delete capability                   |
| Container | Owned labelled container and volume, or explicit attached container | Container boundary, CPU/memory/platform/GPU arguments; portable disk quota unsupported        |
| Boat      | Injected API, stable create key or explicit existing sandbox        | Provider sandbox and hosted ports; explicit root/type/config, no ambient credential discovery |

Reconnection validates locator namespace and reference identity without reinitializing vanished private run claims. Container creation checks ownership labels before reusing resources; it does not implicitly mount host projects, sockets or credentials. Boat creation requests no environment when none was supplied and uses an operation-stable idempotency key. These adapters do not turn a lost response into proof that a side effect failed.

Supervisor binaries must match the target OS and architecture. Deployment can be preinstalled or explicitly supplied. Capabilities are per backend; unsupported operations must remain visible rather than inferred from another provider's success. Native file observation and publication platform limits are in [files](files.md).

This is a provider contract matrix, not a lifecycle model of Docker, SSH or Boat internals. Shared SDK identity rules are modeled in [open identity](open-identity.md). Live credentials, cloud cleanup and remote failures are not qualified by local model checking.
