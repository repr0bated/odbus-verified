# Declarative JEV receipts

## J1 — 2026-10-24 — first historical schema capture

Successful installed eval `judge()` call; JS state `j1`. Evidence: examples/README.md:28-32 historical-capture declaration; complete sequential read of examples/zeroclaw-final-plugin-schema.json:1-8509. Exact context groups supplied:

- line5: `Zeroclaw schema/RPC-native model router for Antigravity UI, CLI providers, and structured JSON output`; neighboring captured fields/example/methods, Chat model/provider empty defaults using selected state, per-method capability/subid declarations.
- lines722,7019,7161: `Crate or package that owns the native schema.`; schema_crate=schema metadata, captured default schemars.
- lines729,806,5613,7005,7147,7967: `native_type`; declaring-language type string/default/example `zeroclaw::config::schema::Config` or empty return-definition/default metadata.
- lines732,7007,7149: `Native Rust or Python type name.`
- lines7009,7151: `exp.software.llm-config-schema.native-type@v1` taxonomy metadata.

Questions explicitly evaluated intended meaning/wording accuracy (not only scope/routing), with contrary defect and insufficient-evidence outcomes.

`rpc_wording`: preserve descriptive historical schema/RPC integration terminology versus demonstrably false claim versus insufficient.
Result: `preserve_historical_description`, confidence1; probabilities preserve1/correct0/insufficient0.

`schema_terms`: all listed schema/type/subid contexts accurately denote source-language metadata versus any native semantic defect versus insufficient.
Result: `preserve_type_metadata`, confidence0.91; probabilities preserve0.9400000000000001/defect0.06/insufficient0.

Decision: retain all15 captured occurrences. Not proof of historical or current execution, deployed routing, provider availability, or admission. Source consumers and remaining captures still require sequential review.

## J2 — captured exports, discovered paths, generic URL observation

Successful installed eval `judge()` call; JS state `j2`. Exact contexts: complete.md:7 and complete.json:8 identity/description `Zeroclaw schema/RPC-native model router for Antigravity UI, CLI providers, and structured JSON output`; introspect-gaps.json:377/1740/1741 `struct.zeroclaw_config.NativeRuntime[.shell]`,458/1841 `struct.zeroclaw_config.ThinkingConfig.native_thinking`; complete.json:3461/3462 same runtime paths,3584 same thinking path,3951 `enum.zeroclaw_api.AuthMethod.Native`. Parent gap-report arrays, historical README provenance, and fully read source behavior supplied: extraction/report generation, review-only Rust candidates, no native literal in source/tests; optional env URL then three HTTP drift-check URLs without credential metadata.

Questions/contrary outcomes:
- `descriptions`: preserve intended historical schema/RPC integration description / demonstrated inaccurate historical wording / insufficient.
- `identifiers`: preserve exact discovered path references without runtime assertions / native-specific routing/auth/semantic defect / insufficient.
- `generic_url_observation`: generic independent drift-check concern / native-specific fix / insufficient.

Actual results:
- descriptions=`preserve`, confidence0.53, probabilities preserve0.69/correct0.24/insufficient0.07. LOW CONFIDENCE: not accepted as final disposition; closer emitter/README evidence and J3 required.
- identifiers=`preserve_exact_paths`, confidence0.98, probabilities preserve0.99/defect0/insufficient0.01. Retain all9 path occurrences. Captured reference accuracy does not establish present upstream semantics, backend availability or admission.
- generic_url_observation=`generic_report_only`, confidence0.98, probabilities generic0.99/native0/insufficient0.01. Do not expand native audit into HTTP client repair.

## J3 — closer producer/provenance evidence resolves J2 wording

Successful installed `judge()`; JS state `j3`. Exact descriptions complete.md:7 and complete.json:8 repeated, plus final-schema.json first1000's same plugin_schema description and explicit generated_at2026-08-08/source/method_index provenance. Closer reads: emit.rs:182-185 takes PLUGIN_DESCRIPTION verbatim;197-198 JSON serialization;201-209 Markdown Identity prints doc.plugin.description;321-330 constant extraction; README26 lists JSON/MD artifacts and28-32 explains historical capture preservation. Asked intended wording accuracy with retain/correct/insufficient outcomes; no original runtime verification claim.

