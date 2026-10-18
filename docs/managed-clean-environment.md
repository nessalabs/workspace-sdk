# Explicit child environment clearing

`Workspace::start_piped_with_clean_environment(LaunchRequest)` starts or reconnects
to a managed piped run whose workload command clears its inherited environment
before adding the request's explicit values. The executable must be an absolute
target path; relative names are refused before a launch claim is created.

The native child command applies these operations in order:

1. Clear inherited environment variables.
2. Apply `CommandSpec::env_remove`.
3. Add `CommandSpec::env`.
4. Add transient `ExecutionRequest::secrets`, overriding public values with the
   same name.

A removal does not suppress a later explicit value. No PATH, HOME, temporary
directory, locale or loader variable is reconstructed automatically. Supply the
values the chosen executable needs. An interpreter or the operating system may
itself introduce variables; clearing describes the environment passed by the
native supervisor, not a promise about everything the child will subsequently
observe. The absolute executable can be a launcher or interpreter; this API is
not an executable or filesystem sandbox.

The clean mode is part of the salted launch identity. An exact request retry
returns the same RunRef without launching another process. Changing between clean
and inherited modes with the same ID conflicts. Ordinary launches, existing
`start_piped`, terminal launches, foreground and private execution keep their
previous behavior and fingerprint encoding. Command and launch request struct
literals are unchanged.

The [managed pipe contract](managed-pipe-input.md) still governs bounded input,
accepted prefixes, unknown delivery, explicit EOF and cleanup ownership. Handle
drop does not terminate execution. A lost launch response is not permission to
choose a new launch ID. An old supervisor or provider that cannot clear the
workload environment refuses the new operation; there is no inherited fallback.

Only the workload child's inherited variables are cleared. The supervisor and
provider transport retain their own environments, credentials and resource
ownership. Transient request secrets remain private transport values, not
launch configuration to persist. A child may echo its explicit secrets into
output; output is not sanitized. This API alone does not qualify an agent host
adapter, application migration, or any live remote provider.
