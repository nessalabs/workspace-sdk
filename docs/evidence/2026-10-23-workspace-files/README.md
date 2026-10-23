# Workspace files evidence

Synthetic cloud tests for issue 4. Reviewed source:
`962272b3424e266f557fda159231b42c075ed968`.

[Review and limitations](../../reviews/2026-10-23-workspace-files.md) explains each
log, independent decisions and precise qualification. Runtime/PTY performance
JSON is embedded in the full gate log. No live provider/user data is evidence here.

Run the rendered/drill-down harness after building a Nessa Docs checkout:

```sh
NESSA_DOCS_ROOT=/path/to/nessa-docs CHROMIUM_PATH=/usr/bin/chromium \
  node docs/evidence/2026-10-23-workspace-files/verify-docs.mjs
```

The harness starts and stops its own loopback server/browser, updates only this
evidence folder, and checks sourceRevision against the reviewed source above.
The screenshots are the new file page in its renderer, not a shipped file editor.
