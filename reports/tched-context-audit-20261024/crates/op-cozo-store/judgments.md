# Contextual JEV judgments

All queries asked whether the exact source context supported retaining the occurrence, demonstrated a concrete correction, or was insufficient/conflicting; they explicitly disallowed inferring deployment from source/test fixtures.

| Receipt | Context groups | JEV choice / confidence | Human disposition |
|---|---|---|---|
| `jdgb-15b33344ea55cf68` | `native` (`native_creation.rs:40,54,164`) | retain / 0.83 | Retain. One consistent reserved identity contract. |
| same | `provision` (`clean_provision.rs:183`) | retain / 0.66 | Retain. Two explicit accepted names plus invariant validation; no retirement evidence. |
| same | `inventory_paths` (`inventory/mod.rs:41,43`) | insufficient / 0.20 (initial context only) | Earlier disposition is superseded: line 41 helper and line 43 threetched root are source paths, now checked against producer/staging evidence below. |
| same | `inventory_fixtures` (`inventory/tests.rs:479-495,592-595`) | retain / 0.41 | Retain fixture/source pairing; tests do not prove deployment. |
| same | `test_names` (`identity_transactions_tests.rs:1938,1999`) | retain / 0.83 | Retain test fixture and deliberate foreign-name negative case. |
| same | `storage_fixture` (`lib.rs:2547`) | insufficient / 0.49 | Retain as serialization-only fixture; not an authoritative storage claim. |
| same | `incidental` (`lib.rs:757,1664,1666`) | retain / 0.67 | Retain; English word and local Datalog relation, unrelated to product naming. |
| `jdgb-15b33375fad5cf69` | `inventory_manifest` (`inventory/mod.rs:1709-1712`) | insufficient / 0.98 | Superseded. That query lacked producer/staging source and conflated filename with plugin ID. |
| `jdgb-15b33644e897fbf4` | `manifest_path` (`inventory/mod.rs:1709-1713`; build-golden.sh:604-613,751-753; build-threetched.sh/source.pin) | correct / 0.98 | Retain exact prepared provenance path; producer, manifest validation and staging agree. `tched-router` is filename/build provenance, not sealed plugin ID. |
| same | `binary_root` (`inventory/mod.rs:43`; build-golden.sh:751,604-613) | correct / 0.92 | Retain staged-root mapping; install path and manifest example runtime path align. |

Method: declarative JEV `judge_batch` with exact source-context groups. Initial inventory-path and manifest judgments lacked complete producer evidence and are superseded above. The follow-up batch evaluated the exact source mapping alongside the complete threetched pin/build/staging chain. The 28 mandatory preload paths were not fully receipted in the initial audit; all were read through EOF in this continuation. Coordinator gap metadata reports no missing or column-limited crate files. Human disposition preserves source-versus-deployment limits. No crate source changes were made.