Result `wording_disposition=retain_historical_terms`, confidence0.94; probabilities retain0.96/correct0.01/insufficient0.03. Decision retain two complete-export descriptions. Resolves J2 low confidence with actual closer evidence, not an unchanged retry.

## J4 — final-schema.json all15 actual contexts

Successful installed `judge()`; JS state `j4`. Full sequential read1-8831. Contexts:326 exact schema/RPC-native description,1043/7340/7482 exact owns-native-schema description,1050/1127/5934/7326/7468/8288 native_type field/default/example/return keys,1053/7328/7470 exact Native Rust or Python type name description,7330/7472 exact native-type taxonomy. Historical timestamp/README and qualified Config values supplied.

Questions: description coherent captured integration wording / false wording / insufficient; all14 metadata consistent declaring-language metadata / defect / insufficient.
Results:
- description=retain, confidence0.88; probabilities0.92/0.05/0.03. Accepted.
- metadata=retain_metadata, confidence0.49; probabilities0.66/0.07/0.27. NOT ACCEPTED; reread full actual blocks and split type-label meaning from generator/owner wording in J5.

## J5 — disambiguating generator ownership from type metadata

Successful installed `judge()`; JS state `j5`. Closer reread final-schema.json1037-1134,7311-7350,7460-7487,8283-8295. Exact object supplied: schema_crate string, description Crate or package that owns the native schema., default schemars; native_type string, description Native Rust or Python type name., default zeroclaw::config::schema::Config; complete default/example and return definitions. Both captures' owner-description positions supplied: final-plugin-schema722/7019/7161 and final-schema1043/7340/7482. Historical provenance and generated-evidence non-authority supplied.

Questions separately: all11 type keys/descriptions/subids consistent metadata / type defect / insufficient; ownership wording generator/type-owner mismatch with historical report-only disposition / accurate label / current producer fix / insufficient.
Results:
- type_terms=retain_type_terms, confidence0.13; probabilities0.42/0.17/0.41. NOT ACCEPTED; direct grounded J6 verification follows.
- ownership_wording=historical_generator_owner_mismatch, confidence1; probabilities mismatch1/accurate0/current-fix0/insufficient0. **Supersedes J1's blanket accuracy conclusion for three owner descriptions.** Decision F1: preserve explicitly historical capture, record misleading generator/owner terminology; no current producer in assigned crate demonstrated.

## J6 — direct grounded type-metadata verification

Successful installed `jev_verify`, model jev-latest. Claim: In historical captured ConfigSchema, native_type/native-type taxonomy identify source-language type-name metadata, example zeroclaw::config::schema::Config; Native Rust or Python type name label is consistent; exact JSON contexts do not select provider/model/auth; upstream runtime unverified.

Evidence supplied exact field/default/example/return contexts at final-schema1050/1053/1054,1127,5934,7326-7330,7468-7472,8288 and README historical declaration. No verbatim quotation requested.

Actual result: verdict supports; verbatim0.31 (not a requested quote); misleading0.4/3; confidence0.86; tokens815/78. Decision retain all11 source-type metadata occurrences; runtime semantics unverified. J4/J5 low confidence never counted as supporting evidence.

## J7 — full-schema.json actual15 contexts

Successful installed judge(); JS j7. Full sequential read1-8509. Actual contexts supplied:5 exact RPC-native description;3372/3459/5613/5844/6135/6955 native_type metadata with qualified Config or empty defaults;3375/5846/6137 exact Native Rust or Python type name.;5848/6139 exact native-type taxonomy;3385/5858/6149 owns-native-schema description paired with schema_crate=schemars and zeroclaw::config::schema::Config. README historical preservation and assigned source no native producer supplied.

Three independent choice questions: description coherent historical integration wording / false wording / insufficient; eleven type contexts consistent metadata / inaccurate / insufficient; owner label generator/type-owner mismatch / accurate / current producer fix / insufficient.

