# Required quality gate

Read `coding_standards.md` before changing code. Every acceptance gate requires:

1. The mechanical Rust and performance checks in `scripts/quality-gate.sh`.
2. An independent code-quality reviewer applying Clean Code, SOLID, dependency
   direction, cohesion, and appropriate design-pattern judgment.
3. A separate adversarial reviewer looking for races, unsafe retries, boundary
   failures, resource leaks, misleading tests, and performance failure modes.

Use separate agents for the two reviews. Give them the actual changed code and
test evidence; ask them to find problems, not to confirm the implementation.
Record findings, fixes, re-review results, and remaining limitations under
`docs/reviews`. Unresolved P1/P2 findings affecting the change block acceptance.
Run affected checks and obtain re-review after fixes. A mechanical pass alone
is not a passed quality gate. Apply these requirements on scheduled follow-ups
as well as interactive work.

Do not edit the separately owned `../pty-runtime` implementation. Integrate its
verified public API only when its task reports completion and evidence supports
the needed behavior.

# External repository learning and credit

When using an external repository to inform work, including a repository outside
an existing plan, first inspect its license at the exact source revision. Record
its URL, revision, license and the specific lessons in a research document under
`docs/research`. Credit the upstream authors and distinguish preexisting local
behavior, newly learned ideas, proposed changes and code actually reused.

Learning a design concept does not establish permission to copy implementation
or documentation. Before copying or adapting material, verify the applicable
terms and preserve required notices and attribution. Report a non-MIT license,
missing/unclear terms or relevant reuse restrictions to the user before adopting
that material; do not claim that every non-MIT license prohibits study or reuse.
Do not add a dependency or import code merely because a repository was shared.
Link the research record from the resulting change/review when its learning is
applied. See [the credited LayerFS review](docs/research/layerfs-learning.md).
