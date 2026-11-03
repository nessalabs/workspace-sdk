---- MODULE EditReview ----
EXTENDS Integers, Sequences
VARIABLES generation, latest, offered, quiet, applied, acceptedGeneration, acceptedQuiet
vars == <<generation, latest, offered, quiet, applied, acceptedGeneration, acceptedQuiet>>
Init == /\ generation = 0 /\ latest = -1 /\ offered = -1 /\ quiet = FALSE /\ applied = FALSE /\ acceptedGeneration = -1 /\ acceptedQuiet = TRUE
Stage == /\ ~applied /\ generation < 2 /\ generation' = generation + 1 /\ latest' = -1 /\ UNCHANGED <<offered, quiet, applied, acceptedGeneration, acceptedQuiet>>
Review == /\ ~applied /\ quiet /\ latest' = generation /\ offered' = generation /\ UNCHANGED <<generation, quiet, applied, acceptedGeneration, acceptedQuiet>>
Quiet == /\ ~applied /\ quiet' = ~quiet /\ UNCHANGED <<generation, latest, offered, applied, acceptedGeneration, acceptedQuiet>>
Apply == /\ ~applied /\ offered >= 0 /\ offered = latest /\ latest = generation /\ quiet /\ applied' = TRUE /\ acceptedGeneration' = offered /\ acceptedQuiet' = quiet /\ UNCHANGED <<generation, latest, offered, quiet>>
Next == Stage \/ Review \/ Quiet \/ Apply
Spec == Init /\ [][Next]_vars
LatestReviewOnly == applied => acceptedGeneration = generation
QuiescentApply == acceptedQuiet
====
