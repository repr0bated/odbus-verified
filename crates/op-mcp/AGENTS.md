# op-mcp — scope boundary for agent runs

## Do not edit this crate unless it is the explicit subject of your task

This crate is under active development by a dedicated agent working on the
**cognitive** subsystem. Concurrent edits here cause lost work, because two
agents share one working tree with no locking between them.

### If this crate fails to compile

**Leave it.** A build break here is almost always a transient mid-edit state
from the cognitive work, not a defect for you to repair. `op-mcp` is a
dependency of `op-web` and others, so its breakage will surface as *their*
build failing — that is still not your signal to edit this crate.

Concretely: if you see something like

```
error[E0004]: non-exhaustive patterns: `ServerMode::Cognitive` not covered
  --> crates/op-mcp/src/grpc/service.rs
```

that is a new enum variant landing in stages. Do **not** add the missing arm.
Report the break and continue with your own scope, or stop if you are blocked.

### Why "it's only one line" is not an argument

A one-line fix is exactly the kind that collides silently: the owning agent
writes the same arm with a different value or ordering, and one edit
overwrites the other with no conflict marker to warn anyone. Small edits are
more dangerous here, not less, because nobody reviews them.

### Never do this

- `git checkout --`, `git reset`, `git stash`, or any destructive git command
- Reverting or overwriting uncommitted changes you did not make
- "Cleaning up" unrelated code in this crate

### The rule

Check `git status` / `git diff` before touching any file. If it already has
uncommitted changes you did not make, integrate around them or report the
conflict — do not resolve it yourself.
