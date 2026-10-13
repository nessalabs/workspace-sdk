# Publication security and rename ownership review

Scoped PASS for the inspected final candidate. Reviewed current workspace source
and `work/publication-candidate-final` read-only, except for this requested review
record. No build, test, credential access, remote action or publication occurred.
This is a publication-content audit, not an assertion that all runtime security
properties are proven.

## Privacy re-review

The private redaction inventory records 134 fields across 12 historical reports.
All 42 original strings in its private mapping are absent from every readable
candidate file. Preserved original evidence covers every redacted report and
contains all original mapped strings. The public redaction document contains
only per-file counts and explanation; its counts match the private inventory,
and it contains no private mapping. Candidate files matching current workspace
paths are byte-identical at review time.

No private user-directory path, credential-shaped private key/token, or live
capability URL was found in the final candidate scans. Previously identified
workspace/resource identifiers, locator targets/control/root fields, deletion
operation identifiers and local binary paths are covered by the verified mapping.
No .env variant, private-key container, or private evidence archive is present in
the candidate. The remaining capability-shaped vendor test URLs use reserved
fixture hosts; they are not live account links.

The final redacted Gitleaks report still contains 19 findings: 14 in the historical
build hash manifest and five generated SDK fixture fields. The build manifest's
file values are SHA-256 digests, not access keys. Vendor fixture findings retain
the previously established generated-fixture classification. No confirmed actual
credential was found. Sensitive values are intentionally not reproduced here.

The root ignore rules now cover /work/, /target/, /dist/, .env and .env.*, PEM/key/
P12/PFX files and .DS_Store. Private archive and mapping locations are under the
excluded work tree. Ignore rules alone do not remove already tracked files;
the final staged publication inventory must still be checked before pushing.
This review does not authorize inclusion of the excluded private archive.

## Historical rename proposal (superseded below)

The Boat-facing API is an alias over the neutral sandbox infrastructure module
and pinned sandbox_client dependency. BoatProvider/BoatOpenOptions and legacy
AsciiBoxProvider/BoxOpenOptions resolve to the same adapter; the client similarly
exports BoatApi/BoatClientConfig over the stable Api/ClientConfig types. The
legacy feature delegates to the new boat feature. Branding does not introduce a
second runtime implementation or domain dependency on transport naming.

The default registration ID remains the existing `ascii-box` identity. This is
appropriate persisted-provider compatibility, not a missed display-name rename.
Sandbox option field names are neutral internally while serde keeps the existing
box_id/box_type serialized names and accepts sandbox_id/sandbox_type aliases.
Existing workspace references therefore keep their registration and wire identity
rather than being silently reassigned during a branding migration. Credentials
continue to belong to the injected client rather than workspace configuration.
No DDD/dependency-direction blocker was found in this inspected naming boundary.

The review closes the previously identified public-metadata/ignore-rule findings
for this candidate. Mechanical, independent quality/adversarial, legal/provenance
and any required release gates remain separate. Changes made after this audit,
including final staging, need a final content/secret check before publication.


## Current-only Boat scope: superseding ownership re-review

The user explicitly requested current-only public names without compatibility.
The historical compatibility section above is no longer the implemented contract.
Current Cargo features expose only boat; the old feature and SDK client/provider
aliases are removed. The default provider registration ID is now boat. Options
serialize as sandbox_id/sandbox_type with deny_unknown_fields and no old aliases.
Old option keys are rejected both alone and combined with current fields; tests
also reject duplicate current fields in both orders instead of selecting a target
by field order. Neutral Sandbox and current Boat names still share one adapter.

This is an intentional breaking configuration change within the user's selected
scope. Domain registration identity remains exact: application lookup uses the
reference's provider ID, and managed locator validation checks it against the
selected provider. No automatic legacy-to-Boat migration, unknown-field fallback
or silent resource reassignment is added. Explicit named registrations remain
isolated and duplicate registration fails. Credentials still belong to the
injected infrastructure client, not serialized workspace options. No new
ownership or dependency-direction blocker was found in the inspected current-only
source and public contract tests.

The earlier privacy redaction findings remain closed for their reviewed content;
this source re-review does not replace the forthcoming exact staged-tree scan or
current-only final-gate/source binding. The coordinator reports a fresh pristine
client revision; vendor provenance verification remains a separate evidence item.
No build or test was launched by this reviewer.

Current-only inspected source SHA-256 bindings:
- `Cargo.toml`: `972e1ba27fdf45c9d929bf3fe76e61901b817ff152fc577172665a82a3ff6077`
- `src/lib.rs`: `ec98d7011a9b6c59f45713dc7cde48afae95e53df9f46615171d181773ee0db8`
- `src/providers/mod.rs`: `a49c934cd3cd2e5dcfb650380135f165a7e0169b14c10dc73554b38b9266c093`
- `src/providers/sandbox/branding.rs`: `f4a50f6a07665fb8b02a1ed2d96f5a423162386e52ce268cf22a70d02ec1f384`
- `src/providers/sandbox/configuration.rs`: `6989e3e213cb438ddabfc9165801e816b567e4299a190f06fb25b21d6439eecd`
- `tests/boat_provider_contract.rs`: `4f8ae7ceef4cf123878f43398c7ac4e5bca9357ec9aacf9539c525f5654f24d0`

## Exact staged publication audit

Scoped PASS for privacy and the inspected current-only ownership boundary in the
261-file staged candidate. Every staged file matched the exported candidate and
its manifest; the reviewed manifest SHA-256 was
`5072780f2595ca55c1028ef878c501202cd0c754c226dc3fe136e2cdacf27180`.
All 42 private original strings remained absent. Scans found no private user path,
credential-shaped secret, nonfixture capability URL, sensitive file container or
excluded work/target/archive content. The 19 redacted scanner findings remain
14 historical SHA-256 digest values and five generated vendor fixture fields.
No new P1/P2 privacy or ownership finding was identified.

The inspected staged source agrees with the current-only ownership re-review
above. At audit time, the only workspace differences from the staged candidate
were removal of one trailing blank line in Cargo.toml and trailing whitespace in
the archived file-write patch. Both were inspected and have no Rust semantics;
the original historical evidence remains preserved in the private archive.
This review record itself will also change the final staged export.

The final mechanical log now ends with successful performance validation and
"Mechanical checks passed." Final acceptance still requires the coordinator to
confirm that process exited successfully, collect the separate quality and
adversarial decisions, and refresh the staged manifest/content scan after these
review-record and whitespace updates. This scoped audit does not certify vendor
provenance or authorize publication independently of those remaining gates.
