---- MODULE OpenIdentity ----
EXTENDS Naturals, Sequences
VARIABLES reserved, request, phase, saved, owner
vars == <<reserved, request, phase, saved, owner>>
Init == /\ reserved = 0 /\ request = 0 /\ phase = "Empty" /\ saved = 0 /\ owner = 0
Choose == /\ request = 0 /\ request' \in 1..2 /\ UNCHANGED <<reserved, phase, saved, owner>>
Reserve == /\ request # 0 /\ phase \in {"Empty", "Reserved"} /\ (reserved = 0 \/ reserved = request) /\ reserved' = request /\ phase' = "Reserved" /\ request' = 0 /\ owner' = (IF owner = 0 THEN request ELSE owner) /\ UNCHANGED saved
Save == /\ phase = "Reserved" /\ request # 0 /\ request = reserved /\ saved' = request /\ phase' = "Saved" /\ UNCHANGED <<reserved, request, owner>>
Ready == /\ phase = "Saved" /\ phase' = "Ready" /\ UNCHANGED <<reserved, request, saved, owner>>
Next == Choose \/ Reserve \/ Save \/ Ready
Spec == Init /\ [][Next]_vars
ReservationStable == reserved = 0 \/ reserved = owner
SavedIdentity == phase \in {"Saved", "Ready"} => saved = reserved
====
