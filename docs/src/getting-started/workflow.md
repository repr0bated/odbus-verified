# Developer Workflow

## Lint and test

From the repository root:

```bash
sh scripts/check-workspace.sh verify
```

This runs formatting, Clippy with all targets/features and warnings denied,
and the complete workspace test suite. It returns failure if any gate fails.
For faster setup validation, use `sh scripts/check-workspace.sh smoke`; that
does not replace full verification. See the
[workspace guide](verified-workspace.md) for prerequisites and diagnostics.

## Frontend

Use the active external UI repository's build and test scripts when a task
includes frontend changes. The in-repository npm manifests are retired and
are not part of backend verification.
