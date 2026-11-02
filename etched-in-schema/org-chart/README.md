# org-chart

The chart is the sole grant authority. Humans edit org facts (positions,
reporting lines, role assignments, hire/move/leave); the compiler derives
effective capabilities deterministically. Nobody edits a capability directly.

- Same schema, same compiler, both extremes: a deep multi-compartment
  hierarchy and a single position holding a single WireGuard key are both
  just charts. Depth is recursion with a bound (32); cycles refuse.
- No implicit inheritance: positions hold explicit roles. What you see is
  what compiles — no subgroup alignment to untangle later.
- Clearance attributes gate role capabilities per member; missing attributes
  skip the grant **with a recorded reason**, never silently.
- Every grant carries provenance (`via_role`, `via_position`) answering
  "why do I hold this cap?" — the read-only view the console renders.
