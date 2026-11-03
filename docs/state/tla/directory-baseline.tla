---- MODULE DirectoryBaseline ----
EXTENDS Naturals
VARIABLES phase, pages, first, second, baseline, valid, publishedPages
vars == <<phase, pages, first, second, baseline, valid, publishedPages>>
Init == /\ phase = "Idle" /\ pages = 0 /\ first = 0 /\ second = 0
        /\ baseline = 0 /\ valid = TRUE /\ publishedPages = 0
Begin == /\ phase = "Idle" /\ phase' = "Reading"
         /\ first' \in 1..2 /\ second' \in 1..2
         /\ UNCHANGED <<pages, baseline, valid, publishedPages>>
ReadPage == /\ phase = "Reading" /\ pages < 2 /\ pages' = pages + 1
            /\ UNCHANGED <<phase, first, second, baseline, valid, publishedPages>>
Publish == /\ phase = "Reading" /\ pages = 2 /\ first = second
           /\ phase' = "Published" /\ baseline' = first
           /\ valid' = (first = second) /\ publishedPages' = pages
           /\ UNCHANGED <<pages, first, second>>
Abort == /\ phase = "Reading" /\ phase' = "Aborted"
         /\ UNCHANGED <<pages, first, second, baseline, valid, publishedPages>>
Next == Begin \/ ReadPage \/ Publish \/ Abort
Spec == Init /\ [][Next]_vars
ConsistentPublication == valid
CompletePublication == phase = "Published" => publishedPages = 2
AbortPreservesBaseline == phase = "Aborted" => baseline = 0
====