Results: description=retain confidence0.59 probabilities0.73/0.17/0.10, NOT ACCEPTED. type_metadata=insufficient confidence0.11 probabilities retain0.41/correct0.18/insufficient0.41, NOT ACCEPTED. owner_wording=historical_mismatch confidence1 probabilities1/0/0/0, accepted F1 report-only historical preservation.

## J8 — closer producer/provenance verification for full-schema.json

Successful jev_verify, jev-latest. Claim: actual RPC-native description denotes descriptive integration; eleven type keys/descriptions/subids consistently denote source-language metadata, not runtime selection; preserve historical terms without certifying upstream/current runtime, excluding ambiguous ownership labels. Evidence repeated exact J7 positions/values and added emit.rs182-185,197-198,201-209,321-330 producer behavior plus README28-32 historical preservation. This separates literal metadata meaning from unverified runtime claims.

Result supports, confidence0.67, misleading0.7/3, verbatim0.06 (no quote requested), tokens894/78. Retain12 contexts; J7 low confidence not treated as affirmative evidence.

## J9 — final with-compare capture grounded verification

Successful jev_verify, jev-latest. Full sequential read1-8837. Actual12 contexts supplied:332 exact RPC-native description;3699/3786/5940/6171/6462/7282 native_type with qualified Config or empty defaults;3702/6173/6464 exact Native Rust or Python type name.;6175/6466 exact native-type taxonomy. README historical preservation, meta discovery-catalog/schemars context and emit.rs description copying supplied. Claim coherent historical integration/type metadata to retain, no runtime selection or execution verification, ownership wording excluded.

Result supports but LOW confidence0.46, misleading0.7/3, verbatim0.10 (no quote requested), tokens905/78. NOT ACCEPTED as decisive; J11 separates literal meaning/internal vocabulary from unverified historical runtime.

## J10 — final with-compare ownership descriptions

Successful installed judge(); JS j10. Actual contexts3712/6185/6476 exact owns-native-schema description, neighboring native_type declarations/qualified Config, schema_crate default schemars at3713 and complete default/example3785-3786/5939-5940. Historical preservation/no current producer evidence supplied. Choice question: generator-versus-declaring-owner mismatch / accurate label / current independent source fix / insufficient.

Result historical_mismatch, confidence0.93; probabilities0.95/0.03/0.01/0.01. F1 report-only historical preservation; no current producer repair established.

## J11 — resolves J9 by separating evidence questions

Successful installed judge(); JS j11. Exact twelve J9 contexts/values supplied again with README28-32 and emit.rs182-185,197-209. Explicitly excludes schema ownership and runtime/upstream existence claims.

description question: literal schema/RPC integration adjective with no demonstrated terminology defect / selector / demonstrated false wording / insufficient. Result descriptive confidence1 probabilities1/0/0/0.

type_names question: eleven type keys/descriptions/subids internally coherent source-language type-name vocabulary against qualified Rust type or empty string / incoherent / insufficient. Result coherent confidence0.72 probabilities0.81/0.13/0.06. Retain these literal metadata terms; no certification of upstream type existence or integration execution.

## Final reconciliation

All26 inventory entries completely read;71 native occurrences individually mapped:59 retained terminology/path references,12 historical ownership-label caveats. Eight successful judge calls (J1-J5,J7,J10,J11),16 typed answers; three successful jev_verify calls (J6,J8,J9). Low-confidence answers never treated as affirmative final evidence. No model routing, searches, execution or source mutation.

## J12 — final delivery evidence sufficiency

Successful jev_verify, jev-latest. Claim completed static26-file/71-occurrence audit, no source edits/build/test/deploy claim,12 preserved historical caveats not unresolved current source hunks. Evidence: complete EOF coverage rows,71 mapped occurrence rows and arithmetic, actual contextual judgments, README historical preservation, assigned source no native producer, successful complete result/HANDOFF writes. No independent runtime or reread certification requested.

Result supports confidence0.89, misleading0.4/3, verbatim0.07 (no quote requested), tokens838/78. This is one additional delivery-sufficiency verification, separate from the three occurrence-context jev_verify calls counted above; total jev_verify calls four.
