---- MODULE Handoff ----
EXTENDS Naturals, Sequences
VARIABLES phase, source, capturedSafe, startedSafe, restoredSafe
vars == <<phase, source, capturedSafe, startedSafe, restoredSafe>>
Init == /\ phase = "New" /\ source = "Running" /\ capturedSafe = TRUE /\ startedSafe = TRUE /\ restoredSafe = TRUE
Observe == /\ source' \in {"Running", "Lost", "Stopped"} /\ phase # "Started" /\ UNCHANGED <<phase, capturedSafe, startedSafe, restoredSafe>>
Capture == /\ phase = "New" /\ source = "Stopped" /\ phase' = "Captured" /\ capturedSafe' = (source = "Stopped") /\ UNCHANGED <<source, startedSafe, restoredSafe>>
Restore == /\ phase = "Captured" /\ source = "Stopped" /\ phase' = "Restored" /\ restoredSafe' = (source = "Stopped") /\ UNCHANGED <<source, capturedSafe, startedSafe>>
Start == /\ phase = "Restored" /\ source = "Stopped" /\ phase' = "Started" /\ startedSafe' = (source = "Stopped") /\ UNCHANGED <<source, capturedSafe, restoredSafe>>
Next == Observe \/ Capture \/ Restore \/ Start
Spec == Init /\ [][Next]_vars
CaptureAfterStop == capturedSafe
StartAfterStop == startedSafe
RestoreAfterStop == restoredSafe
====
