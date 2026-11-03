---
id: "verification-current"
title: "Current atlas verification"
kind: "guide"
status: "reference"
summary: "Separate current model and reader checks from historical platform qualification."
parent: "workspace-sdk"
sourceRevision: "867a4c15e97e025a1066b3c0cd133ffa81f374f1"
sources:
  [
    "scripts/quality-gate.sh",
    "scripts/state-docs/check.mjs",
    "scripts/state-docs/probe-models.mjs",
  ]
diagramLinks: {}
---

# Check this refresh

This update adds focused identity, handoff, directory-observation and edit-review models plus provider/persistence contracts and a feature map. Existing broad owner pages keep their prior source scope. The [historical verification page](verification.md) describes earlier deployment work; it is not evidence that those suites ran for this update.

## Reproduce the document and model checks

Use Node 24 and a built Nessa Docs checkout with its canonical TLC cache already installed. From this repository:

```sh
export NESSA_DOCS_ROOT=/absolute/path/to/nessa-docs
node scripts/state-docs/check.mjs
node scripts/state-docs/check.mjs --models --write
node scripts/state-docs/probe-models.mjs
```

The catalog-only command executes no model. Model runs are sequential; only `--write` saves canonical baseline receipts. `--install` with `--models` explicitly permits the canonical pinned checker download. Negative controls run in memory and never replace an authored model or its saved passing result. The controls must cover every linked model, and each expected semantic invariant must fail with no unchecked rules.

The models intentionally use finite requests, generations and snapshots. Deadlock checking is disabled for terminal states; no fairness or eventual-progress property is asserted. They check authored design rules, not executable Rust or filesystem/provider correctness. Each owner page explains the omitted behavior.

## Current evidence

At source `867a4c15e97e025a1066b3c0cd133ffa81f374f1`, the canonical checker passed four models and ten configured semantic invariants across 71 aggregate distinct states. All ten independent negative controls broke their expected rules, with none unchecked. Receipts retain final spec/config digests, source revisions, checker version, UTC execution dates and counterexamples:

- [Unmodified baseline results](../../verification/state-models/baselines.json).
- [Negative controls and failing paths](../../verification/state-models/negative-controls.json).
- [Browser results](../../verification/state-models/browser.json): all 16 pages and 14 diagrams opened in headless Chrome without page or diagram errors.
- [Mechanical quality-gate output](../../verification/state-models/quality-gate.log): passed on arm64 macOS with Rust 1.99.1.

The mechanical gate ran formatting, warning-free Clippy, probe builds, all-feature and no-default-feature Rust tests, warning-free Rustdoc, supervisor build and both explicit local performance suites. [Managed-run samples](../../verification/state-models/runtime-performance.json) and [PTY samples](../../verification/state-models/runtime-pty-performance.json) are small local qualification samples, not a statistical performance baseline. Ignored live/environment-specific tests remain ignored unless explicitly selected by that gate; no live Boat/SSH/container or native sign-in qualification is claimed.

The initial toolchain attempt failed before tests because Rust was absent from PATH; a second attempt exposed missing shared libraries in the existing formatter/Clippy installation. A matching isolated compiler, standard library, Cargo, formatter and Clippy were installed from hash-verified official components. The successful full gate used only that complete toolchain. Failed attempts were not counted as passed checks.

## Independent review and fixes

[Adversarial review](../reviews/2026-11-03-atlas-adversarial.md) records an independent docs-only reader exercise followed by source/model review. The reviewer identified that the first directory model's completeness claim exceeded its property: publication assigned the completed page count rather than recording what had actually arrived. The model now counts received pages independently and checks the count captured at publication; a missing-final-page control exposes the error. A separate restoration-stop invariant/control was also added to the handoff model.

A separate quality/design reviewer checked the canonical-checker composition and task explanations. Its record is [quality review](../reviews/2026-11-03-atlas-quality.md). Examples now precede mechanism and locally define target descriptors and stopping all writers. Reader exercises are simulated evidence from reviewers, not a study of real users. Existing historical owner pages and deployment claims were not blanket-requalified.

No production Rust behavior changed. The models remain focused abstractions: provider side effects, actual disk durability, arbitrary editor exclusion, remote credentials, OS containment and automatic model-to-Rust conformance are outside their proofs. Four models do not mean every feature is formally verified; consult the explicit dispositions in [the feature map](feature-map.md).
