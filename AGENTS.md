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
