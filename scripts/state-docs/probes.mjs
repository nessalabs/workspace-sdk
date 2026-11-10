const cases = [
  [
    "open-identity",
    "reservation-reuse",
    "ReservationStable",
    "(reserved = 0 \\/ reserved = request)",
    "TRUE",
  ],
  [
    "open-identity",
    "foreign-descriptor",
    "SavedIdentity",
    "/\\ request = reserved",
    "/\\ TRUE",
  ],
  [
    "handoff",
    "capture-running",
    "CaptureAfterStop",
    'phase = "New" /\\ source = "Stopped"',
    'phase = "New"',
  ],
  [
    "handoff",
    "start-lost",
    "StartAfterStop",
    'phase = "Restored" /\\ source = "Stopped"',
    'phase = "Restored"',
  ],
  [
    "directory-baseline",
    "mixed-revision",
    "ConsistentPublication",
    "/\\ first = second",
    "/\\ TRUE",
  ],
  [
    "directory-baseline",
    "publish-partial-on-abort",
    "AbortPreservesBaseline",
    "/\\ UNCHANGED <<pages, first, second, baseline, valid, publishedPages>>",
    "/\\ baseline' = first /\\ UNCHANGED <<pages, first, second, valid, publishedPages>>",
  ],
  [
    "directory-baseline",
    "publish-before-final-page",
    "CompletePublication",
    "/\\ pages = 2 /\\ first = second",
    "/\\ pages >= 1 /\\ first = second",
  ],
  [
    "handoff",
    "restore-lost",
    "RestoreAfterStop",
    'phase = "Captured" /\\ source = "Stopped"',
    'phase = "Captured"',
  ],
];
export const probes = cases.map(([document, name, rule, before, after]) => ({
  document,
  name,
  expectedRules: [rule],
  mutate(spec) {
    if (spec.split(before).length !== 2)
      throw new Error("Mutation target must match exactly once");
    return spec.replace(before, after);
  },
}));
