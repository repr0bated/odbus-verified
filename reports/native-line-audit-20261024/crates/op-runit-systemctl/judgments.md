# Contextual JEV receipts — op-runit-systemctl

Task: native-line-audit-20261024/op-runit-systemctl. Date: 2026-10-24. Auditor: Codex crate worker.

## J1

Actual successful installed `await judge(native_state, native_questions)` through functions.eval, Python, transcript label **Judging sole native occurrence**. Instructions read from `xd://eval/judge`; no routing extension, model override, filesystem code, extraction, searches, or subprocess execution used. One request, three independent typed choice questions. State and questions retained below; no runtime evidence asserted.

### State / source evidence

Scope: evaluate every case/compound native use for wording, intent and runtime meaning, not only AI routing. Preserve accurate terminology. Static evidence only.

Coverage statement submitted: all five inventory files read sequentially — Cargo.toml1-26 EOF; README.md1-110 EOF; src/bin/svd.rs1-110 EOF; src/main.rs1-94 EOF; src/dbus.rs1-203,204-403,404-603,604-789 EOF. Root/src/src/bin listings reconcile five files; no extra or nested guidance entries. Human sequential review observes one native token, src/main.rs16 occurrence1. No tokens in other four files. This is human full-read evidence, not automated extraction or proof provided by the model.

Exact occurrence context submitted from `src/main.rs:3-5,12-17`:

```rust
//! Provides a D-Bus interface that maps systemctl commands to **runit**
//! operations for Artix Linux. This host boots runit as PID 1 and is controlled
//! with `sv`; s6 is not installed.
//! | systemctl command | runit equivalent |
//! |-------------------|------------------|
//! | start <svc>       | `sv up <svc>` |
//! | stop <svc>        | `sv down <svc>` |
//! | restart <svc>     | `sv restart <svc>` (native, not stop + start) |
//! | reload <svc>      | `sv hup <svc>` |
```

Exact `src/dbus.rs:409-441` submitted:

```rust
    /// Restart a service (maps to: sv restart <service>)
    async fn restart(
        &self,
        service: &str,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: Header<'_>,
    ) -> (bool, String) {
        if let Err(error) = Self::authorize_lifecycle(service, connection, &header).await {
            warn!(service, %error, "rejected runit restart request");
            return (false, error);
        }
        debug!("Restarting service: {}", service);
        info!(
            "systemctl restart {} -> sv restart {}/{}",
            service, self.runtime_dir, service
        );

        let live = self.live_path(service);
        if !std::path::Path::new(&live).exists() {
            if let Err(e) = self.ensure_symlink(service, &self.enable_dir) {
                return (false, e);
            }
            return self.run_sv(&["up", &self.live_path(service)]);
        }

        let result = self.run_sv(&["restart", &live]);
        if result.0 {
            info!("Service {} restarted successfully", service);
        } else {
            error!("Failed to restart service {}: {}", service, result.1);
        }
        result
    }
```

Admission summary submitted: src/dbus.rs96-141 validates nonempty, <=128-byte, nondot/nondotdot service names and allows only ASCII alphanumeric/_/-/./@; authorize_lifecycle calls validation then require_root_caller, which resolves unique D-Bus sender with GetConnectionUnixUser, rejects lookup/missing-sender errors and uid !=0. src/dbus.rs143-167 run_sv calls Command::new("sv").args(args).output() and returns process success/output or failure.

Proposed decision submitted: retain native as built-in sv restart rather than synthesized stop-plus-start. Missing-live branch separately uses sv up; command-mapping overview is not a claim every branch calls restart. No model/provider/config selection in this use. Explicitly ask whether this missing-entry branch instead requires native wording clarification.

### Questions / explicit contrary criteria

**wording:** Judge the exact native occurrence and intent against all supplied source, including missing-entry sv up branch. Is retaining the phrase warranted, or must its native-specific wording be corrected? Do not infer runtime deployment.

- `retain_accurate_command_term`: Native denotes sv built-in restart, correctly contrasting an emulated stop+start; missing-entry startup does not invalidate that term in this overview.
- `clarify_native_specific_inaccuracy`: The exact native wording materially misstates implementation given the missing-live branch and requires qualification.
- `insufficient_evidence`: Cannot determine terminology correctness from supplied source.

**selection:** Does this native occurrence perform or prescribe implicit AI model/provider/config selection, duplicate model schemas, or weakened identity admission?

- `not_selection`: Native is a descriptive service-command term; restart still enters validated root-only D-Bus gate.
- `selection_or_admission_defect`: Source shows native-based implicit selection, independent schema authority, or bypass of the lifecycle gate.
- `insufficient_evidence`: Supplied source cannot resolve this usage.

**completion_boundary:** Given the explicit complete sequential read ledger, five-file directory reconciliation, single occurrence, and proposed disposition, choose the supported audit boundary. Do not substitute a runtime pass for source audit.

- `complete_static_if_term_retained`: Coverage and occurrence accounting support a complete static native audit if wording is retained; no build/test/deploy or general quality pass follows.
- `native_work_remaining`: A concrete unresolved native occurrence/change or coverage gap remains in supplied evidence.
- `insufficient_evidence`: Evidence lacks enough accounting/context for a static boundary.

### Actual returned answers

```json
{
  "wording": {
    "type": "choice",
    "choice": "retain_accurate_command_term",
    "confidence": 0.89,
    "probabilities": {
      "retain_accurate_command_term": 0.93,
      "clarify_native_specific_inaccuracy": 0.07,
      "insufficient_evidence": 0
    }
  },
  "selection": {
    "type": "choice",
    "choice": "not_selection",
    "confidence": 1,
    "probabilities": {
      "not_selection": 1,
      "selection_or_admission_defect": 0,
      "insufficient_evidence": 0
    }
  },
  "completion_boundary": {
    "type": "choice",
    "choice": "complete_static_if_term_retained",
    "confidence": 0.8,
    "probabilities": {
      "complete_static_if_term_retained": 0.87,
      "native_work_remaining": 0.13,
      "insufficient_evidence": 0
    }
  }
}
```

### Human disposition

Accept wording judgment: retain the one occurrence. `restart` explicitly runs sv restart for an existing live entry; no synthetic stop/start pair. Missing-live branch is included in evidence, not hidden or generalized into a runtime guarantee. Accept not_selection for this occurrence only, not a whole-system security certification. Complete static accounting is grounded in actual reads and coverage.tsv; judge cannot independently establish omitted-file absence or runtime readiness. No source edit or unresolved native-specific overlap/blocker.
