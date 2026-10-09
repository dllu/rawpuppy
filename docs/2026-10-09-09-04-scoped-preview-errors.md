# 2026-10-09 09:04 — Scoped preview and load failures

Successful preview/load replies already identify their request generation, but
worker errors previously carried only text. A delayed failure from superseded
edits or an older open request could overwrite the current error/busy state.
Errors now retain preview or load scope, and the editor applies them only when
that request is still current. Generation failures retain document load scope.
Save/export failures preserve their existing operation notifications.

The actual photo worker tests send an invalid old preview, then a valid newer
preview; the failure is tagged with its old identity and the worker still renders
the new request. Another test fails an older missing-original request, then opens
a valid new document. Superseded failure identities cannot apply to those newer
requests. Originals remain immutable and no recipe format changes are introduced.

Validation: 43 default tests, strict Rust 1.99 Clippy, formatting and diff checks
passed. The preceding reconstruction-recovery CI also passed all four jobs.
The full PROMPT.md goal remains active; broader editor interactions and other
completion-audit work remain.
