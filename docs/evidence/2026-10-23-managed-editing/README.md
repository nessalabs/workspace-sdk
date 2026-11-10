# Managed editing evidence

Source checkpoint: `ec32735865d41e6b5d320ffaed6a9c8f91bedf46`.
See the [review](../../reviews/2026-10-23-managed-editing.md) for findings,
commands, outcomes, coordination limits and platform qualification.

All workspaces are synthetic. Performance logs are small local debug samples.
The macOS cross-check diagnostic records missing Apple build tools, not a pass.

After building a separate Nessa Docs checkout, rerun rendered checks with:

```sh
NESSA_DOCS_ROOT=/path/to/nessa-docs CHROMIUM_PATH=/usr/bin/chromium \
  node docs/evidence/2026-10-23-managed-editing/verify-docs.mjs
```

The harness owns its temporary loopback server/browser and writes only this
folder's verification JSON/screenshots. These show documentation, not a file UI.
