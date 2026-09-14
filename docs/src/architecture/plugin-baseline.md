# Plugin Execution Baseline

This baseline records source-backed understanding of each plugin registration in
the current checkout. It is a navigation and evidence artifact; it contains no
plugin schema definitions and grants no runtime authority.

**Provenance warning:** the cards, line references and
`plugin-baseline-files.sha256` fingerprint are historical pre-repair evidence.
They must be revalidated against current source after the schema-loader,
tched_router contract, identity-client and dispatch repairs; the fingerprint is
intentionally not refreshed here.

Follow [OD-58 on the task board](../../../WISHLIST.md) for current repairs and
verification. Historical `generic_echo` and backend descriptions below must not
be used as the repaired checkout's method-coverage assessment.

The subsequent [topology observation](topology-observation.json) records local
catalog membership and aggregate identity presence. The
[identity transport trace](identity-transport.md) follows minting, SID1 encoding,
metadata injection and receiver checks. See
[fresh-repository requirements](../contributors/doc-maintenance.md#preparing-a-fresh-repository)
before treating this inventory as a complete build or snapshot manifest.

**Owner invariant:** the plugin IS the schema. Its sealed blob IS the plugin.
There must be no parallel `schemas/` source tree. Source that violates this rule
is recorded as a defect, regardless of whether it builds or has passing tests.
3tched-router / `tched_router` is the framework/schema/plugin integration backed
by the upstream `zeroclaw` executable. Assistant is the control-plane chatbot;
Waypipe is deprecated; the historical Gemma routing role is model agnostic.

**Owner-confirmed topology:** WireGuard terminates at the decoy; there is no
WireGuard on this server. WARP transports identity. Source declarations must
be interpreted against these roles, rather than treated as server requirements.

Start with the [tched_router reference trace](tched-router-baseline.md), then use
the inventory below to follow the relevant plugin. The [System Overview](overview.md)
and [Crate Map](crate-map.md) supply cross-crate context.

## Baseline identity and coverage

[plugin-baseline-revision.json](plugin-baseline-revision.json) records the commit,
capture time, pre-existing working-tree changes, registration inventory and file
manifest digest. [plugin-baseline-files.sha256](plugin-baseline-files.sha256)
fingerprints 1,065 source, protocol, schema-input and policy/guidance files.
The manifest stores hashes and paths, not copied schema content or runtime data.
This was a working-tree examination with a final fingerprint recheck, not an
atomic filesystem snapshot or an assertion that HEAD alone describes the code.

The source scan found **70 registration declarations**: **67 compiled and
non-retired**, **one explicitly retired** (`netmaker`), and **two in uncompiled
modules** (`host_runtime`, `privacy_router`). Every declaration has an evidence
entry. `compact_mcp` is a separate explicit retirement marker with no registration
in this scan. Unknown names can produce `AutoPlugin` drafts; they are not a finite
set of built-in plugins. See `DefaultPluginRegistry::load_plugin` and
`RETIRED_PLUGIN_IDS` in
[`default_registry.rs`](../../../crates/op-plugins/src/default_registry.rs), and
[`auto_create.rs`](../../../crates/op-plugins/src/auto_create.rs).

Registration is source inventory. Actual plugin presence is determined by its
sealed catalog blob. The first execution pass did not inspect runtime catalogs;
the subsequent topology pass read the local SHM manifest and found all 67
compiled, non-retired IDs. The persisted catalog and method reachability were
not checked. A declaration, generated route, available helper or passing test
does not establish a working backing implementation.

Three readers examined networking, identity/memory, and system/UI groups; the
coordinator traced `tched_router`, shared admission/dispatch, and cross-group
differences. Cards identify meaningful tests and uncertainty. Coverage is at
plugin and representative-path level, not exhaustive testing of every method,
external binary, transport combination or failure mode.


## Deployment topology classification

The following classification adds deployment role metadata to every card. For the 67 compiled, non-retired IDs, `control_exposure` is the server catalog; method reachability is untested. The three excluded IDs (`netmaker`, `host_runtime`, `privacy_router`) are absent from the local catalog. The catalog observation is metadata only: it does not establish method execution, live health, or source/installed-binary identity.

The owner has confirmed the required roles: the server control-plane chatbot,
model-agnostic routing through the binary-backed tched_router plugin, identity
minting and SID1 metadata propagation, decoy WireGuard termination, WARP identity
transport, and deployment by btrfs snapshot send/receive. Waypipe is deprecated;
Netmaker is explicitly retired in the source loader. A required role does not
make every method in a similarly named schema necessary. The **Requiredness**
column below concerns the declared plugin API: unresolved means its method set
still needs a concrete consumer or owner requirement, not that the underlying
role has been forgotten. `contract_gap` records misleading or unwired methods;
it is not an instruction to implement all of them.

| ID | Backing location | Role evidence | Requiredness | Disposition |
| --- | --- | --- | --- | --- |
| `adc` | AdcState is a single configured boolean; no provider, credential store or durable database is connected here. | `declared_only` | `unresolved` | `contract_gap` |
| `agent_config` | This plugin is an empty struct with schema-derived data declarations; no external agent configuration store or execution client is implemented here. | `declared_only` | `unresolved` | `contract_gap` |
| `antigravity` | current_state combines environment-derived project/location with a substantial static configuration inventory. It sets token_valid and API-key configured flags without an authentication probe. | `declared_only` | `unresolved` | `contract_gap` |
| `antigravity_chat` | current_state supplies a loopback bridge URL, offline status, invalid-token default and headless display metadata, rather than calling the bridge. | `declared_only` | `unresolved` | `contract_gap` |
| `btrfs` | snapshot transport via btrfs send/receive; no plugin backend | `declared_only` | `unresolved` | `contract_gap` |
| `cognitive_mcp` | The bridge lazily opens CognitiveMcpServer::new_durable with Cozo; Qdrant/Voyage retrieval is optional. Runtime tool descriptors are implementation candidates and must derive authority from the plugin’s single schema, never become parallel contracts. | `source_dispatch` | `unresolved` | `retain_current_path` |
| `config` | ConfigPlugin owns a configurable JSON file path, defaulting to /etc/op-dbus/config-store.json, and loads/saves the full ConfigStoreState. | `declared_only` | `unresolved` | `contract_gap` |
| `cozo` | The plugin owns no Cozo handle. op-cozo-store independently contains real CozoGraphShuttle operations and opens RocksDB, but that does not connect cozo methods. | `declared_only` | `unresolved` | `contract_gap` |
| `cron` | current_state returns a hardcoded job list and scheduling defaults. This module has no timer scheduler, job store or execution transport. | `declared_only` | `unresolved` | `contract_gap` |
| `ctl_plane_chatbot` | Typed episode/config data references embedding and Qdrant/Cozo consumers. This plugin object contains no episode store, vector client or actual configuration queue. | `declared_only` | `unresolved` | `contract_gap` |
| `datastore` | DataStorePlugin receives Arc<dyn StateStore> from the registry. Its read_live helper really exports that store and computes object/namespace counts; create_checkpoint also exports it. This store is not inherently durable. | `declared_only` | `unresolved` | `contract_gap` |
| `dnsresolver` | The StatePlugin path reads /etc/resolv.conf directly and atomically replaces it through /etc/resolv.conf.sysdecl.tmp. This is file-backed resolver configuration, not a connection to systemd-resolved. | `declared_only` | `unresolved` | `contract_gap` |
| `embedding_model` | Configuration comes from projection/environment, while API keys are fetched fresh from environment. Ollama helpers use HTTP; Voyage presence is inferred from a configured key rather than an authenticated health request. | `declared_only` | `unresolved` | `contract_gap` |
| `emqx` | Lifecycle and status call the fixed EMQX runit service through system D-Bus. Listener/hook management uses the broker's authenticated local HTTP API; credentials are read internally. Callers cannot select another service. | `source_dispatch` | `unresolved` | `retain_current_path` |
| `endpoint` | No endpoint registry client or storage mutation is called from this plugin. Read methods declare typed list/optional-record outputs, while mutations declare AckOutput. | `declared_only` | `unresolved` | `contract_gap` |
| `factory` | Metadata includes static API/model descriptions and non-secret environment references. BYOM reads a sealed catalog then legacy projection-cache files, including zeroclaw-name fallback. | `declared_only` | `unresolved` | `contract_gap` |
| `fail2ban` | No Fail2ban control socket, service manager or daemon client is invoked by this plugin's public dispatch path or reconciliation implementation. | `declared_only` | `unresolved` | `contract_gap` |
| `freedesktop` | Local maps and canonical path helpers implement structural metadata; standard zbus interfaces elsewhere remain distinct from this plugin's advertised method bodies. | `declared_only` | `unresolved` | `contract_gap` |
| `full_system` | capture_full_state invokes filesystem/native and service/container observation helpers, defaulting failed sections. set_hostname_native uses the kernel syscall then writes /etc/hostname. | `source_dispatch` | `unresolved` | `retain_current_path` |
| `gcloud_adc` | Files beneath the executing process’s home .config/gcloud directory back this observation. No Cozo persistence or external command is used here. | `source_dispatch` | `unresolved` | `retain_current_path` |
| `gemma_brain` | Source reads generator availability from GEMMA_BINARY_PATH and gallery count from /dev/shm/gemma-ui-specs.json; environment settings name the inference plugin. | `declared_only` | `unresolved` | `contract_gap` |
| `ghostbridge` | Projected state or environment-derived declaration backs the result. current_state reports declared status, default TLS/UI properties and an empty endpoint list; it does not inspect live sockets or certificates. | `source_dispatch` | `unresolved` | `retain_current_path` |
| `hardware` | Helpers parse /proc/cpuinfo, /proc/meminfo, /sys/block and mounts directly; failures generally become empty or Unknown data. | `declared_only` | `unresolved` | `contract_gap` |
| `host_runtime` | Dormant helpers read procfs/sysfs and /etc/hostname, invoke sv status, and connect to requested Unix sockets. Those direct observation/subprocess paths conflict with current plugin policy. | `uncompiled` | `unresolved` | `excluded` |
| `human_principal` | A distinct persistent CozoGraphShuttle database holds human-principal records; store handles are cached by path. Strict authorization lookups fail when storage is unavailable. | `source_dispatch` | `unresolved` | `retain_current_path` |
| `identity_sled` | Private persistent Cozo records warm the engine cache; session arrival links the event chain, container binding and sealed identity. Incus helpers provision containers and attach dedicated btrfs device records. | `source_dispatch` | `unresolved` | `retain_current_path` |
| `incus` | Despite the run_incus_command helper name, operations use bounded reqwest HTTP over the Incus Unix socket, with redirects disabled and asynchronous operation handling. Helpers implement creation, deletion, profile/config/device synchronization and power changes. | `source_dispatch` | `unresolved` | `retain_current_path` |
| `json_render` | JsonRenderState::default builds an in-source catalog of packages, components, renderers and directives with a pinned upstream source commit. No external UI process is called. | `declared_only` | `unresolved` | `contract_gap` |
| `keypair` | KeypairState and KeypairInfo describe names, algorithms, public keys and presence. The implementation owns no secret store, key generation backend or filesystem discovery routine. | `declared_only` | `unresolved` | `contract_gap` |
| `keyring` | A connect_service helper targets the freedesktop plugin over the session bus; current list/default helpers do not use it. Availability merely checks that the session-bus socket path exists. | `source_dispatch` | `unresolved` | `contract_gap` |
| `large_language_model` | Separate environment-driven helpers query Ollama-compatible /api/tags and /api/ps endpoints and choose an override, loaded model or first listed model. create_checkpoint invokes that observed-state helper. | `declared_only` | `unresolved` | `contract_gap` |
| `login1` | A private connect_manager helper constructs an org.freedesktop.login1.Manager system-bus proxy, but no caller or actual manager method invocation was located here. | `declared_only` | `unresolved` | `contract_gap` |
| `mail_server` | Private helpers implement Incus HTTP over a Unix socket and treat Running container status as mail health. No current caller of those helpers was located. | `declared_only` | `unresolved` | `contract_gap` |
| `mcp` | McpStatePlugin owns a StateStore and config path, defaulting to /etc/op-dbus/mcp-config.json. Helpers read/write configuration and save/update execution jobs. | `declared_only` | `unresolved` | `contract_gap` |
| `memory` | MemoryPlugin owns no store. current_state is a hardcoded active/sqlite exemplar with three named namespaces and zero counts. This conflicts with the Cozo policy and must not be described as a live SQLite backend. | `declared_only` | `unresolved` | `contract_gap` |
| `mongodb_mcp` | Bridge-owned ExternalMcpUpstream targets a configured local MCP endpoint; current_state reports readiness from marker files and describes the provider as read_only. Database contents and credentials remain upstream. | `source_dispatch` | `unresolved` | `retain_current_path` |
| `net` | StatePlugin uses OvsdbDbusClient, which now wraps native rovs_ovsdb over a Unix socket despite its name. Host addressing uses native rtnetlink and directly rewrites /etc/network/interfaces. | `declared_only` | `unresolved` | `contract_gap` |
| `netmaker` | Unreachable helper code includes HTTP management API calls, direct /etc/netclient/netclient.json reads/writes and a runit D-Bus service controller whose enum/error text still says S6. | `retired` | `unresolved` | `excluded` |
| `notebooklm` | Cozo stores selection; the external nlm CLI owns provider interaction and Google profile state. Dedicated version-4 output types replace a flattened provider map and normalization handles collections and mind maps. | `source_dispatch` | `unresolved` | `retain_current_path` |
| `oci` | No OCI binary, container daemon or Incus lifecycle helper is invoked by the plugin's StatePlugin implementation or canonical Call path. Dependencies alone do not connect operations. | `declared_only` | `unresolved` | `contract_gap` |
| `openflow` | Safety methods use op_network helpers. Reconciliation installs/deletes normalized flows through an OpenFlowController D-Bus proxy; its dump query reports controller tracking, not live switch flow-table discovery. Lower-level OpenFlowClient uses native rovs TCP. | `source_dispatch` | `unresolved` | `retain_current_path` |
| `openflow_obfuscation` | The implementation generates flow command strings and logs them. It does not execute ovs-ofctl, connect to a controller or install generated flows. | `declared_only` | `unresolved` | `contract_gap` |
| `oscal_subid_registry` | The source defines rich OscalSubidRegistryEntry metadata. CozoGraphShuttle separately supports subid registration, while cognitive_mcp.oscal_subids reads sealed-plugin metadata; neither is connected to this plugin’s methods. | `declared_only` | `unresolved` | `contract_gap` |
| `ovsdb_bridge` | OvsdbDbusClient wraps native vendor rovs_ovsdb over the configured/default Unix socket. Its compatibility name does not imply a D-Bus hop. It lazily connects and retries once after errors. | `source_dispatch` | `unresolved` | `retain_current_path` |
| `packagekit` | State helpers create system-bus PackageKit transactions and call InstallPackages/RemovePackages; installed-state checks directly read package databases. | `declared_only` | `unresolved` | `contract_gap` |
| `pcidecl` | Private helpers read vendor/device/driver information from /sys/bus/pci/devices and write driver_override through native filesystem access. | `declared_only` | `unresolved` | `contract_gap` |
| `persona` | Catalog data comes from compiled op-agents implementations; execution uses create_agent and agent.execute. There is no mutable persona database or separate identity authority in this plugin. | `source_dispatch` | `unresolved` | `retain_current_path` |
| `privacy_router` | Source helpers attempt OVS/native rtnetlink, Incus reconciliation and controller operations. However query_privacy_routes, query_incus_state and query_openflow_state deserialize literal null instead of querying state; Incus application repeats this failure. | `uncompiled` | `unresolved` | `excluded` |
| `privacy_routes` | A direct JSON file at /var/lib/op-dbus/privacy-routes.json is the method and reconciliation store. Any file-read error returns an empty store; malformed JSON errors. Writes replace file contents without an atomic temporary-file protocol. | `source_dispatch` | `unresolved` | `retain_current_path` |
| `procfs` | gather_procfs_state joins ten native /proc readers; parser helpers normalize values and units. The top-level gather helper has no located caller in current Rust source. | `declared_only` | `unresolved` | `contract_gap` |
| `project` | A private Cozo catalog shares the identity store; each project has a canonical UUID directory and separate persistent Cozo document store. Incus helpers create/verify the project container and persistent mount. | `source_dispatch` | `unresolved` | `retain_current_path` |
| `proxy_server` | No proxy server, listener, process manager or upstream connection is created or contacted by this plugin. Declared output for each operation is AckOutput. | `declared_only` | `unresolved` | `contract_gap` |
| `qdrant` | Plugin defaults are illustrative version/collection data and environment-derived endpoints; the plugin owns no client. Separately, the cognitive QdrantSemanticShuttle uses the Qdrant gRPC endpoint (normally 6334), with a five-second startup health timeout. Its retrieval implementation does not wire the qdrant plugin's methods. | `declared_only` | `unresolved` | `contract_gap` |
| `rovs_commands` | Operations call OvsdbDbusClient, which is a native rovs_ovsdb Unix-socket client. These are actual backend calls despite the compatibility type and dispatcher comment saying proxy. | `source_dispatch` | `unresolved` | `retain_current_path` |
| `rtnetlink` | Separate StatePlugin apply calls op_network::rtnetlink directly for MAC, IPv4 address, link up/down and default-route replacement. This is native kernel netlink, not a D-Bus backend. | `declared_only` | `unresolved` | `contract_gap` |
| `schema_renderer` | SchemaRendererState::default constructs in-source UI mappings and layouts. No renderer runtime, saved-rendering store or external UI client is called by this plugin. | `declared_only` | `unresolved` | `contract_gap` |
| `service` | Public auto_generate_service constructs ServiceDef; install_service invokes ServiceDef::install. Private listing uses sv status and service directories; no callers of install_service were located. | `declared_only` | `unresolved` | `contract_gap` |
| `sess_decl` | The plugin is an empty struct with data contracts and no session database, login-service proxy or persistence client. | `declared_only` | `unresolved` | `contract_gap` |
| `shared_unix_socket` | Separate reconciliation owns a std::os::unix::net::UnixListener at /run/ghostbridge/container.sock and reads desired configuration from the plugin blob example. | `declared_only` | `unresolved` | `contract_gap` |
| `snowball` | StreamingSnowball stores authoritative timing records plus vector/state projections and supports snapshots. The bridge initializes an optimized sink or timing-only fallback; if both fail it can keep the event chain in memory. | `source_dispatch` | `unresolved` | `retain_current_path` |
| `software` | A private scan_dpkg parser reads /var/lib/dpkg/status directly, but no current caller of that helper was located. It is not a package-manager client. | `declared_only` | `unresolved` | `contract_gap` |
| `tched_router` | Chat uses TchedRouterRuntimeClient over loopback A2A message/send. CLI config methods invoke the upstream executable. Section getters/PatchConfig directly access its config file; catalog/route reads use projected or default TchedRouterState. | `source_dispatch` | `unresolved` | `retain_current_path` |
| `unix_socket` | StatePlugin holds UnixListeners in a mutex map and reads desired endpoints from the blob example. It can create/remove sockets; an existing path is treated as another owner's shared endpoint. | `declared_only` | `unresolved` | `contract_gap` |
| `users` | UsersPlugin is an empty struct; declarations include UID/GID, groups, shell and desired presence. No account service, user database or native mutation helper exists in this module. | `declared_only` | `unresolved` | `contract_gap` |
| `web_ui` | The plugin owns in-memory identity/tunable structs but does not serve the active frontend. Actual serving is op-web routes::create_router using external static files. | `declared_only` | `unresolved` | `contract_gap` |
| `wg_opdbus` | The state embeds WireGuardInterface plus configuration path, identity role and route targets. Custom persona deserialization rejects changes to the canonical principal/persona/email/plugin fields. | `owner_topology` | `unresolved` | `role_unresolved` |
| `wgcf` | No wgcf binary invocation, WARP API client or WireGuard configuration application is called by this plugin's canonical methods or StatePlugin implementation. | `owner_topology` | `unresolved` | `role_unresolved` |
| `wireguard` | WireGuardPlugin is stateless and invokes no kernel WireGuard API, daemon, configuration-file writer or interface-management client. Other plugins embedding its types do not supply this missing dispatch. | `owner_topology` | `unresolved` | `role_unresolved` |
| `workflows` | A process mutex and append-only JSONL journal back admission. The default path is /var/lib/op-dbus/workflows/run-queue.jsonl; cognitive workflow_query/workflow_run reuse these handlers. | `source_dispatch` | `unresolved` | `retain_current_path` |
| `xray` | Statistics use the commander Unix socket through op_xray_daemon. Restart scans /proc for every xray process and signals each. Generated configuration structs cite an upstream extraction and temporary generator; this is source provenance, not current live-version evidence. | `source_dispatch` | `unresolved` | `retain_current_path` |

## Shared call and state paths

| Path | What source establishes |
|---|---|
| D-Bus `PluginV1.Call` | `SchemaBackedInterface::call` resolves sender identity, then `dispatch_call_for_identity` checks method declaration, JSON-schema arguments and exact-principal capability before entering the engine. See [`schema_router.rs`](../../../crates/op-grpc-bridge/src/schema_router.rs), lines 710/820. |
| Generated typed gRPC | The interceptor authenticates the identity; `call_generated_plugin_method_typed` checks the declared capability and grant, converts the generated protobuf request, and enters the same engine. It extracts the domain result for typed response encoding. See [`grpc_server.rs`](../../../crates/op-grpc-bridge/src/grpc_server.rs), line 612. |
| Generic gRPC `PluginService.CallMethod` | Checks schema capability/identity, converts arguments, and enters the same engine; returns an audited envelope. This inspected entrypoint does not invoke the D-Bus JSON-schema validator, so validation parity must be proved per path. See `grpc_server.rs`, line 1249. |
| MCP projected tool | Authentication, audience/toolset selection, provider policy and `authorize_and_validate_tool_call` precede engine dispatch; tool visibility is narrower than the sealed catalog. See [`mcp_frontend.rs`](../../../crates/op-grpc-bridge/src/mcp_frontend.rs), lines 615/665. |
| Common engine | `dispatch_method_call_with_identity` records/broadcasts the attempted method before selecting the backend. Backend errors can therefore follow an audit event. Durability failures are logged without necessarily failing dispatch. See [`mutation_engine.rs`](../../../crates/op-grpc-bridge/src/mutation_engine.rs), line 2097. |
| Separate state application | Compiled `PluginWorkflowNode::execute` calls `StatePlugin::calculate_diff` and `apply_state`, but currently supplies null current state. Typed current-state decoding can therefore fail before apply. Helper implementations supplied valid diffs do not automatically wire declared `PluginV1.Call` methods. The older `MutationEngine::mutate` also has different backend branches. |

[`plugin_workflow.rs`](../../../crates/op-state/src/plugin_workflow.rs), lines
153/185, supplies the compiled alternate path. `manager.rs` remains on disk but
is excluded from the module tree (`op-state/src/lib.rs` explicitly records its
removal). Its presence is not an active route. The production reachability of
each alternate apply path was not established uniformly. Native backing protocols are identified in each
card; D-Bus architectural authority must not be confused with an actual bus-wire
hop inside every transport adapter.

## How to read dispatch classifications

- **explicit:** canonical engine selects plugin-specific code. Its source-status
  and uncertainty fields still identify stubs, rejected operations or incomplete
  effects; this label does not mean fully implemented.
- **partial:** plugin-specific canonical dispatch contains known missing,
  rejected or placeholder operations.
- **generic_echo:** canonical method dispatch falls through to argument echo and
  a success envelope. Separate helpers or state-application effects may exist.
- **none:** no canonical callable path established for an excluded/retired source.

Source status is descriptive. The owner or runtime evidence must establish an
operational status such as deployed, healthy or intentionally retired. Findings
are referenced to the canonical [`WISHLIST.md`](../../../WISHLIST.md), especially
OD-32 (single schema source), OD-55 (truthful execution), OD-56 (router selection)
and OD-46/OD-47 (scoped execution). The cards are not a separate task board.

## Query and refresh

[plugin-baseline.json](plugin-baseline.json) contains the structured evidence
cards. For example, list source-registered plugins that reach generic echo:

```sh
jq -r '.plugins[] | select(.registration == "registered" and .dispatch == "generic_echo") | .id' \
  docs/src/architecture/plugin-baseline.json
```

Before reusing a claim, check its source symbol and relevant fingerprints:

```sh
sha256sum --check --quiet docs/src/architecture/plugin-baseline-files.sha256
```

A mismatch invalidates affected evidence and prompts a focused reread; it is not
a reason to overwrite the manifest and claim the old conclusions remain valid.
Changes to source registration/module membership require coverage reconciliation.
Owner corrections can invalidate a conclusion even if source hashes are unchanged.
See [Documentation Maintenance](../contributors/doc-maintenance.md) for the
general multi-agent refresh procedure.

## Plugin inventory


## Baseline counts

- Registration declarations: **70**
- Compiled and non-retired registrations: **67**
- Explicit dispatch (`explicit` + `partial`): **21**
- Generic-echo dispatch: **46**

Owner-provided topology note: WireGuard terminates at the decoy; there is no WireGuard on this server; identity is transported by WARP. This statement was not live-validated. The aggregate **46** generic-echo entries are source inventory, not 46 required server implementations; each role requires separate disposition.

## Plugin index

| ID | Registration | Dispatch | Source status |
| --- | --- | --- | --- |
| [`adc`](#adc) | registered | `generic_echo` | Registered source declares get_status with typed configured output. Canonical PluginV1.Call reaches the common MutationEngine dispatcher, which has no adc arm and echoes the input. There is no credential-status lookup in this plugin body. |
| [`agent_config`](#agent-config) | registered | `generic_echo` | Five declared methods cover configuration reads/updates/reset and tool listing/registration. Mutation inputs are unit types and return generic AckOutput. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo. |
| [`antigravity`](#antigravity) | registered | `generic_echo` | Three methods declare auth status, usage reports and safety configuration; model catalog responsibility is delegated to large_language_model/gemini. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo. |
| [`antigravity_chat`](#antigravity-chat) | registered | `generic_echo` | Three typed contracts advertise bridge status, token refresh and configuration; source explicitly notes unwired dispatch and excludes a duplicate model catalog. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo. |
| [`btrfs`](#btrfs) | registered | `generic_echo` | Compiled inventory scaffold. Its current_state supplies fixed active status, named subvolumes and daily snapshot/DR settings; these are examples, not observed storage. |
| [`cognitive_mcp`](#cognitive-mcp) | registered | `partial` | Canonical dispatch has explicit health/list/schema/subid/toolset/workflow branches and translates remaining methods into ToolRegistry calls. Coverage is partial: register_tool maps to a reachable unimplemented stub; optional retrieval/provider tools can be absent. |
| [`config`](#config) | registered | `generic_echo` | Five schema methods cover listing, reading, setting, deleting and exporting. Canonical PluginV1.Call has no config arm and echoes arguments; real configuration mutation nevertheless exists on the separate StatePlugin apply path. |
| [`cozo`](#cozo) | registered | `generic_echo` | cozo_schema declares query, relation import/export, backup/restore and relation inspection. All seven methods fall through the canonical dispatcher to argument echo. Methods use generic AckOutput, including operations that should return query or export data. |
| [`cron`](#cron) | registered | `generic_echo` | Nine methods advertise list/get/create/update/delete/enable/disable/run/history. All method inputs are unit types; mutation outputs use AckOutput. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo. |
| [`ctl_plane_chatbot`](#ctl-plane-chatbot) | registered | `generic_echo` | Seven methods advertise configuration, episode/context reads, significance classification and vectorization; requests currently use unit input contracts. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo. |
| [`datastore`](#datastore) | registered | `generic_echo` | Five read-oriented methods are declared, but no datastore arm exists in canonical MutationEngine dispatch. The admitted method call therefore returns the input rather than invoking the shared StateStore. |
| [`dnsresolver`](#dnsresolver) | registered | `generic_echo` | Compiled source contains twenty resolved-style method declarations, all returning AckOutput; canonical Call has no DNS dispatcher and echoes input. |
| [`embedding_model`](#embedding-model) | registered | `generic_echo` | The schema declares embed, list_models, get_config, set_model and update_config, but canonical method dispatch echoes every call. Separate helpers are operational: voyage_embed_params supplies the cognitive Voyage client; create_checkpoint calls provider-observing live_state. |
| [`emqx`](#emqx) | registered | `explicit` | Compiled plugin with an explicit typed dispatcher covering all nine declared methods. Declared initial state is deterministic metadata, not a health probe. |
| [`endpoint`](#endpoint) | registered | `generic_echo` | Compiled schema scaffold without an endpoint dispatcher. All four method inputs are unit structs, so even get/add/remove carry no endpoint identifier or record. |
| [`factory`](#factory) | registered | `generic_echo` | Six methods advertise computers/sessions/models and BYOM/autonomy changes. The schema is assembled from current_state field values and declares a tched_router dependency. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo. |
| [`fail2ban`](#fail2ban) | registered | `generic_echo` | Compiled scaffold. current_state hardcodes active status, sshd/recidive jail data and counters rather than reading the daemon. The four methods have typed inputs but generic AckOutput responses. |
| [`freedesktop`](#freedesktop) | registered | `generic_echo` | Seven builder-defined methods use generic AckOutput rather than their actual standard result shapes. An in-memory interface inventory describes the standard interfaces. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo. |
| [`full_system`](#full-system) | registered | `explicit` | Both declared methods, capture_state and set_hostname, have explicit dispatcher cases; unknown methods return errors. Broad recovery wording does not establish full-system restore.  capture_full_state initializes the plugins map empty; the former StateManager that was intended to fill it is excluded from the compiled module tree. |
| [`gcloud_adc`](#gcloud-adc) | registered | `explicit` | The single check_auth_status method has an explicit canonical dispatch arm to the plugin helper. It checks ADC-file existence and reads the active gcloud configuration to identify account/project; it does not exchange tokens or authenticate to Google. |
| [`gemma_brain`](#gemma-brain) | registered | `generic_echo` | Seven methods advertise classification, routing, perspective/spec/tag reads, registration and intent analysis. Comments asserting MutationEngine implementation are contradicted by the actual dispatch match. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo. |
| [`ghostbridge`](#ghostbridge) | registered | `explicit` | All four methods have explicit dispatch: GetState, GetIdentity, ListEndpoints and GetGhostrunner. The bridge first resolves projected state, falling back to GhostbridgePlugin::current_state; the plugin dispatcher returns the corresponding typed domain object. |
| [`hardware`](#hardware) | registered | `generic_echo` | Five schema methods use unit inputs; scan/refresh return AckOutput. Private inventory helpers exist but have no located callers in the current Rust source. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo. |
| [`host_runtime`](#host-runtime) | uncompiled | `none` | The source contains seven typed methods and dispatch_host_runtime_method, but state_plugins/mod.rs has no host_runtime module and the bridge has no matching arm.  |
| [`human_principal`](#human-principal) | registered | `explicit` | All six schema methods have explicit handlers. register_key validates the key, derives principal_id server-side, rejects existing keys including revoked tombstones and rejects duplicate active aliases. revoke_key preserves the original revocation timestamp on repeats. |
| [`identity_sled`](#identity-sled) | registered | `explicit` | Seven methods have explicit canonical dispatch. The schema lives in identity_sled.rs; a generated content-hash include fingerprints its record shape rather than defining a second schema. The engine authors SID1 and rejects caller-supplied credential fields. |
| [`incus`](#incus) | registered | `partial` | Compiled plugin with substantive Incus REST helpers, but canonical Call dispatches only delete_instance and remove_device; other declared methods fall through to generic echo. |
| [`json_render`](#json-render) | registered | `generic_echo` | Fifteen methods have typed inputs/outputs, including validate_spec and build_prompt_surface, but their implementations are not connected to canonical method dispatch. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo. |
| [`keypair`](#keypair) | registered | `generic_echo` | The plugin describes itself as a reference schema pattern, but its single list_keypairs method has no canonical dispatch arm. PluginV1.Call returns input through the common fallback, so the typed output declaration does not establish discovery. |
| [`keyring`](#keyring) | registered | `partial` | Both schema methods have an explicit canonical handler, but get_collections unconditionally returns an empty vector and get_default_collection returns None. This is routed functionality with a placeholder backing implementation, not generic argument echo or evidence of an empty real keyring. |
| [`large_language_model`](#large-language-model) | registered | `generic_echo` | The schema declares provider/model/configuration and generation methods, but the canonical dispatch match has no large_language_model arm. Several mutation/generation requests are empty unit inputs with AckOutput, so neither generation payloads nor streaming execution are established. |
| [`login1`](#login1) | registered | `generic_echo` | Twenty-three methods advertise session/user/seat reads and mutations including power-off, reboot and sleep. Generic AckOutput contracts overstate meaningful return data. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo. |
| [`mail_server`](#mail-server) | registered | `generic_echo` | Nine methods advertise domain, mailbox, alias, quota and queue mutations/reads. AckOutput is used even for queue data; canonical dispatch has no mail backend. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo. |
| [`mcp`](#mcp) | registered | `generic_echo` | Three typed methods declare get_config, configure_server and configure_tool_groups; comments correctly note missing method dispatcher instance dependencies. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo. |
| [`memory`](#memory) | registered | `generic_echo` | The registered memory plugin declares namespace creation/deletion, entry CRUD, statistics and compaction, but has no canonical dispatch arm. The common fallback returns the input for admitted calls rather than interacting with a memory database. |
| [`mongodb_mcp`](#mongodb-mcp) | registered | `explicit` | The canonical mongodb_mcp arm converts schema underscores to upstream hyphens and invokes ExternalMcpClient::call_tool. The plugin’s read_method macro declares typed inputs with a flattened ProviderResult output; this is explicit provider forwarding, not an embedded MongoDB store. |
| [`net`](#net) | registered | `generic_echo` | Compiled plugin; canonical apply_interface and delete_ovs_bridge method calls echo inputs. The older mutate entry separately handles some OVS operations. |
| [`netmaker`](#netmaker) | retired | `none` | The module and inventory constructor remain compiled, but default_registry explicitly retires netmaker and the legacy mutate path rejects its invocation. Treat it as retired code, not another supported networking product. |
| [`notebooklm`](#notebooklm) | registered | `partial` | Canonical dispatch is explicit and verifies the caller session. It stores notebook selection per session, then maps methods to the pinned jacob-bd nlm executable. Coverage is partial: declared notebook_query_start and notebook_query_status deliberately error because the CLI has no equivalent. |
| [`oci`](#oci) | registered | `generic_echo` | Compiled schema scaffold with incus and rovs_commands dependencies. Its description promises loopback/network-namespace initialization, but no corresponding runtime implementation appears in this plugin. |
| [`openflow`](#openflow) | registered | `partial` | Compiled plugin; six safety/controller methods explicitly dispatch. add_flow, delete_flow and modify_flow explicitly error and direct callers to StatePlugin apply or the separate SendFlow interface. |
| [`openflow_obfuscation`](#openflow-obfuscation) | registered | `generic_echo` | Compiled source contains substantive Rust flow-string generation at several security/obfuscation levels, but canonical methods have no backend dispatcher. |
| [`oscal_subid_registry`](#oscal-subid-registry) | registered | `generic_echo` | Five schema methods declare register, lookup, list_by_category, resolve and export, but every request uses an empty unit input and canonical dispatch has no oscal_subid_registry arm. Calls therefore reach generic argument echo rather than a registry query. |
| [`ovsdb_bridge`](#ovsdb-bridge) | registered | `partial` | Compiled schema has fourteen methods. Canonical Call implements only list_dbs, get_schema and transact; every other declared operation returns a missing-dispatch error. |
| [`packagekit`](#packagekit) | registered | `generic_echo` | Six method contracts are advertised but canonical dispatch does not call PackageKit. Separate state application is implemented for selected package changes. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo. |
| [`pcidecl`](#pcidecl) | registered | `generic_echo` | Two read methods advertise enumeration/device information with AckOutput. They are independent of the implemented StatePlugin reconciliation path. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo. |
| [`persona`](#persona) | registered | `partial` | The canonical persona arm reaches a plugin-owned dispatcher. get_identity reads op_agents::builtin_agent_descriptors and execute constructs an AgentTask then calls the chosen agent. Coverage is partial: set_identity always rejects and authenticate always returns false without issuing a token. |
| [`privacy_router`](#privacy-router) | uncompiled | `none` | A source file and inventory macro exist, but state_plugins/mod.rs does not declare the module. Therefore its constructor, schema, tests and helpers are not compiled into the current plugin inventory. |
| [`privacy_routes`](#privacy-routes) | registered | `explicit` | Compiled plugin with both declared methods explicitly dispatched. Its separate schema dependencies include wireguard and the uncompiled privacy_router module. |
| [`procfs`](#procfs) | registered | `generic_echo` | Seven methods advertise process and host-statistic reads, with generic acknowledgement result types. Detailed parsers and a gather function exist independently of canonical dispatch. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo. |
| [`project`](#project) | registered | `explicit` | All ten methods have an explicit canonical dispatch path. The dispatcher revalidates the session, genesis, active registered principal and exact method grant, then serializes ACL and document access. Project ceilings intersect existing grants rather than create authority. |
| [`proxy_server`](#proxy-server) | registered | `generic_echo` | Compiled minimal schema scaffold. Methods carry typed request fields such as proxy name, port, enabled and upstream, but canonical Call has no proxy-specific implementation. |
| [`qdrant`](#qdrant) | registered | `generic_echo` | Ten methods are present in qdrant_schema, but the canonical dispatcher has no qdrant arm and echoes the supplied input. The QdrantPlugin body owns no client, while another crate has a real QdrantSemanticShuttle. |
| [`rovs_commands`](#rovs-commands) | registered | `explicit` | Compiled plugin; all eight schema methods have explicit MutationEngine implementations. Its standalone StatePlugin remains schema-only, with empty diff and effect-free apply. |
| [`rtnetlink`](#rtnetlink) | registered | `generic_echo` | Compiled plugin with nine method declarations but no canonical method dispatcher. Its manual state schema models addresses as CIDR strings, while the Rust reconciler expects AddressEntry objects containing ip and prefix. |
| [`schema_renderer`](#schema-renderer) | registered | `generic_echo` | Four methods advertise rendering, gallery listing, rendering lookup and validation. Requests are unit inputs despite operations needing meaningful schema or identifier data. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo. |
| [`service`](#service) | registered | `generic_echo` | Three methods advertise init, run and shutdown using typed inputs and AckOutput. Their canonical dispatch has no corresponding service execution handler. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo. |
| [`sess_decl`](#sess-decl) | registered | `generic_echo` | Two typed methods declare list_sessions and declare_session. Source explicitly says declarations are not durably stored and method dispatch is unwired. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo. |
| [`shared_unix_socket`](#shared-unix-socket) | registered | `generic_echo` | Compiled schema offers create_unix_socket, list_registrations and get_config, but canonical Call only echoes arguments. The helper returns deterministic metadata without storing registrations. |
| [`snowball`](#snowball) | registered | `partial` | Canonical dispatch implements query_events and verify_chain against the engine EventChain. Seven other declared methods explicitly fall through to argument echo. Snapshot and state persistence functionality exists separately on SnowballPlugin’s StatePlugin methods and StreamingSnowball. |
| [`software`](#software) | registered | `generic_echo` | Four methods have typed inputs but generic AckOutput returns, including get_info. Their canonical method path has no package execution branch. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo. |
| [`tched_router`](#tched-router) | registered | `explicit` | Explicit method dispatch exists. The separate captured-schema tree violates the single-source rule, and selection setters update projected state without a traced upstream selection call. |
| [`unix_socket`](#unix-socket) | registered | `generic_echo` | Compiled plugin with generic-echo canonical methods, but a separate legacy MutationEngine::mutate bind/createunixsocket branch calls create_unix_socket and updates projected registration state. |
| [`users`](#users) | registered | `generic_echo` | Four methods expose typed account inputs but all return AckOutput, including list_users. No canonical user-management dispatch implementation is present. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo. |
| [`web_ui`](#web-ui) | registered | `generic_echo` | Three methods advertise render_page, handle_event and update_component with AckOutput. Existing identity/default tests retain obsolete rust-embed assumptions. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo. |
| [`wg_opdbus`](#wg-opdbus) | registered | `generic_echo` | Compiled identity/schema projection, not a second kernel WireGuard implementation. Schema declares create_interface and get_status and depends on wireguard; canonical calls have no dedicated backend. |
| [`wgcf`](#wgcf) | registered | `generic_echo` | Compiled scaffold with eight typed method declarations returning AckOutput. current_state provides fixed software/configuration/tool metadata rather than probing a WARP account, interface or upstream executable. |
| [`wireguard`](#wireguard) | registered | `generic_echo` | Compiled schema-only plugin. It declares set_device, get_device, add_peer, remove_peer, list_peers and set_config; read methods have typed outputs but canonical Call echoes inputs. |
| [`workflows`](#workflows) | registered | `partial` | All five method names reach an explicit dispatcher. query_workflows returns static templates; start_workflow queues known IDs; pause/resume/cancel deliberately return errors because no local executor owns transitions.  |
| [`xray`](#xray) | registered | `partial` | Compiled partial dispatcher: get_stats calls Xray StatsService; restart sends SIGHUP. Six user/inbound/trace methods explicitly error. The comment claiming only statistics works is stale. |

<a id="adc"></a>

## `adc`

**Purpose.** Describe whether application default credentials are configured through the adc plugin.

**Source status.** Registered source declares get_status with typed configured output. Canonical PluginV1.Call reaches the common MutationEngine dispatcher, which has no adc arm and echoes the input. There is no credential-status lookup in this plugin body.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/adc.rs::adc_schema`

**Dispatch.** `generic_echo`

**Backing.** AdcState is a single configured boolean; no provider, credential store or durable database is connected here.

**State effects.** StatePlugin calculate_diff is empty, apply_state returns success with no changes, verification always returns true and checkpoint state is null. An admitted method call still appends an audit event, but that is not evidence that credentials were checked.

**Test evidence.** all_subids_are_valid inspects schema taxonomy only; no functional credential-status test was located in this file.

**Uncertainty.** The separately implemented gcloud_adc plugin must not be mistaken for an adc handler. Runtime blob presence and actual ADC usability remain unverified.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/adc.rs](../../../crates/op-plugins/src/state_plugins/adc.rs) — line 101, `adc_schema`: Declares status contract.
- [crates/op-plugins/src/state_plugins/adc.rs](../../../crates/op-plugins/src/state_plugins/adc.rs) — line 62, `AdcPlugin::apply_state`: No state changes.
- [crates/op-plugins/src/state_plugins/adc.rs](../../../crates/op-plugins/src/state_plugins/adc.rs) — line 146, `tests::all_subids_are_valid`: Taxonomy-only test.
- [crates/op-plugins/src/state_plugins/adc.rs](../../../crates/op-plugins/src/state_plugins/adc.rs) — line 174, `inventory::submit!`: Source registration.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2356, `MutationEngine::dispatch_method_call_with_identity`: Canonical match has no arm for this ID and returns parsed input; outer result still says success.

<a id="agent-config"></a>

## `agent_config`

**Purpose.** Describe agent configuration, enabled models and assigned tool names for the control plane.

**Source status.** Five declared methods cover configuration reads/updates/reset and tool listing/registration. Mutation inputs are unit types and return generic AckOutput. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/agent_config.rs:133 agent_config_schema`

**Dispatch.** `generic_echo`

**Backing.** This plugin is an empty struct with schema-derived data declarations; no external agent configuration store or execution client is implemented here.

**State effects.** calculate_diff produces no actions, apply reports success without changes, verification returns true, and checkpoints contain null state. These acknowledgements do not establish configuration persistence.

**Test evidence.** Local schema_is_schemars_seeded_and_typed and subid tests check contract metadata, not successful configuration or tool mutation.

**Uncertainty.** Actual agent configuration mechanisms elsewhere must be traced separately; these five advertised methods have no established execution effect. No runtime presence or deployment was checked.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/agent_config.rs](../../../crates/op-plugins/src/state_plugins/agent_config.rs) — line 133, `agent_config_schema`: Plugin-owned method contract and source description.
- [crates/op-plugins/src/state_plugins/agent_config.rs](../../../crates/op-plugins/src/state_plugins/agent_config.rs) — line 95, `StatePlugin::apply_state`: Separate state-application behavior described above.
- [crates/op-plugins/src/state_plugins/agent_config.rs](../../../crates/op-plugins/src/state_plugins/agent_config.rs) — line 280, `inventory::submit! / PluginReg::new`: Compiled source registration, not live blob presence.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2356, `MutationEngine::dispatch_method_call_with_identity`: No dedicated plugin arm; canonical method dispatch reaches the generic request echo.

<a id="antigravity"></a>

## `antigravity`

**Purpose.** Describe Google OAuth/Vertex authentication, generation settings, safety settings and usage configuration.

**Source status.** Three methods declare auth status, usage reports and safety configuration; model catalog responsibility is delegated to large_language_model/gemini. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/antigravity.rs:1198 antigravity_schema`

**Dispatch.** `generic_echo`

**Backing.** current_state combines environment-derived project/location with a substantial static configuration inventory. It sets token_valid and API-key configured flags without an authentication probe.

**State effects.** No-op diff/apply and constant verification provide no token refresh, provider request, safety persistence or quota query. Checkpoint construction snapshots the generated configuration metadata.

**Test evidence.** Two local tests verify typed schema seeding, removed duplicate model fields and OSCAL subids; they do not authenticate Google.

**Uncertainty.** Static active/auth flags must not be interpreted as provider readiness. Separate native provider clients were not exercised. No runtime presence or deployment was checked.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/antigravity.rs](../../../crates/op-plugins/src/state_plugins/antigravity.rs) — line 1198, `antigravity_schema`: Plugin-owned method contract and source description.
- [crates/op-plugins/src/state_plugins/antigravity.rs](../../../crates/op-plugins/src/state_plugins/antigravity.rs) — line 1160, `StatePlugin::apply_state`: Separate state-application behavior described above.
- [crates/op-plugins/src/state_plugins/antigravity.rs](../../../crates/op-plugins/src/state_plugins/antigravity.rs) — line 1374, `inventory::submit! / PluginReg::new`: Compiled source registration, not live blob presence.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2356, `MutationEngine::dispatch_method_call_with_identity`: No dedicated plugin arm; canonical method dispatch reaches the generic request echo.
- [crates/op-plugins/src/state_plugins/antigravity.rs](../../../crates/op-plugins/src/state_plugins/antigravity.rs) — line 664, `AntigravityPlugin::current_state`: Environment/static defaults include unprobed token_valid=true.

<a id="antigravity-chat"></a>

## `antigravity_chat`

**Purpose.** Describe the Google chat OAuth bridge and legacy headless IDE integration configuration.

**Source status.** Three typed contracts advertise bridge status, token refresh and configuration; source explicitly notes unwired dispatch and excludes a duplicate model catalog. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/antigravity_chat.rs:223 antigravity_chat_schema`

**Dispatch.** `generic_echo`

**Backing.** current_state supplies a loopback bridge URL, offline status, invalid-token default and headless display metadata, rather than calling the bridge.

**State effects.** Diff/apply are empty, verification is constant true, and checkpointing captures defaults. No refresh_token request, session start or persisted configure operation occurs through this plugin.

**Test evidence.** Local schema seeding and subid tests validate the declared fields and removal of duplicate model inventory only.

**Uncertainty.** Legacy bridge/display labels describe source defaults, not a required assistant container; the assistant remains the control-plane chatbot. No runtime presence or deployment was checked.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/antigravity_chat.rs](../../../crates/op-plugins/src/state_plugins/antigravity_chat.rs) — line 223, `antigravity_chat_schema`: Plugin-owned method contract and source description.
- [crates/op-plugins/src/state_plugins/antigravity_chat.rs](../../../crates/op-plugins/src/state_plugins/antigravity_chat.rs) — line 190, `StatePlugin::apply_state`: Separate state-application behavior described above.
- [crates/op-plugins/src/state_plugins/antigravity_chat.rs](../../../crates/op-plugins/src/state_plugins/antigravity_chat.rs) — line 373, `inventory::submit! / PluginReg::new`: Compiled source registration, not live blob presence.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2356, `MutationEngine::dispatch_method_call_with_identity`: No dedicated plugin arm; canonical method dispatch reaches the generic request echo.
- [crates/op-plugins/src/state_plugins/antigravity_chat.rs](../../../crates/op-plugins/src/state_plugins/antigravity_chat.rs) — line 136, `AntigravityChatPlugin::current_state`: Static offline bridge and invalid token defaults.

<a id="btrfs"></a>

## `btrfs`

**Purpose.** Declares filesystem, subvolume, snapshot, scrub, balance and device operations.

**Source status.** Compiled inventory scaffold. Its current_state supplies fixed active status, named subvolumes and daily snapshot/DR settings; these are examples, not observed storage.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/btrfs_plugin.rs:198 — btrfs_schema / BtrfsState`

**Dispatch.** `generic_echo`

**Backing.** No btrfs backend is invoked by this plugin's canonical methods or StatePlugin implementation. The independent deployment send/receive mechanism does not wire these methods.

**State effects.** calculate_diff is empty, apply_state reports success without effects, verify_state always returns true, checkpoint copies the fixed example, and rollback does nothing. Canonical Call returns its arguments inside the audited success envelope.

**Test evidence.** No local test block found in the inspected plugin module.

**Uncertainty.** The schema's backup/DR model conflicts with the owner's transport-only, at-most-five-subvolumes rule. Source registration establishes eligibility, not presence in the sealed catalog or usable filesystem management.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/mod.rs](../../../crates/op-plugins/src/state_plugins/mod.rs) — line 19, `pub mod btrfs_plugin`: Compiled module membership; source registration does not prove live blob presence.
- [crates/op-plugins/src/state_plugins/btrfs_plugin.rs](../../../crates/op-plugins/src/state_plugins/btrfs_plugin.rs) — line 132, `BtrfsPlugin::current_state`: Fixed sample storage state and backup/DR narrative.
- [crates/op-plugins/src/state_plugins/btrfs_plugin.rs](../../../crates/op-plugins/src/state_plugins/btrfs_plugin.rs) — line 154, `StatePlugin for BtrfsPlugin`: Empty diff, effect-free apply, unconditional verification and ineffective checkpoint/rollback.
- [crates/op-plugins/src/state_plugins/btrfs_plugin.rs](../../../crates/op-plugins/src/state_plugins/btrfs_plugin.rs) — line 198, `btrfs_schema`: Typed storage state and declared method surface.
- [crates/op-plugins/src/state_plugins/btrfs_plugin.rs](../../../crates/op-plugins/src/state_plugins/btrfs_plugin.rs) — line 459, `inventory::submit! / PluginReg::new`: Source inventory constructor.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2187, `dispatch_method_call_with_identity backend match`: No explicit arm for this plugin; default at line 2356 echoes arguments in success envelope.

<a id="cognitive-mcp"></a>

## `cognitive_mcp`

**Purpose.** Project the bridge’s in-process cognitive memory, code retrieval, schema inspection, workflow and tool operations.

**Source status.** Canonical dispatch has explicit health/list/schema/subid/toolset/workflow branches and translates remaining methods into ToolRegistry calls. Coverage is partial: register_tool maps to a reachable unimplemented stub; optional retrieval/provider tools can be absent.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/cognitive_mcp.rs::cognitive_mcp_schema`

**Dispatch.** `partial`

**Backing.** The bridge lazily opens CognitiveMcpServer::new_durable with Cozo; Qdrant/Voyage retrieval is optional. Runtime tool descriptors are implementation candidates and must derive authority from the plugin’s single schema, never become parallel contracts.

**State effects.** Memory mutations persist via Cozo; code indexing writes Qdrant; Rust/agent/workflow calls have their own effects. StatePlugin rejects nonempty apply operations and has no independent service lifecycle or checkpoint.

**Test evidence.** Tests cover method-to-tool mapping, authority metadata, retired service methods and read-only StatePlugin behavior; memory tests filter expired entries. None were executed.

**Uncertainty.** OD-46/47 still track scoped context/delivery: proactive lookup uses unscoped namespace queries and no subscribe_pushes caller was located. No private delivery, provider availability, current catalog or live durability was established.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/cognitive_mcp.rs](../../../crates/op-plugins/src/state_plugins/cognitive_mcp.rs) — line 869, `cognitive_mcp_schema`: Single plugin contract.
- [crates/op-plugins/src/state_plugins/cognitive_mcp.rs](../../../crates/op-plugins/src/state_plugins/cognitive_mcp.rs) — line 96, `CognitiveMcpPlugin::apply_state`: Rejects mutable projection.
- [crates/op-plugins/src/state_plugins/cognitive_mcp.rs](../../../crates/op-plugins/src/state_plugins/cognitive_mcp.rs) — line 1327, `inventory::submit!`: Registration.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 3218, `MutationEngine::dispatch_cognitive_mcp_method`: Canonical operation mapping.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 961, `MutationEngine::cognitive_mcp`: Durable production constructor.
- [crates/op-cognitive-mcp/src/cognitive_tools.rs](../../../crates/op-cognitive-mcp/src/cognitive_tools.rs) — line 218, `RegisterToolTool::execute`: Explicitly unimplemented.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 3527, `tests::memory_methods_select_the_canonical_memory_operation`: Mapping test.
- [crates/op-cognitive-mcp/src/context_awareness.rs](../../../crates/op-cognitive-mcp/src/context_awareness.rs) — line 824, `ContextAwarenessEngine`: Unscoped memory query.

<a id="config"></a>

## `config`

**Purpose.** Maintain the global key/value configuration store through a declared plugin contract.

**Source status.** Five schema methods cover listing, reading, setting, deleting and exporting. Canonical PluginV1.Call has no config arm and echoes arguments; real configuration mutation nevertheless exists on the separate StatePlugin apply path.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/config.rs::config_plugin_schema`

**Dispatch.** `generic_echo`

**Backing.** ConfigPlugin owns a configurable JSON file path, defaulting to /etc/op-dbus/config-store.json, and loads/saves the full ConfigStoreState.

**State effects.** calculate_diff computes create/modify/delete actions; apply_state updates entries then writes the file. This existing direct-file behavior is implementation drift against the owner’s control-plane rule, not an approved alternative. Checkpoint creation stores null while rollback expects a ConfigStoreState, so advertised rollback lacks a coherent snapshot.

**Test evidence.** Local tests verify schema ownership, method presence and subids, not file mutation through PluginV1.Call. No tests were run.

**Uncertainty.** Future work must establish the intended persistence and canonical dispatch binding without creating another schema source or assuming the working apply helper makes the method surface functional.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/config.rs](../../../crates/op-plugins/src/state_plugins/config.rs) — line 66, `ConfigPlugin::load_store`: File backing.
- [crates/op-plugins/src/state_plugins/config.rs](../../../crates/op-plugins/src/state_plugins/config.rs) — line 94, `config_plugin_schema`: Method source.
- [crates/op-plugins/src/state_plugins/config.rs](../../../crates/op-plugins/src/state_plugins/config.rs) — line 243, `ConfigPlugin::apply_state`: Real separate apply behavior.
- [crates/op-plugins/src/state_plugins/config.rs](../../../crates/op-plugins/src/state_plugins/config.rs) — line 282, `ConfigPlugin::create_checkpoint`: Null snapshot versus rollback deserialization.
- [crates/op-plugins/src/state_plugins/config.rs](../../../crates/op-plugins/src/state_plugins/config.rs) — line 314, `tests::should_publish_plugin_owned_config_schema`: Schema tests.
- [crates/op-plugins/src/state_plugins/config.rs](../../../crates/op-plugins/src/state_plugins/config.rs) — line 372, `inventory::submit!`: Registration and default path.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2356, `MutationEngine::dispatch_method_call_with_identity`: Canonical match has no arm for this ID and returns parsed input; outer result still says success.

<a id="cozo"></a>

## `cozo`

**Purpose.** Expose a Cozo relational, graph and vector database administration surface.

**Source status.** cozo_schema declares query, relation import/export, backup/restore and relation inspection. All seven methods fall through the canonical dispatcher to argument echo. Methods use generic AckOutput, including operations that should return query or export data.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/cozo.rs::cozo_schema`

**Dispatch.** `generic_echo`

**Backing.** The plugin owns no Cozo handle. op-cozo-store independently contains real CozoGraphShuttle operations and opens RocksDB, but that does not connect cozo methods.

**State effects.** StatePlugin diff/apply/rollback are no-ops. Schema defaults are illustrative relations and version data; engine() labels a nonempty path sled despite the real persistent shuttle using rocksdb. Admitted calls add audit records without database operations.

**Test evidence.** No local cozo plugin behavior tests were found; shuttle tests elsewhere establish a different entrypoint and cannot prove plugin dispatch.

**Uncertainty.** Connection of declared methods to a single plugin-owned implementation remains missing. Exemplar defaults and environment-derived paths are not live database introspection.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/cozo.rs](../../../crates/op-plugins/src/state_plugins/cozo.rs) — line 181, `CozoPlugin::exemplar_state`: Illustrative defaults.
- [crates/op-plugins/src/state_plugins/cozo.rs](../../../crates/op-plugins/src/state_plugins/cozo.rs) — line 245, `CozoPlugin::apply_state`: No-op apply.
- [crates/op-plugins/src/state_plugins/cozo.rs](../../../crates/op-plugins/src/state_plugins/cozo.rs) — line 282, `cozo_schema`: Seven method declarations.
- [crates/op-plugins/src/state_plugins/cozo.rs](../../../crates/op-plugins/src/state_plugins/cozo.rs) — line 478, `inventory::submit!`: Source registration.
- [crates/op-cozo-store/src/lib.rs](../../../crates/op-cozo-store/src/lib.rs) — line 170, `CozoGraphShuttle::new_persistent`: Actual persistent engine is RocksDB.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2356, `MutationEngine::dispatch_method_call_with_identity`: Canonical match has no arm for this ID and returns parsed input; outer result still says success.

<a id="cron"></a>

## `cron`

**Purpose.** Declare scheduled agent jobs, scheduler options and run-history operations.

**Source status.** Nine methods advertise list/get/create/update/delete/enable/disable/run/history. All method inputs are unit types; mutation outputs use AckOutput. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/cron.rs:228 cron_schema`

**Dispatch.** `generic_echo`

**Backing.** current_state returns a hardcoded job list and scheduling defaults. This module has no timer scheduler, job store or execution transport.

**State effects.** Diff/apply are no-ops; checkpoints serialize default jobs, rollback does nothing and verify returns true. Advertised run or create calls therefore do not establish scheduled execution.

**Test evidence.** The local all_subids_are_valid test checks annotations only; no scheduler behavior or history persistence test was found in the inspected plugin/bridge test paths.

**Uncertainty.** Other cron service implementations do not prove this plugin's methods work. Static enabled jobs and next-run fields are not runtime evidence. No runtime presence or deployment was checked.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/cron.rs](../../../crates/op-plugins/src/state_plugins/cron.rs) — line 228, `cron_schema`: Plugin-owned method contract and source description.
- [crates/op-plugins/src/state_plugins/cron.rs](../../../crates/op-plugins/src/state_plugins/cron.rs) — line 194, `StatePlugin::apply_state`: Separate state-application behavior described above.
- [crates/op-plugins/src/state_plugins/cron.rs](../../../crates/op-plugins/src/state_plugins/cron.rs) — line 406, `inventory::submit! / PluginReg::new`: Compiled source registration, not live blob presence.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2356, `MutationEngine::dispatch_method_call_with_identity`: No dedicated plugin arm; canonical method dispatch reaches the generic request echo.

<a id="ctl-plane-chatbot"></a>

## `ctl_plane_chatbot`

**Purpose.** Define the control-plane chatbot's reasoning episodes, significance, privacy annotations and vectorization configuration.

**Source status.** Seven methods advertise configuration, episode/context reads, significance classification and vectorization; requests currently use unit input contracts. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/ctl_plane_chatbot.rs:635 ctl_plane_chatbot_schema`

**Dispatch.** `generic_echo`

**Backing.** Typed episode/config data references embedding and Qdrant/Cozo consumers. This plugin object contains no episode store, vector client or actual configuration queue.

**State effects.** calculate_diff produces field changes. apply only formats each action into a 'queued' receipt; it does not enqueue or persist anything. Checkpoints contain null and verification is unconditional. These helper effects assume a valid supplied diff. The compiled PluginWorkflowNode currently supplies null current state, which this plugin's typed calculate_diff cannot decode.

**Test evidence.** The local subid test validates annotations; it does not prove episode closure, embedding, deduplication or delivery into Qdrant.

**Uncertainty.** Separate chatbot pipeline implementations require independent tracing. Assistant means this control-plane chatbot, never a presumed assistant container. No runtime presence or deployment was checked.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/ctl_plane_chatbot.rs](../../../crates/op-plugins/src/state_plugins/ctl_plane_chatbot.rs) — line 635, `ctl_plane_chatbot_schema`: Plugin-owned method contract and source description.
- [crates/op-plugins/src/state_plugins/ctl_plane_chatbot.rs](../../../crates/op-plugins/src/state_plugins/ctl_plane_chatbot.rs) — line 580, `StatePlugin::apply_state`: Separate state-application behavior described above.
- [crates/op-plugins/src/state_plugins/ctl_plane_chatbot.rs](../../../crates/op-plugins/src/state_plugins/ctl_plane_chatbot.rs) — line 803, `inventory::submit! / PluginReg::new`: Compiled source registration, not live blob presence.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2356, `MutationEngine::dispatch_method_call_with_identity`: No dedicated plugin arm; canonical method dispatch reaches the generic request echo.
- [crates/op-plugins/src/state_plugins/ctl_plane_chatbot.rs](../../../crates/op-plugins/src/state_plugins/ctl_plane_chatbot.rs) — line 544, `CtlPlaneChatbotPlugin::calculate_diff`: Real config diff exists independently of receipt-only apply.
- [crates/op-state/src/plugin_workflow.rs](../../../crates/op-state/src/plugin_workflow.rs) — line 153, `PluginWorkflowNode::execute`: Compiled alternate workflow supplies null current state before calculate_diff.

<a id="datastore"></a>

## `datastore`

**Purpose.** Describe canonical stored objects, namespaces, execution counts and export data.

**Source status.** Five read-oriented methods are declared, but no datastore arm exists in canonical MutationEngine dispatch. The admitted method call therefore returns the input rather than invoking the shared StateStore.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/datastore.rs::datastore_schema`

**Dispatch.** `generic_echo`

**Backing.** DataStorePlugin receives Arc<dyn StateStore> from the registry. Its read_live helper really exports that store and computes object/namespace counts; create_checkpoint also exports it. This store is not inherently durable.

**State effects.** Whole-store reconciliation intentionally emits NoOp and apply_state changes nothing. The helper’s presence proves a reusable implementation exists, but read_live has no observed canonical method caller. Schema exemplars contain example objects rather than discovered state.

**Test evidence.** No local method execution tests were found. StateStore or MemoryStore tests cannot substitute for exercising this plugin’s named methods.

**Uncertainty.** Which StateStore instance owns runtime objects and how each method should query/export it remain integration questions. No live contents, persistent catalogs or plugin blobs were inspected.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/datastore.rs](../../../crates/op-plugins/src/state_plugins/datastore.rs) — line 123, `DataStorePlugin`: Shared store handle.
- [crates/op-plugins/src/state_plugins/datastore.rs](../../../crates/op-plugins/src/state_plugins/datastore.rs) — line 133, `DataStorePlugin::read_live`: Real helper exports store.
- [crates/op-plugins/src/state_plugins/datastore.rs](../../../crates/op-plugins/src/state_plugins/datastore.rs) — line 202, `DataStorePlugin::calculate_diff`: NoOp whole-store convergence.
- [crates/op-plugins/src/state_plugins/datastore.rs](../../../crates/op-plugins/src/state_plugins/datastore.rs) — line 261, `datastore_schema`: Read method declarations.
- [crates/op-plugins/src/state_plugins/datastore.rs](../../../crates/op-plugins/src/state_plugins/datastore.rs) — line 367, `inventory::submit!`: Registered with ctx.state_store().
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2356, `MutationEngine::dispatch_method_call_with_identity`: Canonical match has no arm for this ID and returns parsed input; outer result still says success.

<a id="dnsresolver"></a>

## `dnsresolver`

**Purpose.** Declares DNS resolution and per-link resolver controls, with a separate resolv.conf reconciler.

**Source status.** Compiled source contains twenty resolved-style method declarations, all returning AckOutput; canonical Call has no DNS dispatcher and echoes input.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/dnsresolver.rs:431 — dnsresolver_schema / DnsState`

**Dispatch.** `generic_echo`

**Backing.** The StatePlugin path reads /etc/resolv.conf directly and atomically replaces it through /etc/resolv.conf.sysdecl.tmp. This is file-backed resolver configuration, not a connection to systemd-resolved.

**State effects.** Diff queries the file instead of using supplied current state. Create/modify writes only in Enforce mode; ObserveOnly does nothing and delete is unsupported. Malformed desired state can default empty; verification can accept malformed desired input. Checkpoint and rollback do not restore resolver state.

**Test evidence.** No local test block found in the inspected plugin module.

**Uncertainty.** Declared lookup/cache/service methods have no demonstrated resolver behavior. Direct file ownership diverges from the D-Bus-only live-state rule and requires reconciliation with the sealed-plugin model.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/mod.rs](../../../crates/op-plugins/src/state_plugins/mod.rs) — line 27, `pub mod dnsresolver`: Compiled module membership; source registration does not prove live blob presence.
- [crates/op-plugins/src/state_plugins/dnsresolver.rs](../../../crates/op-plugins/src/state_plugins/dnsresolver.rs) — line 108, `read_resolv_conf / write_resolv_conf`: Reads resolv.conf; adjacent write helper replaces a temporary file.
- [crates/op-plugins/src/state_plugins/dnsresolver.rs](../../../crates/op-plugins/src/state_plugins/dnsresolver.rs) — line 177, `calculate_diff`: Queries actual file and defaults malformed desired state.
- [crates/op-plugins/src/state_plugins/dnsresolver.rs](../../../crates/op-plugins/src/state_plugins/dnsresolver.rs) — line 223, `apply_state`: Enforce/ObserveOnly split and unsupported delete.
- [crates/op-plugins/src/state_plugins/dnsresolver.rs](../../../crates/op-plugins/src/state_plugins/dnsresolver.rs) — line 268, `verify_state`: Verification, checkpoint and rollback limitations.
- [crates/op-plugins/src/state_plugins/dnsresolver.rs](../../../crates/op-plugins/src/state_plugins/dnsresolver.rs) — line 431, `dnsresolver_schema`: Resolver method declarations and AckOutput contracts.
- [crates/op-plugins/src/state_plugins/dnsresolver.rs](../../../crates/op-plugins/src/state_plugins/dnsresolver.rs) — line 748, `inventory::submit! / PluginReg::new`: Source inventory constructor.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2187, `dispatch_method_call_with_identity backend match`: No explicit arm for this plugin; default at line 2356 echoes arguments in success envelope.

<a id="embedding-model"></a>

## `embedding_model`

**Purpose.** Declare provider, model, endpoint and embedding configuration for interchangeable embedding models.

**Source status.** The schema declares embed, list_models, get_config, set_model and update_config, but canonical method dispatch echoes every call. Separate helpers are operational: voyage_embed_params supplies the cognitive Voyage client; create_checkpoint calls provider-observing live_state.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/embedding_model.rs::embedding_model_schema`

**Dispatch.** `generic_echo`

**Backing.** Configuration comes from projection/environment, while API keys are fetched fresh from environment. Ollama helpers use HTTP; Voyage presence is inferred from a configured key rather than an authenticated health request.

**State effects.** StatePlugin apply_state is a no-op. Thus model-setting methods neither persist nor select a model through this route. The helper reads/probes and provider naming are existing source behavior, not owner approval of a parallel control plane.

**Test evidence.** No local plugin method tests were located. The Voyage/Qdrant end-to-end test covers another path and can skip when credentials/provider are absent; a passing workspace gate does not establish a live provider call.

**Uncertainty.** Provider helper availability does not prove embed dispatch. Model identity, dimensions and credential-presence health need explicit validation before any runtime claim.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/embedding_model.rs](../../../crates/op-plugins/src/state_plugins/embedding_model.rs) — line 166, `voyage_embed_params`: Real helper used outside method surface.
- [crates/op-plugins/src/state_plugins/embedding_model.rs](../../../crates/op-plugins/src/state_plugins/embedding_model.rs) — line 306, `EmbeddingModelPlugin::live_state`: Provider observation.
- [crates/op-plugins/src/state_plugins/embedding_model.rs](../../../crates/op-plugins/src/state_plugins/embedding_model.rs) — line 374, `EmbeddingModelPlugin::apply_state`: No-op.
- [crates/op-plugins/src/state_plugins/embedding_model.rs](../../../crates/op-plugins/src/state_plugins/embedding_model.rs) — line 413, `embedding_model_schema`: Five methods.
- [crates/op-plugins/src/state_plugins/embedding_model.rs](../../../crates/op-plugins/src/state_plugins/embedding_model.rs) — line 523, `inventory::submit!`: Registration.
- [crates/op-cognitive-mcp/src/voyage.rs](../../../crates/op-cognitive-mcp/src/voyage.rs) — line 53, `VoyageClient::new`: Consumes embed parameters.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2356, `MutationEngine::dispatch_method_call_with_identity`: Canonical match has no arm for this ID and returns parsed input; outer result still says success.

<a id="emqx"></a>

## `emqx`

**Purpose.** Projects the existing broker's lifecycle, local management listeners, authenticated hooks and broker-event signal.

**Source status.** Compiled plugin with an explicit typed dispatcher covering all nine declared methods. Declared initial state is deterministic metadata, not a health probe.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/emqx.rs:937 — emqx_schema / EmqxState`

**Dispatch.** `explicit`

**Backing.** Lifecycle and status call the fixed EMQX runit service through system D-Bus. Listener/hook management uses the broker's authenticated local HTTP API; credentials are read internally. Callers cannot select another service.

**State effects.** Start/stop/restart and configure_hooks have real backend paths; hook configuration validates local authenticated transport and re-reads results. StatePlugin diff/apply are intentionally inert, and verify_state checks declared metadata rather than broker health. The legacy mutate path also delegates to this dispatcher.

**Test evidence.** Existing tests cover deterministic schema, service selection, unknown-method rejection, secret exclusion, typed signals and malformed/unauthenticated hook inventories; inspected in this source-only pass.

**Uncertainty.** Source tests do not establish deployed credentials, broker connectivity, successful event delivery or a matching live sealed catalog.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/mod.rs](../../../crates/op-plugins/src/state_plugins/mod.rs) — line 29, `pub mod emqx`: Compiled module membership; source registration does not prove live blob presence.
- [crates/op-plugins/src/state_plugins/emqx.rs](../../../crates/op-plugins/src/state_plugins/emqx.rs) — line 352, `runit_status / runit_lifecycle`: Fixed runit service via D-Bus.
- [crates/op-plugins/src/state_plugins/emqx.rs](../../../crates/op-plugins/src/state_plugins/emqx.rs) — line 409, `api_credentials`: Authenticated local API backing; internal credential-file reads.
- [crates/op-plugins/src/state_plugins/emqx.rs](../../../crates/op-plugins/src/state_plugins/emqx.rs) — line 627, `hook validation`: Rejects unsupported nonlocal or unauthenticated hook transport.
- [crates/op-plugins/src/state_plugins/emqx.rs](../../../crates/op-plugins/src/state_plugins/emqx.rs) — line 743, `dispatch_emqx_method`: All nine methods explicitly dispatched.
- [crates/op-plugins/src/state_plugins/emqx.rs](../../../crates/op-plugins/src/state_plugins/emqx.rs) — line 857, `StatePlugin for EmqxPlugin`: Inert state reconciliation and metadata verification.
- [crates/op-plugins/src/state_plugins/emqx.rs](../../../crates/op-plugins/src/state_plugins/emqx.rs) — line 1048, `tests`: Schema, security and parser tests through end of module.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 1953, `MutationEngine::mutate emqx arm`: Legacy entry delegates separately to the same dispatcher.
- [crates/op-plugins/src/state_plugins/emqx.rs](../../../crates/op-plugins/src/state_plugins/emqx.rs) — line 1039, `inventory::submit! / PluginReg::new`: Source inventory constructor.
- [crates/op-plugins/src/state_plugins/emqx.rs](../../../crates/op-plugins/src/state_plugins/emqx.rs) — line 937, `emqx_schema`: PluginSchema source and declared contract.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2187, `dispatch_method_call_with_identity backend match`: Canonical backend routing for this plugin.

<a id="endpoint"></a>

## `endpoint`

**Purpose.** Declares a service endpoint collection and list, get, add and remove operations.

**Source status.** Compiled schema scaffold without an endpoint dispatcher. All four method inputs are unit structs, so even get/add/remove carry no endpoint identifier or record.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/endpoint.rs:103 — endpoint_schema / EndpointState`

**Dispatch.** `generic_echo`

**Backing.** No endpoint registry client or storage mutation is called from this plugin. Read methods declare typed list/optional-record outputs, while mutations declare AckOutput.

**State effects.** StatePlugin produces an empty diff, reports effect-free apply success, always verifies true and stores a null checkpoint; rollback does nothing. Canonical Call returns input rather than either declared read-output structure.

**Test evidence.** Existing schema_is_schemars_seeded_and_typed and all_subids_are_valid tests check schema structure and authority identifiers; inspected in this source-only pass.

**Uncertainty.** A generated method shape is not working endpoint management. Required request identifiers and actual read/write behavior remain undefined in this source surface; no live sealed-catalog or endpoint-state inspection was performed.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/mod.rs](../../../crates/op-plugins/src/state_plugins/mod.rs) — line 30, `pub mod endpoint`: Compiled module membership; source registration does not prove live blob presence.
- [crates/op-plugins/src/state_plugins/endpoint.rs](../../../crates/op-plugins/src/state_plugins/endpoint.rs) — line 53, `StatePlugin for EndpointPlugin`: Empty diff and no-op apply/verify/checkpoint/rollback.
- [crates/op-plugins/src/state_plugins/endpoint.rs](../../../crates/op-plugins/src/state_plugins/endpoint.rs) — line 103, `endpoint_schema`: Schemars state, unit method inputs and declared outputs.
- [crates/op-plugins/src/state_plugins/endpoint.rs](../../../crates/op-plugins/src/state_plugins/endpoint.rs) — line 192, `schema_is_schemars_seeded_and_typed`: Schema contract test.
- [crates/op-plugins/src/state_plugins/endpoint.rs](../../../crates/op-plugins/src/state_plugins/endpoint.rs) — line 206, `all_subids_are_valid`: Authority identifier validation test.
- [crates/op-plugins/src/state_plugins/endpoint.rs](../../../crates/op-plugins/src/state_plugins/endpoint.rs) — line 236, `inventory::submit! / PluginReg::new`: Source inventory constructor.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2187, `dispatch_method_call_with_identity backend match`: No explicit arm for this plugin; default at line 2356 echoes arguments in success envelope.

<a id="factory"></a>

## `factory`

**Purpose.** Describe Factory Droid computers, sessions, model selection, autonomy and externally backed model discovery.

**Source status.** Six methods advertise computers/sessions/models and BYOM/autonomy changes. The schema is assembled from current_state field values and declares a tched_router dependency. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/factory.rs:379 factory_schema`

**Dispatch.** `generic_echo`

**Backing.** Metadata includes static API/model descriptions and non-secret environment references. BYOM reads a sealed catalog then legacy projection-cache files, including zeroclaw-name fallback.

**State effects.** Plugin diff/apply are no-ops. current_state can read local projection files during schema/checkpoint creation, but no declared method calls Factory or persists autonomy.

**Test evidence.** Two local tests distinguish OpenRouter categories and check non-secret BYOK references, not Factory API behavior.

**Uncertainty.** Model availability and active status are largely static; cache paths/naming conflict with current control-plane policy and need reconciliation. No runtime presence or deployment was checked.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/factory.rs](../../../crates/op-plugins/src/state_plugins/factory.rs) — line 379, `factory_schema`: Plugin-owned method contract and source description.
- [crates/op-plugins/src/state_plugins/factory.rs](../../../crates/op-plugins/src/state_plugins/factory.rs) — line 346, `StatePlugin::apply_state`: Separate state-application behavior described above.
- [crates/op-plugins/src/state_plugins/factory.rs](../../../crates/op-plugins/src/state_plugins/factory.rs) — line 507, `inventory::submit! / PluginReg::new`: Compiled source registration, not live blob presence.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2356, `MutationEngine::dispatch_method_call_with_identity`: No dedicated plugin arm; canonical method dispatch reaches the generic request echo.
- [crates/op-plugins/src/state_plugins/factory.rs](../../../crates/op-plugins/src/state_plugins/factory.rs) — line 57, `projection::read_tched_router_model_routes`: Catalog check plus legacy SHM projection file reads.

<a id="fail2ban"></a>

## `fail2ban`

**Purpose.** Declares jail status, ban/unban and reload operations for Fail2ban.

**Source status.** Compiled scaffold. current_state hardcodes active status, sshd/recidive jail data and counters rather than reading the daemon. The four methods have typed inputs but generic AckOutput responses.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/fail2ban.rs:119 — fail2ban_schema / Fail2banState`

**Dispatch.** `generic_echo`

**Backing.** No Fail2ban control socket, service manager or daemon client is invoked by this plugin's public dispatch path or reconciliation implementation.

**State effects.** calculate_diff is empty; apply_state claims success without applying bans or configuration. Verification always returns true, checkpoint preserves sample metadata and rollback is a no-op. Canonical Call records an event and returns the supplied arguments as its result.

**Test evidence.** No local test block found in the inspected plugin module.

**Uncertainty.** Neither hardcoded active status nor successful transport responses demonstrate enforcement. Actual jail discovery, ban semantics, authorization-to-jail mapping and operational health remain unimplemented in the inspected plugin.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/mod.rs](../../../crates/op-plugins/src/state_plugins/mod.rs) — line 32, `pub mod fail2ban`: Compiled module membership; source registration does not prove live blob presence.
- [crates/op-plugins/src/state_plugins/fail2ban.rs](../../../crates/op-plugins/src/state_plugins/fail2ban.rs) — line 40, `Fail2banPlugin::current_state`: Hardcoded daemon and jail sample data.
- [crates/op-plugins/src/state_plugins/fail2ban.rs](../../../crates/op-plugins/src/state_plugins/fail2ban.rs) — line 75, `StatePlugin for Fail2banPlugin`: Inert reconcile and unconditional verification.
- [crates/op-plugins/src/state_plugins/fail2ban.rs](../../../crates/op-plugins/src/state_plugins/fail2ban.rs) — line 119, `fail2ban_schema`: Ban/unban/status/reload declarations and AckOutput responses.
- [crates/op-plugins/src/state_plugins/fail2ban.rs](../../../crates/op-plugins/src/state_plugins/fail2ban.rs) — line 218, `inventory::submit! / PluginReg::new`: Source inventory constructor.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2187, `dispatch_method_call_with_identity backend match`: No explicit arm for this plugin; default at line 2356 echoes arguments in success envelope.

<a id="freedesktop"></a>

## `freedesktop`

**Purpose.** Represent standard D-Bus ObjectManager, Properties, Introspectable and Peer interface contracts and canonical paths.

**Source status.** Seven builder-defined methods use generic AckOutput rather than their actual standard result shapes. An in-memory interface inventory describes the standard interfaces. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/freedesktop.rs:137 FreeDesktopPlugin::initialize_schema`

**Dispatch.** `generic_echo`

**Backing.** Local maps and canonical path helpers implement structural metadata; standard zbus interfaces elsewhere remain distinct from this plugin's advertised method bodies.

**State effects.** Diff creates no actions; apply reports informational-only success; verification checks desired.name. No GetManagedObjects, Set, Introspect or Peer behavior is forwarded by this canonical plugin path.

**Test evidence.** Five local tests cover creation, canonical paths, registered interface metadata and path normalization/validation, not wire-level standard method execution.

**Uncertainty.** The comment proposing schemas/plugin/freedesktop.json is stale and conflicts with the owner’s single plugin-schema authority; no such loader is invoked here. No runtime presence or deployment was checked.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/freedesktop.rs](../../../crates/op-plugins/src/state_plugins/freedesktop.rs) — line 137, `FreeDesktopPlugin::initialize_schema`: Plugin-owned method contract and source description.
- [crates/op-plugins/src/state_plugins/freedesktop.rs](../../../crates/op-plugins/src/state_plugins/freedesktop.rs) — line 427, `StatePlugin::apply_state`: Separate state-application behavior described above.
- [crates/op-plugins/src/state_plugins/freedesktop.rs](../../../crates/op-plugins/src/state_plugins/freedesktop.rs) — line 560, `inventory::submit! / PluginReg::new`: Compiled source registration, not live blob presence.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2356, `MutationEngine::dispatch_method_call_with_identity`: No dedicated plugin arm; canonical method dispatch reaches the generic request echo.

<a id="full-system"></a>

## `full_system`

**Purpose.** Capture a broad host-state snapshot and expose the narrow implemented hostname mutation.

**Source status.** Both declared methods, capture_state and set_hostname, have explicit dispatcher cases; unknown methods return errors. Broad recovery wording does not establish full-system restore.  capture_full_state initializes the plugins map empty; the former StateManager that was intended to fill it is excluded from the compiled module tree.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/full_system.rs:731 FullSystemPlugin::schema`

**Dispatch.** `explicit`

**Backing.** capture_full_state invokes filesystem/native and service/container observation helpers, defaulting failed sections. set_hostname_native uses the kernel syscall then writes /etc/hostname.

**State effects.** Canonical hostname mutation and separate apply hostname action are real. The syscall precedes file persistence, so a file-write failure can leave partial change. Other diff actions are ignored; rollback logs unimplemented and verify returns true.

**Test evidence.** The local capture_system_info test asserts nonempty kernel version using host observations. No hostname failure-order or full rollback test was located.

**Uncertainty.** Capture defaults can conceal unavailable subsystems. Native file-based observation conflicts with current plugin policy and requires architectural reconciliation. No runtime presence or deployment was checked.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/full_system.rs](../../../crates/op-plugins/src/state_plugins/full_system.rs) — line 731, `FullSystemPlugin::schema`: Plugin-owned contract and source description.
- [crates/op-plugins/src/state_plugins/full_system.rs](../../../crates/op-plugins/src/state_plugins/full_system.rs) — line 841, `StatePlugin::apply_state`: Separate state-application behavior described above.
- [crates/op-plugins/src/state_plugins/full_system.rs](../../../crates/op-plugins/src/state_plugins/full_system.rs) — line 945, `inventory::submit! / PluginReg::new`: Compiled source registration, not live blob presence.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2239, `full_system dispatch arm`: Canonical dispatcher reaches the plugin implementation.
- [crates/op-plugins/src/state_plugins/full_system.rs](../../../crates/op-plugins/src/state_plugins/full_system.rs) — line 906, `dispatch_full_system_method`: Both schema methods dispatched explicitly.
- [crates/op-plugins/src/state_plugins/full_system.rs](../../../crates/op-plugins/src/state_plugins/full_system.rs) — line 184, `set_hostname_native`: Kernel hostname change followed by file write.
- [crates/op-plugins/src/state_plugins/full_system.rs](../../../crates/op-plugins/src/state_plugins/full_system.rs) — line 264, `capture_full_state`: Plugin snapshot map starts empty.
- [crates/op-state/src/lib.rs](../../../crates/op-state/src/lib.rs) — line 11, `StateManager removal comment`: Old manager implementation is not compiled.

<a id="gcloud-adc"></a>

## `gcloud_adc`

**Purpose.** Report the process user’s Google Cloud ADC configuration, account and project.

**Source status.** The single check_auth_status method has an explicit canonical dispatch arm to the plugin helper. It checks ADC-file existence and reads the active gcloud configuration to identify account/project; it does not exchange tokens or authenticate to Google.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/gcloud_adc.rs::gcloud_adc_schema`

**Dispatch.** `explicit`

**Backing.** Files beneath the executing process’s home .config/gcloud directory back this observation. No Cozo persistence or external command is used here.

**State effects.** Read execution performs filesystem lookups only, plus common method audit recording. StatePlugin diff/apply/rollback do nothing and verify_state always succeeds. Direct live-file observation remains owner-policy drift despite an inline comment presenting it as compliant.

**Test evidence.** The file’s all_subids_are_valid test checks taxonomy; no isolated file fixture or real authentication test was located, and no tests were run.

**Uncertainty.** The authenticated field means configuration-file presence, not usable credentials. The process home, mounted credentials and actual Google authorization must be established separately if this surface is used.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/gcloud_adc.rs](../../../crates/op-plugins/src/state_plugins/gcloud_adc.rs) — line 47, `GcloudAdcPlugin::check_auth_status`: Existence/configuration file observation.
- [crates/op-plugins/src/state_plugins/gcloud_adc.rs](../../../crates/op-plugins/src/state_plugins/gcloud_adc.rs) — line 173, `gcloud_adc_schema`: Single method contract.
- [crates/op-plugins/src/state_plugins/gcloud_adc.rs](../../../crates/op-plugins/src/state_plugins/gcloud_adc.rs) — line 213, `dispatch_gcloud_adc_method`: Handler calls helper.
- [crates/op-plugins/src/state_plugins/gcloud_adc.rs](../../../crates/op-plugins/src/state_plugins/gcloud_adc.rs) — line 260, `inventory::submit!`: Registration.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2234, `MutationEngine::dispatch_method_call_with_identity`: Explicit canonical arm.

<a id="gemma-brain"></a>

## `gemma_brain`

**Purpose.** Describe model-agnostic cognitive routing, tag classification, reasoning perspectives and generated UI gallery metadata.

**Source status.** Seven methods advertise classification, routing, perspective/spec/tag reads, registration and intent analysis. Comments asserting MutationEngine implementation are contradicted by the actual dispatch match. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/gemma_brain.rs:266 gemma_brain_schema`

**Dispatch.** `generic_echo`

**Backing.** Source reads generator availability from GEMMA_BINARY_PATH and gallery count from /dev/shm/gemma-ui-specs.json; environment settings name the inference plugin.

**State effects.** No-op diff/apply cannot generate specs, register tags or classify input. Availability is a binary-file existence check, not inference health; checkpoint creation reads gallery metadata.

**Test evidence.** No local test module or plugin-specific execution test was found in the inspected plugin and bridge test sources.

**Uncertainty.** Gemma is a historical label; the owner requires model-agnostic operation. Generator presence and gallery population do not prove runnable plugin methods. No runtime presence or deployment was checked.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/gemma_brain.rs](../../../crates/op-plugins/src/state_plugins/gemma_brain.rs) — line 266, `gemma_brain_schema`: Plugin-owned method contract and source description.
- [crates/op-plugins/src/state_plugins/gemma_brain.rs](../../../crates/op-plugins/src/state_plugins/gemma_brain.rs) — line 228, `StatePlugin::apply_state`: Separate state-application behavior described above.
- [crates/op-plugins/src/state_plugins/gemma_brain.rs](../../../crates/op-plugins/src/state_plugins/gemma_brain.rs) — line 399, `inventory::submit! / PluginReg::new`: Compiled source registration, not live blob presence.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2356, `MutationEngine::dispatch_method_call_with_identity`: No dedicated plugin arm; canonical method dispatch reaches the generic request echo.
- [crates/op-plugins/src/state_plugins/gemma_brain.rs](../../../crates/op-plugins/src/state_plugins/gemma_brain.rs) — line 135, `GemmaBrainPlugin::gallery_spec_count`: Direct local gallery read; absent/unreadable becomes zero.

<a id="ghostbridge"></a>

## `ghostbridge`

**Purpose.** Describe bridge identity, configured endpoints and the Ghostrunner UI surface.

**Source status.** All four methods have explicit dispatch: GetState, GetIdentity, ListEndpoints and GetGhostrunner. The bridge first resolves projected state, falling back to GhostbridgePlugin::current_state; the plugin dispatcher returns the corresponding typed domain object.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/ghostbridge.rs::ghostbridge_schema`

**Dispatch.** `explicit`

**Backing.** Projected state or environment-derived declaration backs the result. current_state reports declared status, default TLS/UI properties and an empty endpoint list; it does not inspect live sockets or certificates.

**State effects.** Methods read the selected state and create common audit events. StatePlugin apply and rollback are no-ops; checkpoints snapshot the declared current_state. The identity view here is bridge metadata, distinct from human_principal and SID1 session authority.

**Test evidence.** every_declared_method_has_a_domain_dispatcher and undeclared_method_is_rejected test the dispatch table; other tests cover defaults/schema taxonomy.

**Uncertainty.** A successful method response does not establish endpoint reachability, certificate validity or deployment. Which publisher supplies a populated runtime projection remains a separate lifecycle question.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/ghostbridge.rs](../../../crates/op-plugins/src/state_plugins/ghostbridge.rs) — line 195, `GhostbridgePlugin::current_state`: Declared fallback.
- [crates/op-plugins/src/state_plugins/ghostbridge.rs](../../../crates/op-plugins/src/state_plugins/ghostbridge.rs) — line 305, `ghostbridge_schema`: Contract.
- [crates/op-plugins/src/state_plugins/ghostbridge.rs](../../../crates/op-plugins/src/state_plugins/ghostbridge.rs) — line 405, `dispatch_ghostbridge_method`: All method handlers.
- [crates/op-plugins/src/state_plugins/ghostbridge.rs](../../../crates/op-plugins/src/state_plugins/ghostbridge.rs) — line 486, `tests::every_declared_method_has_a_domain_dispatcher`: Coverage of method names.
- [crates/op-plugins/src/state_plugins/ghostbridge.rs](../../../crates/op-plugins/src/state_plugins/ghostbridge.rs) — line 426, `inventory::submit!`: Registration.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2312, `MutationEngine::dispatch_method_call_with_identity`: Projection plus explicit arm.

<a id="hardware"></a>

## `hardware`

**Purpose.** Describe host CPU, memory and disk inventory plus device scan, refresh and statistics methods.

**Source status.** Five schema methods use unit inputs; scan/refresh return AckOutput. Private inventory helpers exist but have no located callers in the current Rust source. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/hardware.rs:292 hardware_schema`

**Dispatch.** `generic_echo`

**Backing.** Helpers parse /proc/cpuinfo, /proc/meminfo, /sys/block and mounts directly; failures generally become empty or Unknown data.

**State effects.** State diff/apply are no-ops and checkpoint contains null. The canonical methods neither call the inventory helpers nor persist a refreshed inventory.

**Test evidence.** The local all_subids_are_valid test checks schema annotations. No device scan or canonical method execution test was found in inspected test paths.

**Uncertainty.** Implementation of parsers is separate from connection to the plugin surface; a successful echo must not be reported as hardware discovery. No runtime presence or deployment was checked.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/hardware.rs](../../../crates/op-plugins/src/state_plugins/hardware.rs) — line 292, `hardware_schema`: Plugin-owned method contract and source description.
- [crates/op-plugins/src/state_plugins/hardware.rs](../../../crates/op-plugins/src/state_plugins/hardware.rs) — line 254, `StatePlugin::apply_state`: Separate state-application behavior described above.
- [crates/op-plugins/src/state_plugins/hardware.rs](../../../crates/op-plugins/src/state_plugins/hardware.rs) — line 427, `inventory::submit! / PluginReg::new`: Compiled source registration, not live blob presence.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2356, `MutationEngine::dispatch_method_call_with_identity`: No dedicated plugin arm; canonical method dispatch reaches the generic request echo.
- [crates/op-plugins/src/state_plugins/hardware.rs](../../../crates/op-plugins/src/state_plugins/hardware.rs) — line 121, `HardwarePlugin::get_cpu_info`: Private filesystem-based inventory helper exists separately.

<a id="host-runtime"></a>

## `host_runtime`

**Purpose.** Provide host observations for runit services, system information, interfaces, NUMA, local sockets and metrics.

**Source status.** The source contains seven typed methods and dispatch_host_runtime_method, but state_plugins/mod.rs has no host_runtime module and the bridge has no matching arm. 

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/host_runtime.rs:240 host_runtime_schema`

**Dispatch.** `none`

**Backing.** Dormant helpers read procfs/sysfs and /etc/hostname, invoke sv status, and connect to requested Unix sockets. Those direct observation/subprocess paths conflict with current plugin policy.

**State effects.** If wired, the dispatcher would perform observations and socket connection probes; its separate StatePlugin diff/apply are no-ops. No current compiled canonical route is established.

**Test evidence.** A schema_exposes_dispatched_methods test exists in the excluded file and therefore is not included by normal workspace compilation.

**Uncertainty.** Source registration text alone does not register the plugin. Activation requires an architectural decision and policy-compliant implementation, not merely adding a module. No runtime presence or deployment was checked.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/host_runtime.rs](../../../crates/op-plugins/src/state_plugins/host_runtime.rs) — line 240, `host_runtime_schema`: Plugin-owned method contract and source description.
- [crates/op-plugins/src/state_plugins/host_runtime.rs](../../../crates/op-plugins/src/state_plugins/host_runtime.rs) — line 203, `StatePlugin::apply_state`: Separate state-application behavior described above.
- [crates/op-plugins/src/state_plugins/host_runtime.rs](../../../crates/op-plugins/src/state_plugins/host_runtime.rs) — line 806, `inventory::submit! / PluginReg::new`: Registration exists in an uncompiled module.
- [crates/op-plugins/src/state_plugins/mod.rs](../../../crates/op-plugins/src/state_plugins/mod.rs) — line 38, `state_plugins module declarations`: hardware module present; host_runtime absent from the module inventory.
- [crates/op-plugins/src/state_plugins/host_runtime.rs](../../../crates/op-plugins/src/state_plugins/host_runtime.rs) — line 760, `dispatch_host_runtime_method`: Dormant real observation dispatch exists.

<a id="human-principal"></a>

## `human_principal`

**Purpose.** Register WireGuard-derived human principals, resolve keys, manage display aliases and revoke keys.

**Source status.** All six schema methods have explicit handlers. register_key validates the key, derives principal_id server-side, rejects existing keys including revoked tombstones and rejects duplicate active aliases. revoke_key preserves the original revocation timestamp on repeats.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/human_principal.rs::human_principal_schema`

**Dispatch.** `explicit`

**Backing.** A distinct persistent CozoGraphShuttle database holds human-principal records; store handles are cached by path. Strict authorization lookups fail when storage is unavailable.

**State effects.** Register, revoke and alias operations persist records through Cozo; ordinary read helpers may degrade differently from strict assertion lookups. StatePlugin apply is deliberately inert because mutations belong to schema methods. Registration changes principal records and does not itself grant capabilities.

**Owner topology.** Owner-provided topology: WireGuard terminates at the decoy; there is no WireGuard on this server. Identity is transported by WARP. The source-level WireGuard-derived principal model does not establish host tunnel termination.

**Test evidence.** Source tests cover forged principal IDs, duplicate keys/aliases, revoked-key visibility, idempotent revocation and audit recording. No tests or registration calls were executed.

**Uncertainty.** Live registry content, grant issuance and deployed enforcement are unverified. Source read-then-write uniqueness logic merits concurrency scrutiny before claiming atomic uniqueness under simultaneous registrations.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/human_principal.rs](../../../crates/op-plugins/src/state_plugins/human_principal.rs) — line 156, `human_principal_schema`: Six-method contract.
- [crates/op-plugins/src/state_plugins/human_principal.rs](../../../crates/op-plugins/src/state_plugins/human_principal.rs) — line 113, `HumanPrincipalPlugin::apply_state`: Separate apply intentionally inert.
- [crates/op-plugins/src/state_plugins/human_principal.rs](../../../crates/op-plugins/src/state_plugins/human_principal.rs) — line 465, `inventory::submit!`: Registration.
- [crates/op-grpc-bridge/src/human_principal_dispatch.rs](../../../crates/op-grpc-bridge/src/human_principal_dispatch.rs) — line 86, `dispatch_human_principal_method`: Complete named dispatch.
- [crates/op-grpc-bridge/src/human_principal_dispatch.rs](../../../crates/op-grpc-bridge/src/human_principal_dispatch.rs) — line 233, `register_key`: Derived ID and durable record.
- [crates/op-grpc-bridge/src/human_principal_dispatch.rs](../../../crates/op-grpc-bridge/src/human_principal_dispatch.rs) — line 549, `tests::register_key_ignores_forged_principal_id`: Behavioral test.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2214, `MutationEngine::dispatch_method_call_with_identity`: Canonical arm.

<a id="identity-sled"></a>

## `identity_sled`

**Purpose.** Manage anchored session sleds, identity container provisioning and per-session event history.

**Source status.** Seven methods have explicit canonical dispatch. The schema lives in identity_sled.rs; a generated content-hash include fingerprints its record shape rather than defining a second schema. The engine authors SID1 and rejects caller-supplied credential fields.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/identity_sled.rs::identity_sled_schema`

**Dispatch.** `explicit`

**Backing.** Private persistent Cozo records warm the engine cache; session arrival links the event chain, container binding and sealed identity. Incus helpers provision containers and attach dedicated btrfs device records.

**State effects.** write_identity requires existing provisioning and cannot change the sealed term. Credential creation requires durable persistence before cache publication. Touch/history/event methods maintain session state; generic StatePlugin reconciliation is intentionally inert. Public results redact possession credentials while a protected projection carries them.

**Test evidence.** Tests inspect genesis mint/reuse, PSK rejection, containerless rejection, credential mismatch and persistence behavior; none were run.

**Uncertainty.** Preserve the container-backed SID1 identity invariant while distinguishing the control-plane chatbot role from an obsolete named assistant container/service. Actual identity topology, mounts and runtime registration remain unverified.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/identity_sled.rs](../../../crates/op-plugins/src/state_plugins/identity_sled.rs) — line 303, `identity_sled_schema`: Seven methods.
- [crates/op-plugins/src/state_plugins/identity_sled.rs](../../../crates/op-plugins/src/state_plugins/identity_sled.rs) — line 36, `SCHEMA_CONTENT_HASH`: Build-derived hash include.
- [crates/op-plugins/src/state_plugins/identity_sled.rs](../../../crates/op-plugins/src/state_plugins/identity_sled.rs) — line 683, `inventory::submit!`: Registration.
- [crates/op-grpc-bridge/src/identity_sled_dispatch.rs](../../../crates/op-grpc-bridge/src/identity_sled_dispatch.rs) — line 1134, `dispatch_identity_sled_method`: Named operations.
- [crates/op-grpc-bridge/src/identity_sled_dispatch.rs](../../../crates/op-grpc-bridge/src/identity_sled_dispatch.rs) — line 474, `persist_sled_required`: Credential durability requirement.
- [crates/op-grpc-bridge/src/identity_sled_dispatch.rs](../../../crates/op-grpc-bridge/src/identity_sled_dispatch.rs) — line 2069, `tests::write_cannot_create_containerless_identity_or_accept_psk`: Behavior tests.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2209, `MutationEngine::dispatch_method_call_with_identity`: Canonical arm.

<a id="incus"></a>

## `incus`

**Purpose.** Projects container instances, profiles, devices and lifecycle controls.

**Source status.** Compiled plugin with substantive Incus REST helpers, but canonical Call dispatches only delete_instance and remove_device; other declared methods fall through to generic echo.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/incus.rs:1461 — incus_schema / IncusState`

**Dispatch.** `partial`

**Backing.** Despite the run_incus_command helper name, operations use bounded reqwest HTTP over the Incus Unix socket, with redirects disabled and asynchronous operation handling. Helpers implement creation, deletion, profile/config/device synchronization and power changes.

**State effects.** Diff compares real supplied instance structures. Apply handles Create/Delete, but initializes current_state to None, making its Modify branch fail with current-instance-not-found even though apply_modify exists. Verification is unconditional; checkpoint is null and cannot supply a useful rollback state.

**Test evidence.** Existing comparison/diff tests and Unix HTTP tests cover chunked responses, truncation, size limits, redirects, HTTP failure and timeout; inspected in this source-only pass.

**Uncertainty.** The executable-path availability check is not daemon health. Helper existence must not be mistaken for canonical method wiring or working reconciliation updates.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/mod.rs](../../../crates/op-plugins/src/state_plugins/mod.rs) — line 41, `pub mod incus`: Compiled module membership; source registration does not prove live blob presence.
- [crates/op-plugins/src/state_plugins/incus.rs](../../../crates/op-plugins/src/state_plugins/incus.rs) — line 185, `incus_request_on_socket`: Bounded Unix-socket HTTP transport.
- [crates/op-plugins/src/state_plugins/incus.rs](../../../crates/op-plugins/src/state_plugins/incus.rs) — line 675, `apply_create / apply_modify / apply_delete`: Concrete lifecycle helpers.
- [crates/op-plugins/src/state_plugins/incus.rs](../../../crates/op-plugins/src/state_plugins/incus.rs) — line 979, `calculate_diff`: Structured instance comparison.
- [crates/op-plugins/src/state_plugins/incus.rs](../../../crates/op-plugins/src/state_plugins/incus.rs) — line 1048, `apply_state`: Current state is None; Modify cannot resolve an existing instance.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2252, `incus dispatch guards`: Only delete_instance and remove_device wired; other methods hit fallback.
- [crates/op-plugins/src/state_plugins/incus.rs](../../../crates/op-plugins/src/state_plugins/incus.rs) — line 1798, `Unix HTTP transport tests`: Bounded transport failure and success coverage.
- [crates/op-plugins/src/state_plugins/incus.rs](../../../crates/op-plugins/src/state_plugins/incus.rs) — line 971, `is_available`: Checks executable path, not daemon health.
- [crates/op-plugins/src/state_plugins/incus.rs](../../../crates/op-plugins/src/state_plugins/incus.rs) — line 1762, `inventory::submit! / PluginReg::new`: Source inventory constructor.
- [crates/op-plugins/src/state_plugins/incus.rs](../../../crates/op-plugins/src/state_plugins/incus.rs) — line 1461, `incus_schema`: PluginSchema source and declared contract.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2187, `dispatch_method_call_with_identity backend match`: Canonical backend routing for this plugin.

<a id="json-render"></a>

## `json_render`

**Purpose.** Publish the json-render framework’s component/action/package inventory and UI spec contract for generative consumers.

**Source status.** Fifteen methods have typed inputs/outputs, including validate_spec and build_prompt_surface, but their implementations are not connected to canonical method dispatch. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/json_render.rs:585 json_render_schema`

**Dispatch.** `generic_echo`

**Backing.** JsonRenderState::default builds an in-source catalog of packages, components, renderers and directives with a pinned upstream source commit. No external UI process is called.

**State effects.** No-op diff/apply and default-state checkpoints do not validate specs, change renderer configuration or execute actions. The schema itself supplies descriptive inventory to consumers.

**Test evidence.** Local tests check OSCAL annotations and presence of all fifteen declared methods; they do not execute validation or prompt construction.

**Uncertainty.** The reference inventory can exceed the external UI’s implemented registry. Defaults marked active and typed responses are not proof of rendered behavior. No runtime presence or deployment was checked.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/json_render.rs](../../../crates/op-plugins/src/state_plugins/json_render.rs) — line 585, `json_render_schema`: Plugin-owned method contract and source description.
- [crates/op-plugins/src/state_plugins/json_render.rs](../../../crates/op-plugins/src/state_plugins/json_render.rs) — line 543, `StatePlugin::apply_state`: Separate state-application behavior described above.
- [crates/op-plugins/src/state_plugins/json_render.rs](../../../crates/op-plugins/src/state_plugins/json_render.rs) — line 1766, `inventory::submit! / PluginReg::new`: Compiled source registration, not live blob presence.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2356, `MutationEngine::dispatch_method_call_with_identity`: No dedicated plugin arm; canonical method dispatch reaches the generic request echo.
- [crates/op-plugins/src/state_plugins/json_render.rs](../../../crates/op-plugins/src/state_plugins/json_render.rs) — line 476, `JsonRenderState::default`: Pinned inventory is constructed in plugin source.

<a id="keypair"></a>

## `keypair`

**Purpose.** Declare managed SSH keypair metadata and provide a list_keypairs contract.

**Source status.** The plugin describes itself as a reference schema pattern, but its single list_keypairs method has no canonical dispatch arm. PluginV1.Call returns input through the common fallback, so the typed output declaration does not establish discovery.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/keypair.rs::keypair_schema`

**Dispatch.** `generic_echo`

**Backing.** KeypairState and KeypairInfo describe names, algorithms, public keys and presence. The implementation owns no secret store, key generation backend or filesystem discovery routine.

**State effects.** StatePlugin diff/apply/rollback perform no work; verification always succeeds and checkpoints contain null. Capability flags advertise rollback/checkpoint/verification support without corresponding substantive behavior. The shared method audit records invocation, not a real key listing.

**Test evidence.** schema_is_schemars_seeded_and_typed and all_subids_are_valid validate declarations only. They do not exercise discovery or key lifecycle.

**Uncertainty.** Do not infer SSH key provisioning from this plugin or conflate it with WireGuard registration in op-identity. The intended backing implementation and actual sealed-catalog presence remain unverified.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/keypair.rs](../../../crates/op-plugins/src/state_plugins/keypair.rs) — line 33, `KeypairState`: Declaration-only state.
- [crates/op-plugins/src/state_plugins/keypair.rs](../../../crates/op-plugins/src/state_plugins/keypair.rs) — line 177, `KeypairPlugin::apply_state`: No-op state behavior.
- [crates/op-plugins/src/state_plugins/keypair.rs](../../../crates/op-plugins/src/state_plugins/keypair.rs) — line 219, `keypair_schema`: Single list method.
- [crates/op-plugins/src/state_plugins/keypair.rs](../../../crates/op-plugins/src/state_plugins/keypair.rs) — line 256, `tests::schema_is_schemars_seeded_and_typed`: Schema-only test.
- [crates/op-plugins/src/state_plugins/keypair.rs](../../../crates/op-plugins/src/state_plugins/keypair.rs) — line 298, `inventory::submit!`: Registration.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2356, `MutationEngine::dispatch_method_call_with_identity`: No specific arm; canonical result echoes input.

<a id="keyring"></a>

## `keyring`

**Purpose.** Expose Secret Service collection metadata through the keyring plugin.

**Source status.** Both schema methods have an explicit canonical handler, but get_collections unconditionally returns an empty vector and get_default_collection returns None. This is routed functionality with a placeholder backing implementation, not generic argument echo or evidence of an empty real keyring.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/keyring.rs::keyring_schema`

**Dispatch.** `partial`

**Backing.** A connect_service helper targets the freedesktop plugin over the session bus; current list/default helpers do not use it. Availability merely checks that the session-bus socket path exists.

**State effects.** Methods return fixed metadata and append common audit records. StatePlugin apply/rollback explicitly reject changes; verification checks only JSON object shape. No secrets are created, read or persisted by these declared methods.

**Test evidence.** Local tests verify construction and capability flags, not collection enumeration or provider interaction. They were inspected only.

**Uncertainty.** The actual Secret Service provider binding is missing from the inspected method implementation. Source comments claiming a provider is absent cannot establish current runtime state, and socket existence is insufficient provider-health evidence.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/keyring.rs](../../../crates/op-plugins/src/state_plugins/keyring.rs) — line 57, `KeyringPlugin::connect_service`: Unused provider connection helper.
- [crates/op-plugins/src/state_plugins/keyring.rs](../../../crates/op-plugins/src/state_plugins/keyring.rs) — line 70, `KeyringPlugin::get_collections`: Hardcoded empty result.
- [crates/op-plugins/src/state_plugins/keyring.rs](../../../crates/op-plugins/src/state_plugins/keyring.rs) — line 84, `KeyringPlugin::get_default_collection`: Hardcoded None.
- [crates/op-plugins/src/state_plugins/keyring.rs](../../../crates/op-plugins/src/state_plugins/keyring.rs) — line 197, `keyring_schema`: Two methods.
- [crates/op-plugins/src/state_plugins/keyring.rs](../../../crates/op-plugins/src/state_plugins/keyring.rs) — line 259, `dispatch_keyring_method`: Explicit dispatch.
- [crates/op-plugins/src/state_plugins/keyring.rs](../../../crates/op-plugins/src/state_plugins/keyring.rs) — line 282, `tests::test_keyring_plugin_creation`: Construction tests.
- [crates/op-plugins/src/state_plugins/keyring.rs](../../../crates/op-plugins/src/state_plugins/keyring.rs) — line 302, `inventory::submit!`: Registration.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2244, `MutationEngine::dispatch_method_call_with_identity`: Canonical arm.

<a id="large-language-model"></a>

## `large_language_model`

**Purpose.** Describe a model-agnostic language-model provider, selected model and generation parameters.

**Source status.** The schema declares provider/model/configuration and generation methods, but the canonical dispatch match has no large_language_model arm. Several mutation/generation requests are empty unit inputs with AckOutput, so neither generation payloads nor streaming execution are established.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/large_language_model.rs::large_language_model_schema`

**Dispatch.** `generic_echo`

**Backing.** Separate environment-driven helpers query Ollama-compatible /api/tags and /api/ps endpoints and choose an override, loaded model or first listed model. create_checkpoint invokes that observed-state helper.

**State effects.** StatePlugin apply and rollback are no-ops; only helper/checkpoint reads can contact a provider. Declared generation calls merely echo arguments after common audit recording. Historical Gemma naming elsewhere does not limit the owner’s model-agnostic role.

**Test evidence.** No local functional method tests were located. Provider helper code is not an end-to-end generation test, and nothing was executed.

**Uncertainty.** The active model/provider, publication of observed state and intended generation implementation remain unverified. A method named stream_generate does not create a streaming RPC or an implementation.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/large_language_model.rs](../../../crates/op-plugins/src/state_plugins/large_language_model.rs) — line 131, `LargeLanguageModelPlugin::fetch_provider_models`: Provider helper.
- [crates/op-plugins/src/state_plugins/large_language_model.rs](../../../crates/op-plugins/src/state_plugins/large_language_model.rs) — line 180, `LargeLanguageModelPlugin::live_state`: Model resolution.
- [crates/op-plugins/src/state_plugins/large_language_model.rs](../../../crates/op-plugins/src/state_plugins/large_language_model.rs) — line 234, `LargeLanguageModelPlugin::apply_state`: No-op apply.
- [crates/op-plugins/src/state_plugins/large_language_model.rs](../../../crates/op-plugins/src/state_plugins/large_language_model.rs) — line 273, `large_language_model_schema`: Method declarations.
- [crates/op-plugins/src/state_plugins/large_language_model.rs](../../../crates/op-plugins/src/state_plugins/large_language_model.rs) — line 410, `inventory::submit!`: Registration.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2356, `MutationEngine::dispatch_method_call_with_identity`: No specific arm; canonical result echoes input.

<a id="login1"></a>

## `login1`

**Purpose.** Describe login sessions, users and seats, including advertised power and session lifecycle operations.

**Source status.** Twenty-three methods advertise session/user/seat reads and mutations including power-off, reboot and sleep. Generic AckOutput contracts overstate meaningful return data. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/login1.rs:224 login1_schema`

**Dispatch.** `generic_echo`

**Backing.** A private connect_manager helper constructs an org.freedesktop.login1.Manager system-bus proxy, but no caller or actual manager method invocation was located here.

**State effects.** Diff can emit a Modify action; apply ignores it and reports read-only. Verification returns true and checkpoints are empty, so power/session methods have no demonstrated effect.

**Test evidence.** No local test module or targeted canonical execution test was found in the inspected plugin and bridge tests.

**Uncertainty.** Host login1 service availability is unverified. The read-only implementation conflicts with the much broader schema; standard native login1 services remain separate. No runtime presence or deployment was checked.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/login1.rs](../../../crates/op-plugins/src/state_plugins/login1.rs) — line 224, `login1_schema`: Plugin-owned method contract and source description.
- [crates/op-plugins/src/state_plugins/login1.rs](../../../crates/op-plugins/src/state_plugins/login1.rs) — line 97, `StatePlugin::apply_state`: Separate state-application behavior described above.
- [crates/op-plugins/src/state_plugins/login1.rs](../../../crates/op-plugins/src/state_plugins/login1.rs) — line 575, `inventory::submit! / PluginReg::new`: Compiled source registration, not live blob presence.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2356, `MutationEngine::dispatch_method_call_with_identity`: No dedicated plugin arm; canonical method dispatch reaches the generic request echo.

<a id="mail-server"></a>

## `mail_server`

**Purpose.** Describe mail domain/mailbox/alias/queue operations and the mail stack’s container and socket metadata.

**Source status.** Nine methods advertise domain, mailbox, alias, quota and queue mutations/reads. AckOutput is used even for queue data; canonical dispatch has no mail backend. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/mail_server.rs:315 mail_server_schema`

**Dispatch.** `generic_echo`

**Backing.** Private helpers implement Incus HTTP over a Unix socket and treat Running container status as mail health. No current caller of those helpers was located.

**State effects.** No-op diff/apply cannot provision domains, mailboxes or aliases, flush a queue or change quotas. Checkpoints are null and verify returns true. Native helper code does not prove these operations execute.

**Test evidence.** No local test module or targeted mail plugin execution test was found in inspected plugin/bridge test sources.

**Uncertainty.** Container liveness is weaker than SMTP/IMAP health. Metadata naming a D-Bus service does not establish an owned bus object or running mail stack. No runtime presence or deployment was checked.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/mail_server.rs](../../../crates/op-plugins/src/state_plugins/mail_server.rs) — line 315, `mail_server_schema`: Plugin-owned contract and source description.
- [crates/op-plugins/src/state_plugins/mail_server.rs](../../../crates/op-plugins/src/state_plugins/mail_server.rs) — line 278, `StatePlugin::apply_state`: Separate state-application behavior described above.
- [crates/op-plugins/src/state_plugins/mail_server.rs](../../../crates/op-plugins/src/state_plugins/mail_server.rs) — line 682, `inventory::submit! / PluginReg::new`: Compiled source registration, not live blob presence.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2356, `MutationEngine::dispatch_method_call_with_identity`: No dedicated plugin arm; canonical method dispatch reaches the generic request echo.

<a id="mcp"></a>

## `mcp`

**Purpose.** Configure MCP server descriptors and tool-group settings while retaining the bridge as the only public MCP door.

**Source status.** Three typed methods declare get_config, configure_server and configure_tool_groups; comments correctly note missing method dispatcher instance dependencies. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/mcp.rs:365 mcp_schema`

**Dispatch.** `generic_echo`

**Backing.** McpStatePlugin owns a StateStore and config path, defaulting to /etc/op-dbus/mcp-config.json. Helpers read/write configuration and save/update execution jobs.

**State effects.** Separate apply handles server and tool-group changes with real non-atomic file writes. compact_mode changes report success without saving. Checkpoints contain null although rollback tries to deserialize them into McpConfig. These helper effects assume a valid supplied diff. The compiled PluginWorkflowNode currently supplies null current state, which this plugin's typed calculate_diff cannot decode.

**Test evidence.** Local tests verify the plugin-owned schema, agent_config dependency and subids. They do not cover file persistence, compact-mode updates or rollback.

**Uncertainty.** File updates do not establish a running provider reload. Legacy compact-mode configuration must not be mistaken for another listener or plugin. No runtime presence or deployment was checked.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/mcp.rs](../../../crates/op-plugins/src/state_plugins/mcp.rs) — line 365, `mcp_schema`: Plugin-owned contract and source description.
- [crates/op-plugins/src/state_plugins/mcp.rs](../../../crates/op-plugins/src/state_plugins/mcp.rs) — line 509, `StatePlugin::apply_state`: Separate state-application behavior described above.
- [crates/op-plugins/src/state_plugins/mcp.rs](../../../crates/op-plugins/src/state_plugins/mcp.rs) — line 653, `inventory::submit! / PluginReg::new`: Compiled source registration, not live blob presence.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2356, `MutationEngine::dispatch_method_call_with_identity`: No dedicated plugin arm; canonical method dispatch reaches the generic request echo.
- [crates/op-plugins/src/state_plugins/mcp.rs](../../../crates/op-plugins/src/state_plugins/mcp.rs) — line 206, `McpStatePlugin::save_config`: Non-atomic backing configuration write.
- [crates/op-plugins/src/state_plugins/mcp.rs](../../../crates/op-plugins/src/state_plugins/mcp.rs) — line 553, `create_checkpoint / rollback`: Null checkpoint contradicts rollback's McpConfig decode requirement.
- [crates/op-state/src/plugin_workflow.rs](../../../crates/op-state/src/plugin_workflow.rs) — line 153, `PluginWorkflowNode::execute`: Compiled alternate workflow supplies null current state before calculate_diff.

<a id="memory"></a>

## `memory`

**Purpose.** Declare namespace and entry management for a cognitive-memory surface.

**Source status.** The registered memory plugin declares namespace creation/deletion, entry CRUD, statistics and compaction, but has no canonical dispatch arm. The common fallback returns the input for admitted calls rather than interacting with a memory database.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/memory_plugin.rs::memory_schema`

**Dispatch.** `generic_echo`

**Backing.** MemoryPlugin owns no store. current_state is a hardcoded active/sqlite exemplar with three named namespaces and zero counts. This conflicts with the Cozo policy and must not be described as a live SQLite backend.

**State effects.** StatePlugin apply/rollback are no-ops, verification always succeeds and checkpoints capture the hardcoded exemplar. The separate cognitive_mcp runtime has real Cozo memory operations; those are not wired to this plugin’s method names.

**Test evidence.** No local memory-plugin behavior tests were located. CognitiveMemoryStore tests exercise the separate runtime and cannot validate the memory plugin.

**Uncertainty.** The relationship or consolidation between memory and cognitive_mcp is unresolved. Method declarations, illustrative statistics and common audit events do not demonstrate durable storage or tenant isolation.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/memory_plugin.rs](../../../crates/op-plugins/src/state_plugins/memory_plugin.rs) — line 41, `MemoryPlugin::current_state`: Hardcoded SQLite-labelled exemplar.
- [crates/op-plugins/src/state_plugins/memory_plugin.rs](../../../crates/op-plugins/src/state_plugins/memory_plugin.rs) — line 78, `MemoryPlugin::apply_state`: No-op.
- [crates/op-plugins/src/state_plugins/memory_plugin.rs](../../../crates/op-plugins/src/state_plugins/memory_plugin.rs) — line 269, `memory_schema`: Namespace/entry method declarations.
- [crates/op-plugins/src/state_plugins/memory_plugin.rs](../../../crates/op-plugins/src/state_plugins/memory_plugin.rs) — line 458, `inventory::submit!`: Registration.
- [crates/op-cognitive-mcp/src/memory_store.rs](../../../crates/op-cognitive-mcp/src/memory_store.rs) — line 115, `CognitiveMemoryStore::new`: Separate Cozo memory implementation.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2356, `MutationEngine::dispatch_method_call_with_identity`: No specific arm; canonical result echoes input.

<a id="mongodb-mcp"></a>

## `mongodb_mcp`

**Purpose.** Project read-oriented MongoDB data, metadata and knowledge operations through an external MCP provider.

**Source status.** The canonical mongodb_mcp arm converts schema underscores to upstream hyphens and invokes ExternalMcpClient::call_tool. The plugin’s read_method macro declares typed inputs with a flattened ProviderResult output; this is explicit provider forwarding, not an embedded MongoDB store.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/mongodb_mcp.rs::mongodb_mcp_schema`

**Dispatch.** `explicit`

**Backing.** Bridge-owned ExternalMcpUpstream targets a configured local MCP endpoint; current_state reports readiness from marker files and describes the provider as read_only. Database contents and credentials remain upstream.

**State effects.** Method effects depend on the provider operation; common auditing occurs before forwarding. StatePlugin apply is a no-op and verification only checks a readiness marker. No MongoDB connection or provider readiness was tested.

**Test evidence.** No local functional plugin tests were located. ProviderResult’s unrestricted flattened map does not demonstrate native protobuf result fidelity; that needs contract validation rather than another schema source.

**Uncertainty.** Read-only labels and marker files alone do not prove provider enforcement or availability. Native response compatibility and actual upstream configuration remain unverified.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/mongodb_mcp.rs](../../../crates/op-plugins/src/state_plugins/mongodb_mcp.rs) — line 135, `ProviderResult`: Flattened provider map.
- [crates/op-plugins/src/state_plugins/mongodb_mcp.rs](../../../crates/op-plugins/src/state_plugins/mongodb_mcp.rs) — line 150, `MongoDbMcpPlugin::current_state`: Marker-derived state.
- [crates/op-plugins/src/state_plugins/mongodb_mcp.rs](../../../crates/op-plugins/src/state_plugins/mongodb_mcp.rs) — line 230, `mongodb_mcp_schema`: Read-method declarations.
- [crates/op-plugins/src/state_plugins/mongodb_mcp.rs](../../../crates/op-plugins/src/state_plugins/mongodb_mcp.rs) — line 334, `inventory::submit!`: Registration.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 99, `ExternalMcpUpstream::call_tool`: Upstream transport helper.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2338, `MutationEngine::dispatch_method_call_with_identity`: Explicit underscore-to-hyphen forwarding.

<a id="net"></a>

## `net`

**Purpose.** Models interfaces and OVS bridge configuration with separate host-network reconciliation.

**Source status.** Compiled plugin; canonical apply_interface and delete_ovs_bridge method calls echo inputs. The older mutate entry separately handles some OVS operations.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/net.rs:976 — net_schema / NetworkConfig`

**Dispatch.** `generic_echo`

**Backing.** StatePlugin uses OvsdbDbusClient, which now wraps native rovs_ovsdb over a Unix socket despite its name. Host addressing uses native rtnetlink and directly rewrites /etc/network/interfaces.

**State effects.** Diff emits interface create/modify/delete. Apply can create bridges/ports and configure link, IP and routes; deletion is restricted to bridge-like names. Some kernel failures become warnings. Verification always returns true; checkpoint is null and rollback cannot reconstruct prior state from it.

**Test evidence.** Local all_subids_are_valid checks schema authority identifiers; no end-to-end network test executed.

**Uncertainty.** Canonical method availability does not expose the reconciler's effects. Direct file/kernel access and stale Netmaker interface exclusions require policy review; observed source transport must not be described as a live D-Bus backend.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/mod.rs](../../../crates/op-plugins/src/state_plugins/mod.rs) — line 52, `pub mod net`: Compiled module membership; source registration does not prove live blob presence.
- [crates/op-plugins/src/state_plugins/net.rs](../../../crates/op-plugins/src/state_plugins/net.rs) — line 436, `apply_ovs_config`: OVS operations and interface exclusions.
- [crates/op-plugins/src/state_plugins/net.rs](../../../crates/op-plugins/src/state_plugins/net.rs) — line 481, `apply_ovs_config host configuration`: File and rtnetlink changes with warning-only failures.
- [crates/op-plugins/src/state_plugins/net.rs](../../../crates/op-plugins/src/state_plugins/net.rs) — line 634, `update_interfaces_file`: Direct /etc/network/interfaces ownership.
- [crates/op-plugins/src/state_plugins/net.rs](../../../crates/op-plugins/src/state_plugins/net.rs) — line 791, `calculate_diff / apply_state`: Reconciliation and restricted deletions.
- [crates/op-plugins/src/state_plugins/net.rs](../../../crates/op-plugins/src/state_plugins/net.rs) — line 1036, `all_subids_are_valid`: Local schema identifier test.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 1825, `MutationEngine::mutate net branch`: Distinct legacy OVS method/property operations.
- [crates/op-network/src/rovs_proxy.rs](../../../crates/op-network/src/rovs_proxy.rs) — line 33, `OvsdbDbusClient`: Actual backend is native OVSDB client.
- [crates/op-plugins/src/state_plugins/net.rs](../../../crates/op-plugins/src/state_plugins/net.rs) — line 1066, `inventory::submit! / PluginReg::new`: Source inventory constructor.
- [crates/op-plugins/src/state_plugins/net.rs](../../../crates/op-plugins/src/state_plugins/net.rs) — line 976, `net_schema`: PluginSchema source and declared contract.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2187, `dispatch_method_call_with_identity backend match`: No explicit arm for this plugin; default at line 2356 echoes arguments in success envelope.

<a id="netmaker"></a>

## `netmaker`

**Purpose.** Historical Netmaker enrollment, node/network management and netclient supervision surface.

**Source status.** The module and inventory constructor remain compiled, but default_registry explicitly retires netmaker and the legacy mutate path rejects its invocation. Treat it as retired code, not another supported networking product.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/netmaker.rs:694 — netmaker_schema / NetmakerState`

**Dispatch.** `none`

**Backing.** Unreachable helper code includes HTTP management API calls, direct /etc/netclient/netclient.json reads/writes and a runit D-Bus service controller whose enum/error text still says S6.

**State effects.** Standalone helpers and StatePlugin diff/apply can express enrollment and service-start operations; package installation errors and rollback is explicitly unsupported. A six-method dispatch_netmaker_method exists but is not wired into canonical Call. Checkpoint is null and verification unconditional.

**Test evidence.** No local test block found in the inspected plugin module.

**Uncertainty.** Retirement must remain authoritative despite source scaffolding and stale dependencies elsewhere. Removing or migrating residual callers requires tracing those callers; this baseline does not infer a running netclient or catalog blob.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/mod.rs](../../../crates/op-plugins/src/state_plugins/mod.rs) — line 53, `pub mod netmaker`: Compiled module membership; source registration does not prove live blob presence.
- [crates/op-plugins/src/default_registry.rs](../../../crates/op-plugins/src/default_registry.rs) — line 42, `RETIRED_PLUGIN_IDS`: Explicit retirement.
- [crates/op-plugins/src/state_plugins/netmaker.rs](../../../crates/op-plugins/src/state_plugins/netmaker.rs) — line 82, `ServiceController::detect`: Actual runit D-Bus target with stale S6 naming.
- [crates/op-plugins/src/state_plugins/netmaker.rs](../../../crates/op-plugins/src/state_plugins/netmaker.rs) — line 181, `read_config / write_config`: Direct netclient configuration file backing.
- [crates/op-plugins/src/state_plugins/netmaker.rs](../../../crates/op-plugins/src/state_plugins/netmaker.rs) — line 319, `list_networks_api`: HTTP API helpers.
- [crates/op-plugins/src/state_plugins/netmaker.rs](../../../crates/op-plugins/src/state_plugins/netmaker.rs) — line 556, `apply_state`: Enrollment/service effects; checkpoint/rollback limitations follow.
- [crates/op-plugins/src/state_plugins/netmaker.rs](../../../crates/op-plugins/src/state_plugins/netmaker.rs) — line 848, `dispatch_netmaker_method`: Orphaned method dispatcher.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 1971, `MutationEngine::mutate netmaker arm`: Alternate entry also explicitly rejects retired plugin.
- [crates/op-plugins/src/state_plugins/netmaker.rs](../../../crates/op-plugins/src/state_plugins/netmaker.rs) — line 906, `inventory::submit! / PluginReg::new`: Source inventory constructor.
- [crates/op-plugins/src/state_plugins/netmaker.rs](../../../crates/op-plugins/src/state_plugins/netmaker.rs) — line 694, `netmaker_schema`: PluginSchema source and declared contract.

<a id="notebooklm"></a>

## `notebooklm`

**Purpose.** Expose notebook, source, research, artifact, sharing and authentication operations through the NotebookLM plugin.

**Source status.** Canonical dispatch is explicit and verifies the caller session. It stores notebook selection per session, then maps methods to the pinned jacob-bd nlm executable. Coverage is partial: declared notebook_query_start and notebook_query_status deliberately error because the CLI has no equivalent.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/notebooklm.rs::notebooklm_schema (plugin-owned typed outputs in notebooklm/outputs.rs and notebooklm/provider_outputs.rs)`

**Dispatch.** `partial`

**Backing.** Cozo stores selection; the external nlm CLI owns provider interaction and Google profile state. Dedicated version-4 output types replace a flattened provider map and normalization handles collections and mind maps.

**State effects.** Read/write operations execute the mapped CLI; local selection writes Cozo. StatePlugin apply is inert. Existing command/file handling is observed implementation, not authorization to bypass D-Bus or create another schema authority.

**Test evidence.** Response-contract tests normalize provider fixtures, validate PluginSchema and encode/decode generated protobuf; CLI mapping tests cover invocation forms. None were run in this baseline.

**Uncertainty.** Current source includes uncommitted response work. Live binary/catalog rollout, Google authorization and native-provider behavior remain separate from fixture evidence.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/notebooklm.rs](../../../crates/op-plugins/src/state_plugins/notebooklm.rs) — line 619, `notebooklm_schema`: Current contract.
- [crates/op-plugins/src/state_plugins/notebooklm.rs](../../../crates/op-plugins/src/state_plugins/notebooklm.rs) — line 695, `notebooklm_schema`: Declares asynchronous query method.
- [crates/op-plugins/src/state_plugins/notebooklm.rs](../../../crates/op-plugins/src/state_plugins/notebooklm.rs) — line 1086, `inventory::submit!`: Registration.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2327, `MutationEngine::dispatch_method_call_with_identity`: Verified-session arm.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2428, `MutationEngine::dispatch_notebooklm_method`: Cozo selection and CLI routing.
- [crates/op-grpc-bridge/src/nlm_cli.rs](../../../crates/op-grpc-bridge/src/nlm_cli.rs) — line 111, `nlm_argv`: Async queries explicitly unsupported.
- [crates/op-grpc-bridge/tests/notebooklm_response_contracts.rs](../../../crates/op-grpc-bridge/tests/notebooklm_response_contracts.rs) — line 97, `pinned_provider_objects_and_collections_encode_without_losing_fields`: Meaningful fixture contracts.

<a id="oci"></a>

## `oci`

**Purpose.** Declares image pulling and OCI container creation, lifecycle, state and port attachment.

**Source status.** Compiled schema scaffold with incus and rovs_commands dependencies. Its description promises loopback/network-namespace initialization, but no corresponding runtime implementation appears in this plugin.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/oci.rs:207 — oci_schema / oci_container_fields`

**Dispatch.** `generic_echo`

**Backing.** No OCI binary, container daemon or Incus lifecycle helper is invoked by the plugin's StatePlugin implementation or canonical Call path. Dependencies alone do not connect operations.

**State effects.** Diff is empty, apply_state reports success without changes, verify_state always returns true, checkpoint is null and rollback does nothing. The initial example deliberately contains an empty containers array, avoiding fabricated container state. All seven method responses are AckOutput, including the state query.

**Test evidence.** Existing initial_projection_does_not_fabricate_containers verifies the empty initial example; inspected in this source-only pass.

**Uncertainty.** Container execution and network setup remain declarations in this surface. A generated lifecycle method or dependency entry must not be presented as a usable container provisioning backend.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/mod.rs](../../../crates/op-plugins/src/state_plugins/mod.rs) — line 55, `pub mod oci`: Compiled module membership; source registration does not prove live blob presence.
- [crates/op-plugins/src/state_plugins/oci.rs](../../../crates/op-plugins/src/state_plugins/oci.rs) — line 207, `oci_schema`: Lifecycle declarations, dependencies and empty projection.
- [crates/op-plugins/src/state_plugins/oci.rs](../../../crates/op-plugins/src/state_plugins/oci.rs) — line 338, `StatePlugin for OciPlugin`: Empty diff, effect-free apply and null checkpoint.
- [crates/op-plugins/src/state_plugins/oci.rs](../../../crates/op-plugins/src/state_plugins/oci.rs) — line 399, `initial_projection_does_not_fabricate_containers`: Test verifies initial example does not invent containers.
- [crates/op-plugins/src/state_plugins/oci.rs](../../../crates/op-plugins/src/state_plugins/oci.rs) — line 391, `inventory::submit! / PluginReg::new`: Source inventory constructor.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2187, `dispatch_method_call_with_identity backend match`: No explicit arm for this plugin; default at line 2356 echoes arguments in success envelope.

<a id="openflow"></a>

## `openflow`

**Purpose.** Declares bridge flow policy and controller safety/health operations.

**Source status.** Compiled plugin; six safety/controller methods explicitly dispatch. add_flow, delete_flow and modify_flow explicitly error and direct callers to StatePlugin apply or the separate SendFlow interface.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/openflow.rs:1490 — openflow_schema / OpenFlowConfig`

**Dispatch.** `partial`

**Backing.** Safety methods use op_network helpers. Reconciliation installs/deletes normalized flows through an OpenFlowController D-Bus proxy; its dump query reports controller tracking, not live switch flow-table discovery. Lower-level OpenFlowClient uses native rovs TCP.

**State effects.** Diff covers flows, ports and security configuration. Apply orders port creation, flow creation/deletion and port removal. Verification always returns true; checkpoint is null. These real reconciliation calls must remain separate from the three rejected canonical flow methods.

**Test evidence.** No local test block found in this plugin or inspected OpenFlowClient module.

**Uncertainty.** Successful controller metadata reads do not prove switch state. Proxy ownership, backend reachability and generated response shapes require integration verification; sample privacy-chain topology is not evidence of deployed configuration.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/mod.rs](../../../crates/op-plugins/src/state_plugins/mod.rs) — line 56, `pub mod openflow`: Compiled module membership; source registration does not prove live blob presence.
- [crates/op-plugins/src/state_plugins/openflow.rs](../../../crates/op-plugins/src/state_plugins/openflow.rs) — line 27, `OpenFlowController`: D-Bus proxy service, interface and flow methods.
- [crates/op-plugins/src/state_plugins/openflow.rs](../../../crates/op-plugins/src/state_plugins/openflow.rs) — line 306, `install_flow / query_flows / delete_flow`: Actual reconcile transport and tracked-versus-live query distinction.
- [crates/op-plugins/src/state_plugins/openflow.rs](../../../crates/op-plugins/src/state_plugins/openflow.rs) — line 1125, `calculate_diff`: Flow/port reconciliation.
- [crates/op-plugins/src/state_plugins/openflow.rs](../../../crates/op-plugins/src/state_plugins/openflow.rs) — line 1295, `apply_state`: Ordered backend effects.
- [crates/op-plugins/src/state_plugins/openflow.rs](../../../crates/op-plugins/src/state_plugins/openflow.rs) — line 1778, `dispatch_openflow_method`: Six implemented methods and three explicit flow-method errors.
- [crates/op-network/src/openflow.rs](../../../crates/op-network/src/openflow.rs) — line 112, `OpenFlowClient::connect`: Native rovs TCP transport behind lower-level client.
- [crates/op-plugins/src/state_plugins/openflow.rs](../../../crates/op-plugins/src/state_plugins/openflow.rs) — line 1848, `inventory::submit! / PluginReg::new`: Source inventory constructor.
- [crates/op-plugins/src/state_plugins/openflow.rs](../../../crates/op-plugins/src/state_plugins/openflow.rs) — line 1490, `openflow_schema`: PluginSchema source and declared contract.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2187, `dispatch_method_call_with_identity backend match`: Canonical backend routing for this plugin.

<a id="openflow-obfuscation"></a>

## `openflow_obfuscation`

**Purpose.** Declares privacy flow obfuscation configuration and obfuscate/deobfuscate operations.

**Source status.** Compiled source contains substantive Rust flow-string generation at several security/obfuscation levels, but canonical methods have no backend dispatcher.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/openflow_obfuscation.rs:401 — StatePlugin::schema / OpenFlowObfuscationConfig`

**Dispatch.** `generic_echo`

**Backing.** The implementation generates flow command strings and logs them. It does not execute ovs-ofctl, connect to a controller or install generated flows.

**State effects.** Diff compares config objects and can emit Modify. apply_state ignores the supplied diff, generates from the plugin's existing self.config and returns success after logging; a source comment leaves installation unimplemented. Verification is unconditional, checkpoint is null and rollback logs without undoing anything.

**Test evidence.** Existing level0/1/2/3 and command-generation tests check flow counts and strings; inspected in this source-only pass. They do not verify a switch or controller effect.

**Uncertainty.** Generated flows are algorithm output, not applied protection. Intended policy must determine how schema changes update configuration and enter an authorized flow-installation path before obfuscation can be considered operational.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/mod.rs](../../../crates/op-plugins/src/state_plugins/mod.rs) — line 57, `pub mod openflow_obfuscation`: Compiled module membership; source registration does not prove live blob presence.
- [crates/op-plugins/src/state_plugins/openflow_obfuscation.rs](../../../crates/op-plugins/src/state_plugins/openflow_obfuscation.rs) — line 103, `generate_level1_flows`: Pure flow-generation implementation.
- [crates/op-plugins/src/state_plugins/openflow_obfuscation.rs](../../../crates/op-plugins/src/state_plugins/openflow_obfuscation.rs) — line 401, `StatePlugin::schema`: Typed config and two declared methods.
- [crates/op-plugins/src/state_plugins/openflow_obfuscation.rs](../../../crates/op-plugins/src/state_plugins/openflow_obfuscation.rs) — line 451, `calculate_diff`: Config comparison creates Modify.
- [crates/op-plugins/src/state_plugins/openflow_obfuscation.rs](../../../crates/op-plugins/src/state_plugins/openflow_obfuscation.rs) — line 477, `apply_state`: Ignores diff; generates/logs without installation.
- [crates/op-plugins/src/state_plugins/openflow_obfuscation.rs](../../../crates/op-plugins/src/state_plugins/openflow_obfuscation.rs) — line 581, `tests`: String/count tests at all obfuscation levels.
- [crates/op-plugins/src/state_plugins/openflow_obfuscation.rs](../../../crates/op-plugins/src/state_plugins/openflow_obfuscation.rs) — line 682, `inventory::submit! / PluginReg::new`: Source inventory constructor.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2187, `dispatch_method_call_with_identity backend match`: No explicit arm for this plugin; default at line 2356 echoes arguments in success envelope.

<a id="oscal-subid-registry"></a>

## `oscal_subid_registry`

**Purpose.** Describe the dual-identifier registry linking machine UUIDs with operational OSCAL taxonomy keys.

**Source status.** Five schema methods declare register, lookup, list_by_category, resolve and export, but every request uses an empty unit input and canonical dispatch has no oscal_subid_registry arm. Calls therefore reach generic argument echo rather than a registry query.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/oscal_subid_registry.rs::oscal_subid_registry_schema`

**Dispatch.** `generic_echo`

**Backing.** The source defines rich OscalSubidRegistryEntry metadata. CozoGraphShuttle separately supports subid registration, while cognitive_mcp.oscal_subids reads sealed-plugin metadata; neither is connected to this plugin’s methods.

**State effects.** StatePlugin diff/apply/rollback are no-ops; availability and verification always report true and checkpoints are null. Generic method auditing records the invocation without inserting or resolving a registry entry.

**Test evidence.** No local plugin behavior tests were found. Workspace taxonomy checks concern declarations, not these methods’ runtime effects; no tests were executed.

**Uncertainty.** Which registry operation should be canonical remains a source integration issue. Do not build another schema tree to bridge it, and do not mistake the working cognitive lookup for coverage of this plugin.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/oscal_subid_registry.rs](../../../crates/op-plugins/src/state_plugins/oscal_subid_registry.rs) — line 391, `oscal_subid_registry_schema`: Five parameterless method declarations.
- [crates/op-plugins/src/state_plugins/oscal_subid_registry.rs](../../../crates/op-plugins/src/state_plugins/oscal_subid_registry.rs) — line 537, `OscalSubidRegistryPlugin::apply_state`: No-op.
- [crates/op-plugins/src/state_plugins/oscal_subid_registry.rs](../../../crates/op-plugins/src/state_plugins/oscal_subid_registry.rs) — line 578, `inventory::submit!`: Registration.
- [crates/op-cozo-store/src/lib.rs](../../../crates/op-cozo-store/src/lib.rs) — line 577, `CozoGraphShuttle::register_subid`: Separate real backing helper.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 3264, `MutationEngine::dispatch_cognitive_mcp_method`: Different plugin’s sealed metadata reader.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2356, `MutationEngine::dispatch_method_call_with_identity`: No specific arm; canonical result echoes input.

<a id="ovsdb-bridge"></a>

## `ovsdb_bridge`

**Purpose.** Declares OVSDB protocol operations and bridge/port conveniences.

**Source status.** Compiled schema has fourteen methods. Canonical Call implements only list_dbs, get_schema and transact; every other declared operation returns a missing-dispatch error.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/ovsdb_bridge.rs:366 — ovsdb_bridge_schema`

**Dispatch.** `partial`

**Backing.** OvsdbDbusClient wraps native vendor rovs_ovsdb over the configured/default Unix socket. Its compatibility name does not imply a D-Bus hop. It lazily connects and retries once after errors.

**State effects.** Canonical transact can mutate the database. The plugin's query_bridges helper builds a partial hierarchy with fallback/default data, but StatePlugin diff/apply are no-ops, verification unconditional and checkpoint null. Its availability check only tests socket-path existence.

**Test evidence.** No local test block in the plugin or inspected rovs_proxy module. An existing OvsdbMirror integration test rejects unauthenticated transact; this is a separate surface, not PluginV1.Call behavior. Inspected, not executed.

**Uncertainty.** Monitoring/locking and convenience declarations are not implemented routes. Default-filled query results and socket presence do not establish live topology or health; retrying mutations also needs semantic review.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/mod.rs](../../../crates/op-plugins/src/state_plugins/mod.rs) — line 59, `pub mod ovsdb_bridge`: Compiled module membership; source registration does not prove live blob presence.
- [crates/op-plugins/src/state_plugins/ovsdb_bridge.rs](../../../crates/op-plugins/src/state_plugins/ovsdb_bridge.rs) — line 208, `query_bridges`: Partial/default-filled bridge hierarchy helper.
- [crates/op-plugins/src/state_plugins/ovsdb_bridge.rs](../../../crates/op-plugins/src/state_plugins/ovsdb_bridge.rs) — line 299, `is_available`: Socket existence check.
- [crates/op-plugins/src/state_plugins/ovsdb_bridge.rs](../../../crates/op-plugins/src/state_plugins/ovsdb_bridge.rs) — line 312, `StatePlugin reconciliation`: No-op diff/apply and unconditional verification.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 3086, `dispatch_ovsdb_bridge_method`: Only three methods implemented, others fail explicitly.
- [crates/op-network/src/rovs_proxy.rs](../../../crates/op-network/src/rovs_proxy.rs) — line 47, `OvsdbDbusClient::with_client`: Native connection plus one reconnect/retry.
- [crates/op-grpc-bridge/tests/oracle_assertion_e2e.rs](../../../crates/op-grpc-bridge/tests/oracle_assertion_e2e.rs) — line 934, `oracle_assertion_cannot_authenticate_unguarded_ovsdb_transact`: Existing authentication integration test, not topology validation.
- [crates/op-plugins/src/state_plugins/ovsdb_bridge.rs](../../../crates/op-plugins/src/state_plugins/ovsdb_bridge.rs) — line 586, `inventory::submit! / PluginReg::new`: Source inventory constructor.
- [crates/op-plugins/src/state_plugins/ovsdb_bridge.rs](../../../crates/op-plugins/src/state_plugins/ovsdb_bridge.rs) — line 366, `ovsdb_bridge_schema`: PluginSchema source and declared contract.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2187, `dispatch_method_call_with_identity backend match`: Canonical backend routing for this plugin.

<a id="packagekit"></a>

## `packagekit`

**Purpose.** Declare software package resolution, installation/removal, updates and name search through PackageKit.

**Source status.** Six method contracts are advertised but canonical dispatch does not call PackageKit. Separate state application is implemented for selected package changes. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/packagekit.rs:414 packagekit_schema`

**Dispatch.** `generic_echo`

**Backing.** State helpers create system-bus PackageKit transactions and call InstallPackages/RemovePackages; installed-state checks directly read package databases.

**State effects.** apply performs install/remove for Create/Delete, while Modify only reports unimplemented. ensure=latest is not resolved by calculate_diff. Successful D-Bus request completion is not proof of transaction Finished success.

**Test evidence.** No local test module or targeted plugin integration test was found in inspected plugin/bridge tests; mock transaction outcomes were not established.

**Uncertainty.** PackageKit availability, package-ID resolution and asynchronous transaction completion need verification before claiming package convergence. Direct database reads remain policy drift. No runtime presence or deployment was checked.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/packagekit.rs](../../../crates/op-plugins/src/state_plugins/packagekit.rs) — line 414, `packagekit_schema`: Plugin-owned contract and source description.
- [crates/op-plugins/src/state_plugins/packagekit.rs](../../../crates/op-plugins/src/state_plugins/packagekit.rs) — line 275, `StatePlugin::apply_state`: Separate state-application behavior described above.
- [crates/op-plugins/src/state_plugins/packagekit.rs](../../../crates/op-plugins/src/state_plugins/packagekit.rs) — line 374, `inventory::submit! / PluginReg::new`: Compiled source registration, not live blob presence.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2356, `MutationEngine::dispatch_method_call_with_identity`: No dedicated plugin arm; canonical method dispatch reaches the generic request echo.
- [crates/op-plugins/src/state_plugins/packagekit.rs](../../../crates/op-plugins/src/state_plugins/packagekit.rs) — line 87, `PackageKitPlugin::install_via_direct`: Real separate apply backing uses D-Bus transaction calls.

<a id="pcidecl"></a>

## `pcidecl`

**Purpose.** Represent PCI device declarations, presence constraints and driver override intent.

**Source status.** Two read methods advertise enumeration/device information with AckOutput. They are independent of the implemented StatePlugin reconciliation path. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/pcidecl.rs:139 PciDeclPlugin::schema / add_pcidecl_methods`

**Dispatch.** `generic_echo`

**Backing.** Private helpers read vendor/device/driver information from /sys/bus/pci/devices and write driver_override through native filesystem access.

**State effects.** calculate_diff handles observe-only and enforcement. apply can write requested overrides; delete is reported unsupported. verify checks compliance but defaults malformed declarations to an empty list, potentially returning true. Rollback is a no-op.

**Test evidence.** No local test module or targeted canonical/apply integration test was found in the inspected test sources.

**Uncertainty.** Driver override intent does not prove device rebinding or rollback. Missing devices become NoOp actions; filesystem access conflicts with current control-plane policy. No runtime presence or deployment was checked.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/pcidecl.rs](../../../crates/op-plugins/src/state_plugins/pcidecl.rs) — line 139, `PciDeclPlugin::schema / add_pcidecl_methods`: Plugin-owned contract and source description.
- [crates/op-plugins/src/state_plugins/pcidecl.rs](../../../crates/op-plugins/src/state_plugins/pcidecl.rs) — line 220, `StatePlugin::apply_state`: Separate state-application behavior described above.
- [crates/op-plugins/src/state_plugins/pcidecl.rs](../../../crates/op-plugins/src/state_plugins/pcidecl.rs) — line 300, `inventory::submit! / PluginReg::new`: Compiled source registration, not live blob presence.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2356, `MutationEngine::dispatch_method_call_with_identity`: No dedicated plugin arm; canonical method dispatch reaches the generic request echo.
- [crates/op-plugins/src/state_plugins/pcidecl.rs](../../../crates/op-plugins/src/state_plugins/pcidecl.rs) — line 258, `PciDeclPlugin::verify_state`: Malformed desired input defaults to empty declaration list.

<a id="persona"></a>

## `persona`

**Purpose.** Expose the built-in agent persona catalog and execute supported agent operations.

**Source status.** The canonical persona arm reaches a plugin-owned dispatcher. get_identity reads op_agents::builtin_agent_descriptors and execute constructs an AgentTask then calls the chosen agent. Coverage is partial: set_identity always rejects and authenticate always returns false without issuing a token.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/persona.rs::persona_schema`

**Dispatch.** `partial`

**Backing.** Catalog data comes from compiled op-agents implementations; execution uses create_agent and agent.execute. There is no mutable persona database or separate identity authority in this plugin.

**State effects.** Executing an agent may have its operation’s side effects; the wrapper returns success/data/metadata or a false success result. StatePlugin rejects non-NoOp changes because the catalog is code-defined, verifies against that catalog and does not support rollback.

**Test evidence.** No local persona behavior tests were located. Individual agent tests, where present, would not alone establish this dispatcher’s authorization or payload contract. Nothing was executed.

**Uncertainty.** Agent task execution is separate from human/session authentication. Per-operation safety, output/error conventions and unsupported declared persona mutations remain source questions.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/persona.rs](../../../crates/op-plugins/src/state_plugins/persona.rs) — line 227, `PersonaPlugin::live`: Compiled descriptors.
- [crates/op-plugins/src/state_plugins/persona.rs](../../../crates/op-plugins/src/state_plugins/persona.rs) — line 268, `dispatch_persona_method`: Named dispatch.
- [crates/op-plugins/src/state_plugins/persona.rs](../../../crates/op-plugins/src/state_plugins/persona.rs) — line 284, `dispatch_persona_method`: Unsupported mutation and authentication placeholder.
- [crates/op-plugins/src/state_plugins/persona.rs](../../../crates/op-plugins/src/state_plugins/persona.rs) — line 291, `dispatch_persona_method`: Real agent execution.
- [crates/op-plugins/src/state_plugins/persona.rs](../../../crates/op-plugins/src/state_plugins/persona.rs) — line 372, `PersonaPlugin::apply_state`: Read-only catalog enforcement.
- [crates/op-plugins/src/state_plugins/persona.rs](../../../crates/op-plugins/src/state_plugins/persona.rs) — line 425, `persona_schema`: Contract.
- [crates/op-plugins/src/state_plugins/persona.rs](../../../crates/op-plugins/src/state_plugins/persona.rs) — line 513, `inventory::submit!`: Registration.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2230, `MutationEngine::dispatch_method_call_with_identity`: Canonical arm.

<a id="privacy-router"></a>

## `privacy_router`

**Purpose.** Historical system privacy fabric combining Incus containers, WireGuard/WARP, OVS and OpenFlow policy.

**Source status.** A source file and inventory macro exist, but state_plugins/mod.rs does not declare the module. Therefore its constructor, schema, tests and helpers are not compiled into the current plugin inventory.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/privacy_router.rs:1194 — privacy_router_schema / PrivacyRouterConfig`

**Dispatch.** `none`

**Backing.** Source helpers attempt OVS/native rtnetlink, Incus reconciliation and controller operations. However query_privacy_routes, query_incus_state and query_openflow_state deserialize literal null instead of querying state; Incus application repeats this failure.

**State effects.** If re-enabled, apply would attempt bridge/WARP setup then containers and flows, but these missing state sources prevent a coherent reconciler. Verify returns true, checkpoint is null and rollback errors. Three declared policy/audit/log methods have no canonical dispatcher.

**Owner topology.** Owner-provided topology: WireGuard terminates at the decoy; there is no WireGuard on this server. Identity is transported by WARP. This historical source intent was not live-validated and does not establish a server tunnel backend.

**Test evidence.** Six local pure configuration/container-selection tests exist in the uncompiled file; inspected in this source-only pass.

**Uncertainty.** Stale Netmaker provisioning assumptions and blocked state reads make this implementation debt, not a second supported product. privacy_routes still declares a dependency on this unavailable module.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/mod.rs](../../../crates/op-plugins/src/state_plugins/mod.rs) — line 64, `state_plugins module declarations`: privacy_routes is declared; privacy_router is absent from the complete module inventory.
- [crates/op-plugins/src/state_plugins/privacy_router.rs](../../../crates/op-plugins/src/state_plugins/privacy_router.rs) — line 330, `query_privacy_routes / query_incus_state / query_openflow_state`: Literal-null state stubs.
- [crates/op-plugins/src/state_plugins/privacy_router.rs](../../../crates/op-plugins/src/state_plugins/privacy_router.rs) — line 471, `ensure_warp_interface_on_bridge`: Native OVS/netlink effects and stale Netmaker requirement.
- [crates/op-plugins/src/state_plugins/privacy_router.rs](../../../crates/op-plugins/src/state_plugins/privacy_router.rs) — line 776, `apply_incus_system_containers`: Null deserialization blocks container reconciliation.
- [crates/op-plugins/src/state_plugins/privacy_router.rs](../../../crates/op-plugins/src/state_plugins/privacy_router.rs) — line 1002, `apply_state`: Intended ordered reconciliation and ineffective verification/checkpoint.
- [crates/op-plugins/src/state_plugins/privacy_router.rs](../../../crates/op-plugins/src/state_plugins/privacy_router.rs) — line 1091, `tests`: Configuration/selection tests in uncompiled source.
- [crates/op-plugins/src/state_plugins/privacy_routes.rs](../../../crates/op-plugins/src/state_plugins/privacy_routes.rs) — line 451, `privacy_routes_schema dependencies`: Dependency on uncompiled privacy_router.
- [crates/op-plugins/src/state_plugins/privacy_router.rs](../../../crates/op-plugins/src/state_plugins/privacy_router.rs) — line 1523, `inventory::submit! / PluginReg::new`: Registration exists in uncompiled file only.
- [crates/op-plugins/src/state_plugins/privacy_router.rs](../../../crates/op-plugins/src/state_plugins/privacy_router.rs) — line 1194, `privacy_router_schema`: PluginSchema source and declared contract.

<a id="privacy-routes"></a>

## `privacy_routes`

**Purpose.** Stores per-user privacy-route metadata and exposes list/delete operations.

**Source status.** Compiled plugin with both declared methods explicitly dispatched. Its separate schema dependencies include wireguard and the uncompiled privacy_router module.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/privacy_routes.rs:230 — privacy_routes_schema / PrivacyRoutesState`

**Dispatch.** `explicit`

**Backing.** A direct JSON file at /var/lib/op-dbus/privacy-routes.json is the method and reconciliation store. Any file-read error returns an empty store; malformed JSON errors. Writes replace file contents without an atomic temporary-file protocol.

**State effects.** Diff compares routes by ID; apply creates/modifies/deletes metadata and sorts before saving. Canonical delete_route changes this store, not OVS flows or tunnel state. Verify is unconditional; null checkpoints cannot deserialize into rollback state despite advertised rollback support.

**Owner topology.** Owner-provided topology: WireGuard terminates at the decoy; there is no WireGuard on this server. Identity is transported by WARP. Route metadata here does not establish server tunnel enforcement.

**Test evidence.** No local test block found in the inspected plugin module.

**Uncertainty.** Direct disk-backed present state diverges from the sealed-plugin authority rule. Metadata deletion does not establish network enforcement removal, and dependency resolution cannot currently supply privacy_router.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/mod.rs](../../../crates/op-plugins/src/state_plugins/mod.rs) — line 64, `pub mod privacy_routes`: Compiled module membership; source registration does not prove live blob presence.
- [crates/op-plugins/src/state_plugins/privacy_routes.rs](../../../crates/op-plugins/src/state_plugins/privacy_routes.rs) — line 13, `PRIVACY_ROUTES_STATE_PATH`: Separate disk state path.
- [crates/op-plugins/src/state_plugins/privacy_routes.rs](../../../crates/op-plugins/src/state_plugins/privacy_routes.rs) — line 57, `load_store / save_store`: Read-error-to-empty and direct writes.
- [crates/op-plugins/src/state_plugins/privacy_routes.rs](../../../crates/op-plugins/src/state_plugins/privacy_routes.rs) — line 97, `calculate_diff / apply_state`: Route metadata reconciliation.
- [crates/op-plugins/src/state_plugins/privacy_routes.rs](../../../crates/op-plugins/src/state_plugins/privacy_routes.rs) — line 199, `verify_state / create_checkpoint / rollback`: Unconditional verify and unusable null checkpoint.
- [crates/op-plugins/src/state_plugins/privacy_routes.rs](../../../crates/op-plugins/src/state_plugins/privacy_routes.rs) — line 451, `privacy_routes_schema`: wireguard/privacy_router dependencies.
- [crates/op-plugins/src/state_plugins/privacy_routes.rs](../../../crates/op-plugins/src/state_plugins/privacy_routes.rs) — line 492, `dispatch_privacy_routes_method`: Explicit list/delete disk-backed methods.
- [crates/op-plugins/src/state_plugins/privacy_routes.rs](../../../crates/op-plugins/src/state_plugins/privacy_routes.rs) — line 519, `inventory::submit! / PluginReg::new`: Source inventory constructor.
- [crates/op-plugins/src/state_plugins/privacy_routes.rs](../../../crates/op-plugins/src/state_plugins/privacy_routes.rs) — line 230, `privacy_routes_schema`: PluginSchema source and declared contract.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2187, `dispatch_method_call_with_identity backend match`: Canonical backend routing for this plugin.

<a id="procfs"></a>

## `procfs`

**Purpose.** Describe read-only memory, CPU, process, load, uptime, networking and kernel observations.

**Source status.** Seven methods advertise process and host-statistic reads, with generic acknowledgement result types. Detailed parsers and a gather function exist independently of canonical dispatch. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/procfs.rs:212 procfs_schema`

**Dispatch.** `generic_echo`

**Backing.** gather_procfs_state joins ten native /proc readers; parser helpers normalize values and units. The top-level gather helper has no located caller in current Rust source.

**State effects.** Diff emits NoOp, apply changes nothing, verification returns true and checkpoint is null. Neither a registered schema nor parser implementation means the declared read methods collect live data.

**Test evidence.** Local tests cover OSCAL annotations, load/memory normalization and typed mounts/network counter fields; they do not exercise canonical transport reads.

**Uncertainty.** Direct /proc access is implementation drift under current policy. Empty defaults on file-read failure may conceal unavailable observations. No runtime presence or deployment was checked.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/procfs.rs](../../../crates/op-plugins/src/state_plugins/procfs.rs) — line 212, `procfs_schema`: Plugin-owned contract and source description.
- [crates/op-plugins/src/state_plugins/procfs.rs](../../../crates/op-plugins/src/state_plugins/procfs.rs) — line 174, `StatePlugin::apply_state`: Separate state-application behavior described above.
- [crates/op-plugins/src/state_plugins/procfs.rs](../../../crates/op-plugins/src/state_plugins/procfs.rs) — line 725, `inventory::submit! / PluginReg::new`: Compiled source registration, not live blob presence.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2356, `MutationEngine::dispatch_method_call_with_identity`: No dedicated plugin arm; canonical method dispatch reaches the generic request echo.
- [crates/op-plugins/src/state_plugins/procfs.rs](../../../crates/op-plugins/src/state_plugins/procfs.rs) — line 332, `gather_procfs_state`: Separate native parser/gathering implementation.
- [crates/op-plugins/src/state_plugins/procfs.rs](../../../crates/op-plugins/src/state_plugins/procfs.rs) — line 802, `mounts_is_an_array_of_typed_rows`: Field-shape coverage, not transport execution.

<a id="project"></a>

## `project`

**Purpose.** Provide container-backed projects, session membership, invitations, documents, agent selection and mode-context resolution.

**Source status.** All ten methods have an explicit canonical dispatch path. The dispatcher revalidates the session, genesis, active registered principal and exact method grant, then serializes ACL and document access. Project ceilings intersect existing grants rather than create authority.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/project.rs::project_schema`

**Dispatch.** `explicit`

**Backing.** A private Cozo catalog shares the identity store; each project has a canonical UUID directory and separate persistent Cozo document store. Incus helpers create/verify the project container and persistent mount.

**State effects.** Create/invite/revoke/select/put mutate catalog or documents. Inputs are hashed instead of copied into global audit events. Generic StatePlugin apply/checkpoint/rollback explicitly reject unscoped operations.

**Test evidence.** Tests cover missing membership, reader restrictions, invitation target/expiry/single use, capability ceilings and schema privacy. No container or end-to-end method tests were run.

**Uncertainty.** OD-46’s source context resolver does not establish enforced private chat execution. Actual mounts, images, grants and downstream scoped-memory use remain unverified; the assistant chatbot must not be redefined as a project container.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/project.rs](../../../crates/op-plugins/src/state_plugins/project.rs) — line 85, `Project::authorize`: Session ACL.
- [crates/op-plugins/src/state_plugins/project.rs](../../../crates/op-plugins/src/state_plugins/project.rs) — line 291, `ProjectPlugin::calculate_diff`: Unscoped apply rejected.
- [crates/op-plugins/src/state_plugins/project.rs](../../../crates/op-plugins/src/state_plugins/project.rs) — line 316, `project_schema`: Ten methods.
- [crates/op-plugins/src/state_plugins/project.rs](../../../crates/op-plugins/src/state_plugins/project.rs) — line 445, `tests::acl_does_not_infer_membership_or_elevate_reader`: Meaningful ACL test.
- [crates/op-plugins/src/state_plugins/project.rs](../../../crates/op-plugins/src/state_plugins/project.rs) — line 422, `inventory::submit!`: Registration.
- [crates/op-grpc-bridge/src/project_dispatch.rs](../../../crates/op-grpc-bridge/src/project_dispatch.rs) — line 81, `project_store`: Private persistent project storage.
- [crates/op-grpc-bridge/src/project_dispatch.rs](../../../crates/op-grpc-bridge/src/project_dispatch.rs) — line 174, `dispatch`: Verified session and grant enforcement.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2219, `MutationEngine::dispatch_method_call_with_identity`: Canonical arm.

<a id="proxy-server"></a>

## `proxy_server`

**Purpose.** Declares proxy enabled/port state and start, stop and configure operations.

**Source status.** Compiled minimal schema scaffold. Methods carry typed request fields such as proxy name, port, enabled and upstream, but canonical Call has no proxy-specific implementation.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/proxy_server.rs:135 — proxy_server_schema / ProxyServerState`

**Dispatch.** `generic_echo`

**Backing.** No proxy server, listener, process manager or upstream connection is created or contacted by this plugin. Declared output for each operation is AckOutput.

**State effects.** StatePlugin calculates an empty diff and reports successful apply without effects. Verification always returns true; checkpoint is null and rollback does nothing. Canonical Call returns request arguments inside an audited success envelope, which is not evidence that a proxy started or its settings changed.

**Test evidence.** No local test block found in the inspected plugin module.

**Uncertainty.** The schema is useful as scaffolding but does not specify operational ownership, configuration persistence or service health. Those must be implemented through the authorized plugin path before clients rely on lifecycle responses.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/mod.rs](../../../crates/op-plugins/src/state_plugins/mod.rs) — line 67, `pub mod proxy_server`: Compiled module membership; source registration does not prove live blob presence.
- [crates/op-plugins/src/state_plugins/proxy_server.rs](../../../crates/op-plugins/src/state_plugins/proxy_server.rs) — line 17, `ProxyServerState`: Minimal enabled/port state model.
- [crates/op-plugins/src/state_plugins/proxy_server.rs](../../../crates/op-plugins/src/state_plugins/proxy_server.rs) — line 86, `StatePlugin for ProxyServerPlugin`: Effect-free reconcile, unconditional verify and null checkpoint.
- [crates/op-plugins/src/state_plugins/proxy_server.rs](../../../crates/op-plugins/src/state_plugins/proxy_server.rs) — line 135, `proxy_server_schema`: Typed lifecycle/configuration method declarations.
- [crates/op-plugins/src/state_plugins/proxy_server.rs](../../../crates/op-plugins/src/state_plugins/proxy_server.rs) — line 215, `inventory::submit! / PluginReg::new`: Source inventory constructor.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2187, `dispatch_method_call_with_identity backend match`: No explicit arm for this plugin; default at line 2356 echoes arguments in success envelope.

<a id="qdrant"></a>

## `qdrant`

**Purpose.** Declare Qdrant collection, point, search, snapshot and payload-index operations.

**Source status.** Ten methods are present in qdrant_schema, but the canonical dispatcher has no qdrant arm and echoes the supplied input. The QdrantPlugin body owns no client, while another crate has a real QdrantSemanticShuttle.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/qdrant.rs::qdrant_schema`

**Dispatch.** `generic_echo`

**Backing.** Plugin defaults are illustrative version/collection data and environment-derived endpoints; the plugin owns no client. Separately, the cognitive QdrantSemanticShuttle uses the Qdrant gRPC endpoint (normally 6334), with a five-second startup health timeout. Its retrieval implementation does not wire the qdrant plugin's methods.

**State effects.** StatePlugin diff/apply/rollback do no work, verify_state always succeeds and checkpoint is null. Declared point/collection mutations therefore produce common audit records without Qdrant writes through this path.

**Test evidence.** No local qdrant-plugin behavior tests were found. The external Voyage/Qdrant test exercises the separate shuttle and returns early when credentials or service are absent; a passing workspace gate does not establish a live provider call.

**Uncertainty.** Provider reachability, actual collections, native response shapes and method binding remain unverified. Illustrative green status must not be treated as observed health.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/qdrant.rs](../../../crates/op-plugins/src/state_plugins/qdrant.rs) — line 253, `QdrantPlugin::exemplar_state`: Illustrative version/collection.
- [crates/op-plugins/src/state_plugins/qdrant.rs](../../../crates/op-plugins/src/state_plugins/qdrant.rs) — line 343, `QdrantPlugin::apply_state`: No-op.
- [crates/op-plugins/src/state_plugins/qdrant.rs](../../../crates/op-plugins/src/state_plugins/qdrant.rs) — line 384, `qdrant_schema`: Ten declared methods.
- [crates/op-plugins/src/state_plugins/qdrant.rs](../../../crates/op-plugins/src/state_plugins/qdrant.rs) — line 611, `inventory::submit!`: Registration.
- [crates/op-cognitive-mcp/src/qdrant_shuttle.rs](../../../crates/op-cognitive-mcp/src/qdrant_shuttle.rs) — line 41, `QdrantSemanticShuttle::new`: Separate real client.
- [crates/op-cognitive-mcp/tests/e2e_voyage_qdrant.rs](../../../crates/op-cognitive-mcp/tests/e2e_voyage_qdrant.rs) — line 46, `voyage_vectorization_lands_in_qdrant_and_is_retrievable`: Other path; conditional skip.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2356, `MutationEngine::dispatch_method_call_with_identity`: No qdrant arm; echo fallback.
- [crates/op-cognitive-mcp/src/qdrant_shuttle.rs](../../../crates/op-cognitive-mcp/src/qdrant_shuttle.rs) — line 76, `QdrantSemanticShuttle::new_with_clients`: Qdrant gRPC client and bounded health check.

<a id="rovs-commands"></a>

## `rovs_commands`

**Purpose.** Exposes common OVS bridge/port creation, deletion and listing operations.

**Source status.** Compiled plugin; all eight schema methods have explicit MutationEngine implementations. Its standalone StatePlugin remains schema-only, with empty diff and effect-free apply.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/rovs_commands.rs:197 — rovs_commands_schema`

**Dispatch.** `explicit`

**Backing.** Operations call OvsdbDbusClient, which is a native rovs_ovsdb Unix-socket client. These are actual backend calls despite the compatibility type and dispatcher comment saying proxy.

**State effects.** Canonical methods create/delete bridges, add/remove ports and ensure internal ports; list methods read OVSDB. Verification is unconditional and checkpoint null. create_bridge accepts a declared datapath_type that its dispatcher ignores.

**Test evidence.** Existing internal_port_method_is_typed_and_oscal_authorized checks the typed internal-port contract and authority metadata; inspected in this source-only pass.

**Uncertainty.** Most schema outputs are AckOutput while implementation returns created/deleted objects or bare arrays; only ensure_internal_port has a matching domain shape. Generated wrappers and generic envelopes therefore need contract tests, not assumptions from successful native operations.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/mod.rs](../../../crates/op-plugins/src/state_plugins/mod.rs) — line 69, `pub mod rovs_commands`: Compiled module membership; source registration does not prove live blob presence.
- [crates/op-plugins/src/state_plugins/rovs_commands.rs](../../../crates/op-plugins/src/state_plugins/rovs_commands.rs) — line 27, `CreateBridgeInput`: Declared datapath_type input.
- [crates/op-plugins/src/state_plugins/rovs_commands.rs](../../../crates/op-plugins/src/state_plugins/rovs_commands.rs) — line 141, `StatePlugin for RovsCommandsPlugin`: Schema-only reconciliation.
- [crates/op-plugins/src/state_plugins/rovs_commands.rs](../../../crates/op-plugins/src/state_plugins/rovs_commands.rs) — line 197, `rovs_commands_schema`: Eight declarations and output contracts.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2991, `dispatch_rovs_commands_method`: All methods wired; actual response structures and ignored datapath_type.
- [crates/op-network/src/rovs_proxy.rs](../../../crates/op-network/src/rovs_proxy.rs) — line 33, `OvsdbDbusClient`: Native OVSDB backend.
- [crates/op-plugins/src/state_plugins/rovs_commands.rs](../../../crates/op-plugins/src/state_plugins/rovs_commands.rs) — line 383, `internal_port_method_is_typed_and_oscal_authorized`: Typed/authority contract test.
- [crates/op-plugins/src/state_plugins/rovs_commands.rs](../../../crates/op-plugins/src/state_plugins/rovs_commands.rs) — line 374, `inventory::submit! / PluginReg::new`: Source inventory constructor.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2187, `dispatch_method_call_with_identity backend match`: Canonical backend routing for this plugin.

<a id="rtnetlink"></a>

## `rtnetlink`

**Purpose.** Declares native kernel interface, address, route and rule controls.

**Source status.** Compiled plugin with nine method declarations but no canonical method dispatcher. Its manual state schema models addresses as CIDR strings, while the Rust reconciler expects AddressEntry objects containing ip and prefix.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/rtnetlink.rs:343 — rtnetlink_schema / RtnetlinkState`

**Dispatch.** `generic_echo`

**Backing.** Separate StatePlugin apply calls op_network::rtnetlink directly for MAC, IPv4 address, link up/down and default-route replacement. This is native kernel netlink, not a D-Bus backend.

**State effects.** Diff only modifies interfaces already present in supplied current state; absent interfaces are logged and skipped. Apply accumulates backend errors. Verification always returns true, checkpoint is null and rollback can yield an empty diff because its comparison starts from null.

**Test evidence.** No local plugin tests found. The backing test_list_routes_for_loopback merely calls list_routes_for_interface, which is an empty-vector stub; inspected in this source-only pass.

**Uncertainty.** Schema-valid addresses can fail typed state decoding and silently default empty. Declared add/delete link, route and rule methods must not be confused with the narrower reconciler.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/mod.rs](../../../crates/op-plugins/src/state_plugins/mod.rs) — line 70, `pub mod rtnetlink`: Compiled module membership; source registration does not prove live blob presence.
- [crates/op-plugins/src/state_plugins/rtnetlink.rs](../../../crates/op-plugins/src/state_plugins/rtnetlink.rs) — line 21, `RtnetlinkInterfaceConfig / AddressEntry`: Rust address object shape.
- [crates/op-plugins/src/state_plugins/rtnetlink.rs](../../../crates/op-plugins/src/state_plugins/rtnetlink.rs) — line 96, `calculate_diff`: Only existing interfaces modified; malformed states default empty.
- [crates/op-plugins/src/state_plugins/rtnetlink.rs](../../../crates/op-plugins/src/state_plugins/rtnetlink.rs) — line 144, `apply_state`: Actual native link/address/default-route effects.
- [crates/op-plugins/src/state_plugins/rtnetlink.rs](../../../crates/op-plugins/src/state_plugins/rtnetlink.rs) — line 254, `rollback`: Null-current comparison cannot restore absent interfaces.
- [crates/op-plugins/src/state_plugins/rtnetlink.rs](../../../crates/op-plugins/src/state_plugins/rtnetlink.rs) — line 374, `rtnetlink_schema addresses field`: Conflicting array-of-CIDR-strings schema.
- [crates/op-network/src/rtnetlink.rs](../../../crates/op-network/src/rtnetlink.rs) — line 536, `list_routes_for_interface`: Backing route-list helper is a stub.
- [crates/op-network/src/rtnetlink.rs](../../../crates/op-network/src/rtnetlink.rs) — line 586, `test_list_routes_for_loopback`: Existing test cannot validate actual route enumeration.
- [crates/op-plugins/src/state_plugins/rtnetlink.rs](../../../crates/op-plugins/src/state_plugins/rtnetlink.rs) — line 623, `inventory::submit! / PluginReg::new`: Source inventory constructor.
- [crates/op-plugins/src/state_plugins/rtnetlink.rs](../../../crates/op-plugins/src/state_plugins/rtnetlink.rs) — line 343, `rtnetlink_schema`: PluginSchema source and declared contract.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2187, `dispatch_method_call_with_identity backend match`: No explicit arm for this plugin; default at line 2356 echoes arguments in success envelope.

<a id="schema-renderer"></a>

## `schema_renderer`

**Purpose.** Describe schema-to-UI field mappings, supported layouts and gallery rendering options.

**Source status.** Four methods advertise rendering, gallery listing, rendering lookup and validation. Requests are unit inputs despite operations needing meaningful schema or identifier data. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/schema_renderer.rs:185 schema_renderer_schema`

**Dispatch.** `generic_echo`

**Backing.** SchemaRendererState::default constructs in-source UI mappings and layouts. No renderer runtime, saved-rendering store or external UI client is called by this plugin.

**State effects.** Diff/apply are no-ops; checkpoint serializes default configuration and verification returns true. The declared render/validate operations do not execute rendering or validation through canonical dispatch.

**Test evidence.** The local all_subids_are_valid test checks annotations only; no rendered-output, schema rejection or gallery persistence test was located.

**Uncertainty.** The active frontend is external and may implement separate renderer behavior; this plugin’s inventory must not be treated as its executed registry. No runtime presence or deployment was checked.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/schema_renderer.rs](../../../crates/op-plugins/src/state_plugins/schema_renderer.rs) — line 185, `schema_renderer_schema`: Plugin-owned contract and source description.
- [crates/op-plugins/src/state_plugins/schema_renderer.rs](../../../crates/op-plugins/src/state_plugins/schema_renderer.rs) — line 151, `StatePlugin::apply_state`: Separate state-application behavior described above.
- [crates/op-plugins/src/state_plugins/schema_renderer.rs](../../../crates/op-plugins/src/state_plugins/schema_renderer.rs) — line 313, `inventory::submit! / PluginReg::new`: Compiled source registration, not live blob presence.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2356, `MutationEngine::dispatch_method_call_with_identity`: No dedicated plugin arm; canonical method dispatch reaches the generic request echo.

<a id="service"></a>

## `service`

**Purpose.** Describe runit service lifecycle methods and construct/install service definitions.

**Source status.** Three methods advertise init, run and shutdown using typed inputs and AckOutput. Their canonical dispatch has no corresponding service execution handler. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/service.rs:227 service_schema`

**Dispatch.** `generic_echo`

**Backing.** Public auto_generate_service constructs ServiceDef; install_service invokes ServiceDef::install. Private listing uses sv status and service directories; no callers of install_service were located.

**State effects.** Separate diff/apply are no-ops with empty checkpoints and constant verification. The real install helper is not reached by advertised init/run/shutdown or the state apply path.

**Test evidence.** No local plugin test module or targeted canonical service lifecycle test was found in the inspected plugin/bridge tests.

**Uncertainty.** Native helper existence must not be interpreted as service control availability. Subprocess/file observation paths remain inconsistent with the stated plugin policy. No runtime presence or deployment was checked.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/service.rs](../../../crates/op-plugins/src/state_plugins/service.rs) — line 227, `service_schema`: Plugin-owned contract and source description.
- [crates/op-plugins/src/state_plugins/service.rs](../../../crates/op-plugins/src/state_plugins/service.rs) — line 184, `StatePlugin::apply_state`: Separate state-application behavior described above.
- [crates/op-plugins/src/state_plugins/service.rs](../../../crates/op-plugins/src/state_plugins/service.rs) — line 299, `inventory::submit! / PluginReg::new`: Compiled source registration, not live blob presence.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2356, `MutationEngine::dispatch_method_call_with_identity`: No dedicated plugin arm; canonical method dispatch reaches the generic request echo.
- [crates/op-plugins/src/state_plugins/service.rs](../../../crates/op-plugins/src/state_plugins/service.rs) — line 102, `ServicePlugin::install_service`: A separate real installation helper exists outside canonical/apply routing.

<a id="sess-decl"></a>

## `sess_decl`

**Purpose.** Declare session records containing a session identifier and user association.

**Source status.** Two typed methods declare list_sessions and declare_session. Source explicitly says declarations are not durably stored and method dispatch is unwired. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/sessdecl.rs:113 sess_decl_schema`

**Dispatch.** `generic_echo`

**Backing.** The plugin is an empty struct with data contracts and no session database, login-service proxy or persistence client.

**State effects.** Diff/apply are no-ops, verification always returns true and checkpoint contains null. Declaring a session through the canonical method therefore cannot establish durable membership or authentication.

**Test evidence.** The local all_subids_are_valid test validates schema annotations; no declaration persistence or listing round-trip test was located in inspected test paths.

**Uncertainty.** This schema is separate from SID1 identity authority and native login sessions. Do not infer account or session creation from its advertised names. No runtime presence or deployment was checked.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/sessdecl.rs](../../../crates/op-plugins/src/state_plugins/sessdecl.rs) — line 113, `sess_decl_schema`: Plugin-owned contract and source description.
- [crates/op-plugins/src/state_plugins/sessdecl.rs](../../../crates/op-plugins/src/state_plugins/sessdecl.rs) — line 76, `StatePlugin::apply_state`: Separate state-application behavior described above.
- [crates/op-plugins/src/state_plugins/sessdecl.rs](../../../crates/op-plugins/src/state_plugins/sessdecl.rs) — line 211, `inventory::submit! / PluginReg::new`: Compiled source registration, not live blob presence.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2356, `MutationEngine::dispatch_method_call_with_identity`: No dedicated plugin arm; canonical method dispatch reaches the generic request echo.

<a id="shared-unix-socket"></a>

## `shared_unix_socket`

**Purpose.** Declares service registrations sharing the singleton container.sock endpoint.

**Source status.** Compiled schema offers create_unix_socket, list_registrations and get_config, but canonical Call only echoes arguments. The helper returns deterministic metadata without storing registrations.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/shared_unix_socket.rs:274 — shared_unix_socket_schema / SharedUnixSocketState`

**Dispatch.** `generic_echo`

**Backing.** Separate reconciliation owns a std::os::unix::net::UnixListener at /run/ghostbridge/container.sock and reads desired configuration from the plugin blob example.

**State effects.** create_unix_socket sorts/deduplicates ports and reports metadata success. apply_state calls reconcile, which ignores desired registrations and ensures one listener; if no locally owned listener exists, ensure_bound removes an already-existing socket path before binding. Verification checks only local ownership/path, not service routing.

**Test evidence.** Existing registration_is_deterministic and create_unix_socket_is_idempotent_metadata verify metadata behavior; inspected in this source-only pass. They do not exercise singleton binding or registration persistence.

**Uncertainty.** The destructive socket replacement can conflict with another owner of the shared endpoint. No registration store, accept loop or service forwarding is demonstrated by this plugin's methods.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/mod.rs](../../../crates/op-plugins/src/state_plugins/mod.rs) — line 76, `pub mod shared_unix_socket`: Compiled module membership; source registration does not prove live blob presence.
- [crates/op-plugins/src/state_plugins/shared_unix_socket.rs](../../../crates/op-plugins/src/state_plugins/shared_unix_socket.rs) — line 23, `SHARED_UNIX_SOCKET`: Singleton socket path.
- [crates/op-plugins/src/state_plugins/shared_unix_socket.rs](../../../crates/op-plugins/src/state_plugins/shared_unix_socket.rs) — line 101, `read_desired`: Reads blob example.
- [crates/op-plugins/src/state_plugins/shared_unix_socket.rs](../../../crates/op-plugins/src/state_plugins/shared_unix_socket.rs) — line 120, `create_unix_socket`: Metadata-only response without registration storage.
- [crates/op-plugins/src/state_plugins/shared_unix_socket.rs](../../../crates/op-plugins/src/state_plugins/shared_unix_socket.rs) — line 141, `ensure_bound / reconcile`: Existing path deletion and singleton binding; ignores registrations.
- [crates/op-plugins/src/state_plugins/shared_unix_socket.rs](../../../crates/op-plugins/src/state_plugins/shared_unix_socket.rs) — line 233, `apply_state / verify_state`: Reconcile and limited ownership verification.
- [crates/op-plugins/src/state_plugins/shared_unix_socket.rs](../../../crates/op-plugins/src/state_plugins/shared_unix_socket.rs) — line 359, `registration_is_deterministic`: Metadata tests, including idempotence at line 367.
- [crates/op-plugins/src/state_plugins/shared_unix_socket.rs](../../../crates/op-plugins/src/state_plugins/shared_unix_socket.rs) — line 379, `inventory::submit! / PluginReg::new`: Source inventory constructor.
- [crates/op-plugins/src/state_plugins/shared_unix_socket.rs](../../../crates/op-plugins/src/state_plugins/shared_unix_socket.rs) — line 274, `shared_unix_socket_schema`: PluginSchema source and declared contract.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2187, `dispatch_method_call_with_identity backend match`: No explicit arm for this plugin; default at line 2356 echoes arguments in success envelope.

<a id="snowball"></a>

## `snowball`

**Purpose.** Expose audit-chain queries and declare snapshot, retention and current-state operations.

**Source status.** Canonical dispatch implements query_events and verify_chain against the engine EventChain. Seven other declared methods explicitly fall through to argument echo. Snapshot and state persistence functionality exists separately on SnowballPlugin’s StatePlugin methods and StreamingSnowball.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/snowball_plugin.rs::snowball_schema`

**Dispatch.** `partial`

**Backing.** StreamingSnowball stores authoritative timing records plus vector/state projections and supports snapshots. The bridge initializes an optimized sink or timing-only fallback; if both fail it can keep the event chain in memory.

**State effects.** StatePlugin apply writes current_state; checkpoint creates a real snapshot; rollback resolves a snapshot path rather than proving a restored running state. Canonical unsupported methods still append audit events without executing their requested operations.

**Test evidence.** Integration tests explicitly distinguish real query results from echo, test filters/integrity, preserve the seven-method echo behavior and exercise restart persistence.

**Uncertainty.** Durability, btrfs topology and desired rollback semantics need runtime/design confirmation. Existing snapshots/helpers do not establish canonical method coverage or approval of a backup-style deployment hierarchy.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/snowball_plugin.rs](../../../crates/op-plugins/src/state_plugins/snowball_plugin.rs) — line 323, `SnowballPlugin::apply_state`: Separate state persistence.
- [crates/op-plugins/src/state_plugins/snowball_plugin.rs](../../../crates/op-plugins/src/state_plugins/snowball_plugin.rs) — line 361, `SnowballPlugin::create_checkpoint`: Separate real snapshot.
- [crates/op-plugins/src/state_plugins/snowball_plugin.rs](../../../crates/op-plugins/src/state_plugins/snowball_plugin.rs) — line 396, `snowball_schema`: Nine method contract.
- [crates/op-plugins/src/state_plugins/snowball_plugin.rs](../../../crates/op-plugins/src/state_plugins/snowball_plugin.rs) — line 545, `inventory::submit!`: Registration.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2347, `MutationEngine::dispatch_method_call_with_identity`: Two handlers and seven echo methods.
- [crates/op-grpc-bridge/tests/accountability_audit_trail.rs](../../../crates/op-grpc-bridge/tests/accountability_audit_trail.rs) — line 44, `query_events_returns_real_audit_data_not_echo`: Functional query test.
- [crates/op-grpc-bridge/tests/accountability_audit_trail.rs](../../../crates/op-grpc-bridge/tests/accountability_audit_trail.rs) — line 187, `existing_snowball_methods_still_echo`: Explicit missing coverage test.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 1043, `MutationEngine::init_audit_durability_with_cache_at`: RAM fallback remains.

<a id="software"></a>

## `software`

**Purpose.** Describe software inventory and advertised install, uninstall, update and information operations.

**Source status.** Four methods have typed inputs but generic AckOutput returns, including get_info. Their canonical method path has no package execution branch. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/software.rs:185 software_schema`

**Dispatch.** `generic_echo`

**Backing.** A private scan_dpkg parser reads /var/lib/dpkg/status directly, but no current caller of that helper was located. It is not a package-manager client.

**State effects.** Diff/apply are no-ops and checkpoints contain null; verification returns true. No installed package is changed or inventory refreshed by the advertised operations.

**Test evidence.** The local all_subids_are_valid test covers annotations. No install/remove, dpkg parsing or canonical get_info result test was located in the inspected sources.

**Uncertainty.** The dpkg-only helper is not a host-portable package inventory guarantee. Distinguish this skeleton from PackageKit’s separate real state-apply implementation. No runtime presence or deployment was checked.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/software.rs](../../../crates/op-plugins/src/state_plugins/software.rs) — line 185, `software_schema`: Plugin-owned contract and source description.
- [crates/op-plugins/src/state_plugins/software.rs](../../../crates/op-plugins/src/state_plugins/software.rs) — line 148, `StatePlugin::apply_state`: Separate state-application behavior described above.
- [crates/op-plugins/src/state_plugins/software.rs](../../../crates/op-plugins/src/state_plugins/software.rs) — line 304, `inventory::submit! / PluginReg::new`: Compiled source registration, not live blob presence.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2356, `MutationEngine::dispatch_method_call_with_identity`: No dedicated plugin arm; canonical method dispatch reaches the generic request echo.

<a id="tched-router"></a>

## `tched_router`

**Purpose.** 3tched-router framework/schema/plugin integration for model-agnostic routing and the control-plane chatbot, backed by the upstream zeroclaw executable. The plugin IS its schema; its sealed blob IS the runtime plugin.

**Source status.** Explicit method dispatch exists. The separate captured-schema tree violates the single-source rule, and selection setters update projected state without a traced upstream selection call.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/tched_router.rs::tched_router_schema_from_state and TchedRouterPlugin::schema; generated config method registration currently embeds schemas/zeroclaw inputs (OD-32 defect). schema_live delegates to schema and does not probe the executable.`

**Dispatch.** `explicit`

**Backing.** Chat uses TchedRouterRuntimeClient over loopback A2A message/send. CLI config methods invoke the upstream executable. Section getters/PatchConfig directly access its config file; catalog/route reads use projected or default TchedRouterState.

**State effects.** Common audited dispatch precedes execution. Set* updates bridge cache/projection and emits declared signals; PatchConfig validates and atomically replaces a config file. StatePlugin calculate_diff/apply_state itself is a no-op. These paths have different effects and durability guarantees.

**Test evidence.** list_models_uses_the_audited_tched_router_method_dispatcher checks domain filtering and event fields through the engine. Plugin tests check provider/model rejection, schema fields and capture size/version. Config-template tests check atomic write failure/symlink metadata. No live chat/provider selection proof in this baseline.

**Uncertainty.** Upstream provider/model selection helpers have no located production caller; Chat labels replies using projected/default selection. Private project/mode execution remains OD-46/47. Live executable/capture parity and deployment endpoints unverified. Tests retaining capture inputs do not legitimize the parallel schema tree.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/tched_router.rs](../../../crates/op-plugins/src/state_plugins/tched_router.rs) — line 1524, `TchedRouterPlugin::schema/schema_live`: Canonical schema entrypoint; live accessor returns declared schema.
- [crates/op-plugins/src/state_plugins/tched_router.rs](../../../crates/op-plugins/src/state_plugins/tched_router.rs) — line 1597, `tched_router_schema_from_state`: Typed plugin contract and generated config surface assembly.
- [crates/op-plugins/src/state_plugins/tched_router_config_surface.rs](../../../crates/op-plugins/src/state_plugins/tched_router_config_surface.rs) — line 39, `full_config_schema`: Separate schema file include violating owner rule.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2267, `dispatch_method_call_with_identity tched_router arm`: Chat uses runtime adapter; remaining calls use plugin dispatch.
- [crates/op-plugins/src/state_plugins/tched_router.rs](../../../crates/op-plugins/src/state_plugins/tched_router.rs) — line 2042, `dispatch_tched_router_method`: Catalog reads, selector mutations and configuration dispatch.
- [crates/op-plugins/src/state_plugins/tched_router.rs](../../../crates/op-plugins/src/state_plugins/tched_router.rs) — line 2129, `run_tched_router_config`: Upstream binary invocation.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2502, `persist_tched_router_mutation`: Selection writes cache/projection rather than invoking runtime selection helpers.
- [crates/op-grpc-bridge/src/tched_router_runtime.rs](../../../crates/op-grpc-bridge/src/tched_router_runtime.rs) — line 334, `TchedRouterRuntimeClient::chat`: Health check, comparison against state selection, A2A request, reply labels.
- [crates/op-plugins/src/state_plugins/tched_router_config_surface.rs](../../../crates/op-plugins/src/state_plugins/tched_router_config_surface.rs) — line 3387, `dispatch_config_method`: Section/full config reads and PatchConfig file mutation.
- [crates/op-grpc-bridge/tests/tched_router_integration.rs](../../../crates/op-grpc-bridge/tests/tched_router_integration.rs) — line 166, `list_models_uses_the_audited_tched_router_method_dispatcher`: Offline engine result and audit assertions, not upstream execution.
- [crates/op-plugins/src/bin/tched_router_config_surface.dispatch.inc](../../../crates/op-plugins/src/bin/tched_router_config_surface.dispatch.inc) — line 362, `patch_config_preserves_live_file_when_atomic_staging_fails`: Atomic write fixture behavior; generator template source.

<a id="unix-socket"></a>

## `unix_socket`

**Purpose.** Declares bind/listen/accept/close operations and shared service socket registration.

**Source status.** Compiled plugin with generic-echo canonical methods, but a separate legacy MutationEngine::mutate bind/createunixsocket branch calls create_unix_socket and updates projected registration state.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/unix_socket.rs:149 — unix_socket_schema_derived / UnixSocketState`

**Dispatch.** `generic_echo`

**Backing.** StatePlugin holds UnixListeners in a mutex map and reads desired endpoints from the blob example. It can create/remove sockets; an existing path is treated as another owner's shared endpoint.

**State effects.** Reconcile binds wanted endpoints and unbinds stale locally owned sockets. Canonical listen/accept/close are not connected to those helpers. Verification requires locally owned listener entries even when ensure_bound accepted an externally owned path. Rollback re-reconciles but discards errors.

**Test evidence.** Existing tests exercise declared socket bind/unbind, repeated binding and schema authority identifiers; inspected in this source-only pass.

**Uncertainty.** The alternate mutate branch is real but does not fix PluginV1.Call wiring. Socket metadata does not prove a service-forwarding loop, and ownership-sensitive verification can disagree with successful shared-path acceptance.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/mod.rs](../../../crates/op-plugins/src/state_plugins/mod.rs) — line 81, `pub mod unix_socket`: Compiled module membership; source registration does not prove live blob presence.
- [crates/op-plugins/src/state_plugins/unix_socket.rs](../../../crates/op-plugins/src/state_plugins/unix_socket.rs) — line 149, `unix_socket_schema_derived`: Declared bind/listen/accept/close methods.
- [crates/op-plugins/src/state_plugins/unix_socket.rs](../../../crates/op-plugins/src/state_plugins/unix_socket.rs) — line 273, `ensure_bound`: Actual bind or external-owner path acceptance.
- [crates/op-plugins/src/state_plugins/unix_socket.rs](../../../crates/op-plugins/src/state_plugins/unix_socket.rs) — line 321, `reconcile`: Create wanted/remove stale owned listeners.
- [crates/op-plugins/src/state_plugins/unix_socket.rs](../../../crates/op-plugins/src/state_plugins/unix_socket.rs) — line 436, `apply_state / verify_state / rollback`: Blob-example reconcile and ownership/error limitations.
- [crates/op-plugins/src/state_plugins/unix_socket.rs](../../../crates/op-plugins/src/state_plugins/unix_socket.rs) — line 537, `should_bind_and_unbind_declared_sockets`: Existing filesystem socket tests; idempotence follows.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 1877, `MutationEngine::mutate unix_socket arm`: Distinct bind helper and projected-state update.
- [crates/op-plugins/src/state_plugins/unix_socket.rs](../../../crates/op-plugins/src/state_plugins/unix_socket.rs) — line 582, `inventory::submit! / PluginReg::new`: Source inventory constructor.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2187, `dispatch_method_call_with_identity backend match`: No explicit arm for this plugin; default at line 2356 echoes arguments in success envelope.

<a id="users"></a>

## `users`

**Purpose.** Declare user-account configuration and advertised create, delete, modify and list operations.

**Source status.** Four methods expose typed account inputs but all return AckOutput, including list_users. No canonical user-management dispatch implementation is present. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/users.rs:157 users_schema`

**Dispatch.** `generic_echo`

**Backing.** UsersPlugin is an empty struct; declarations include UID/GID, groups, shell and desired presence. No account service, user database or native mutation helper exists in this module.

**State effects.** Diff/apply are empty and report success; verification is unconditional, checkpoint is null and rollback does nothing. These behaviors cannot establish account changes or restoration.

**Test evidence.** No local test module or targeted canonical account-operation test was found in the inspected plugin and bridge tests.

**Uncertainty.** Separate host account tooling may exist elsewhere, but neither its existence nor schema admission proves this plugin manages users. No runtime presence or deployment was checked.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/users.rs](../../../crates/op-plugins/src/state_plugins/users.rs) — line 157, `users_schema`: Plugin-owned contract and source description.
- [crates/op-plugins/src/state_plugins/users.rs](../../../crates/op-plugins/src/state_plugins/users.rs) — line 120, `StatePlugin::apply_state`: Separate state-application behavior described above.
- [crates/op-plugins/src/state_plugins/users.rs](../../../crates/op-plugins/src/state_plugins/users.rs) — line 238, `inventory::submit! / PluginReg::new`: Compiled source registration, not live blob presence.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2356, `MutationEngine::dispatch_method_call_with_identity`: No dedicated plugin arm; canonical method dispatch reaches the generic request echo.

<a id="web-ui"></a>

## `web_ui`

**Purpose.** Describe web UI identity, configurable presentation settings and page/event/component methods.

**Source status.** Three methods advertise render_page, handle_event and update_component with AckOutput. Existing identity/default tests retain obsolete rust-embed assumptions. After shared admission, the canonical method route has no plugin-specific handler and returns the generic audited request echo.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/web_ui.rs:555 web_ui_schema`

**Dispatch.** `generic_echo`

**Backing.** The plugin owns in-memory identity/tunable structs but does not serve the active frontend. Actual serving is op-web routes::create_router using external static files.

**State effects.** calculate_diff detects selected tunables. apply only formats 'Updated UI config' receipts without changing fields or persistence; checkpoint is null, verify is unconditional and rollback does nothing.

**Test evidence.** Six local tests cover defaults, immutable paths, property names and subids. One explicitly asserts the obsolete rust-embed driver, not current serving behavior.

**Uncertainty.** Owner-designated /srv/git/vercel-json-render-ui is authoritative. Embedded/always-available comments and tests do not establish runtime UI parity. No runtime presence or deployment was checked.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/web_ui.rs](../../../crates/op-plugins/src/state_plugins/web_ui.rs) — line 555, `web_ui_schema`: Plugin-owned contract and source description.
- [crates/op-plugins/src/state_plugins/web_ui.rs](../../../crates/op-plugins/src/state_plugins/web_ui.rs) — line 508, `StatePlugin::apply_state`: Separate state-application behavior described above.
- [crates/op-plugins/src/state_plugins/web_ui.rs](../../../crates/op-plugins/src/state_plugins/web_ui.rs) — line 717, `inventory::submit! / PluginReg::new`: Compiled source registration, not live blob presence.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2356, `MutationEngine::dispatch_method_call_with_identity`: No dedicated plugin arm; canonical method dispatch reaches the generic request echo.
- [crates/op-plugins/src/state_plugins/web_ui.rs](../../../crates/op-plugins/src/state_plugins/web_ui.rs) — line 659, `test_default_identity`: Test preserves obsolete rust-embed driver metadata.
- [crates/op-web/src/routes/mod.rs](../../../crates/op-web/src/routes/mod.rs) — line 291, `create_router`: Actual runtime static-file serving is separate.

<a id="wg-opdbus"></a>

## `wg_opdbus`

**Purpose.** Associates a fixed service persona and hypervisor identity role with the wireguard plugin's interface model.

**Source status.** Compiled identity/schema projection, not a second kernel WireGuard implementation. Schema declares create_interface and get_status and depends on wireguard; canonical calls have no dedicated backend.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/wg_opdbus.rs:281 — wg_opdbus_schema / WgOpdbusState`

**Dispatch.** `generic_echo`

**Backing.** The state embeds WireGuardInterface plus configuration path, identity role and route targets. Custom persona deserialization rejects changes to the canonical principal/persona/email/plugin fields.

**State effects.** Diff is empty and apply_state does nothing. verify_state checks the wireguard plugin reference and interface presence only, not a live device or identity session. Checkpoint uses default projected state and rollback is a no-op.

**Owner topology.** Owner-provided topology: WireGuard terminates at the decoy; there is no WireGuard on this server. Identity is transported by WARP. This identity/topology statement does not turn the schema projection into a server backend.

**Test evidence.** Existing canonical_service_persona_is_canonical_and_immutable and wg_opdbus_schema_projects_service_identity verify immutable identity fields and schema projection; inspected in this source-only pass.

**Uncertainty.** Immutable service metadata does not establish an authenticated runtime session or an installed interface. Device creation, status and route effects remain unavailable through this canonical method path.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/mod.rs](../../../crates/op-plugins/src/state_plugins/mod.rs) — line 84, `pub mod wg_opdbus`: Compiled module membership; source registration does not prove live blob presence.
- [crates/op-plugins/src/state_plugins/wg_opdbus.rs](../../../crates/op-plugins/src/state_plugins/wg_opdbus.rs) — line 61, `Deserialize for AnnaScribeServicePersona`: Enforces canonical service identity fields.
- [crates/op-plugins/src/state_plugins/wg_opdbus.rs](../../../crates/op-plugins/src/state_plugins/wg_opdbus.rs) — line 103, `WgOpdbusState`: Identity and shared WireGuard interface model.
- [crates/op-plugins/src/state_plugins/wg_opdbus.rs](../../../crates/op-plugins/src/state_plugins/wg_opdbus.rs) — line 228, `StatePlugin for WgOpdbusPlugin`: Inert apply and limited metadata verification.
- [crates/op-plugins/src/state_plugins/wg_opdbus.rs](../../../crates/op-plugins/src/state_plugins/wg_opdbus.rs) — line 281, `wg_opdbus_schema`: Two methods and wireguard dependency.
- [crates/op-plugins/src/state_plugins/wg_opdbus.rs](../../../crates/op-plugins/src/state_plugins/wg_opdbus.rs) — line 344, `canonical_service_persona_is_canonical_and_immutable`: Immutable identity test; schema projection test follows.
- [crates/op-plugins/src/state_plugins/wg_opdbus.rs](../../../crates/op-plugins/src/state_plugins/wg_opdbus.rs) — line 336, `inventory::submit! / PluginReg::new`: Source inventory constructor.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2187, `dispatch_method_call_with_identity backend match`: No explicit arm for this plugin; default at line 2356 echoes arguments in success envelope.

<a id="wgcf"></a>

## `wgcf`

**Purpose.** Declares WARP account registration, configuration generation, endpoint/key changes and application.

**Source status.** Compiled scaffold with eight typed method declarations returning AckOutput. current_state provides fixed software/configuration/tool metadata rather than probing a WARP account, interface or upstream executable.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/wgcf.rs:261 — wgcf_schema / WgcfState`

**Dispatch.** `generic_echo`

**Backing.** No wgcf binary invocation, WARP API client or WireGuard configuration application is called by this plugin's canonical methods or StatePlugin implementation.

**State effects.** Default configuration is enabled with a fwmark, port and /etc/wireguard/wgcf.conf path, but these defaults do not install anything. Diff is empty, apply reports success without effects and verify always returns true. Checkpoint and rollback explicitly return unsupported errors.

**Owner topology.** Owner-provided topology: WireGuard terminates at the decoy; there is no WireGuard on this server. Identity is transported by WARP. Do not infer WARP transport failure from this scaffold’s missing account/config handlers.

**Test evidence.** Existing all_subids_are_valid checks schema authority identifiers; inspected in this source-only pass. No operational WARP behavior was exercised.

**Uncertainty.** Method names such as register, generate_config, apply_config and refresh describe a proposed surface, not implemented account or tunnel management. Retired Netmaker assumptions elsewhere must not be used to infer its backend.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/mod.rs](../../../crates/op-plugins/src/state_plugins/mod.rs) — line 85, `pub mod wgcf`: Compiled module membership; source registration does not prove live blob presence.
- [crates/op-plugins/src/state_plugins/wgcf.rs](../../../crates/op-plugins/src/state_plugins/wgcf.rs) — line 14, `WgcfConfig`: Declared defaults and configuration path.
- [crates/op-plugins/src/state_plugins/wgcf.rs](../../../crates/op-plugins/src/state_plugins/wgcf.rs) — line 115, `WgcfPlugin::current_state`: Fixed metadata rather than probing.
- [crates/op-plugins/src/state_plugins/wgcf.rs](../../../crates/op-plugins/src/state_plugins/wgcf.rs) — line 171, `StatePlugin for WgcfPlugin`: Inert reconciliation and unsupported checkpoint/rollback.
- [crates/op-plugins/src/state_plugins/wgcf.rs](../../../crates/op-plugins/src/state_plugins/wgcf.rs) — line 261, `wgcf_schema`: Eight method declarations.
- [crates/op-plugins/src/state_plugins/wgcf.rs](../../../crates/op-plugins/src/state_plugins/wgcf.rs) — line 455, `all_subids_are_valid`: Identifier validation test only.
- [crates/op-plugins/src/state_plugins/wgcf.rs](../../../crates/op-plugins/src/state_plugins/wgcf.rs) — line 470, `inventory::submit! / PluginReg::new`: Source inventory constructor.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2187, `dispatch_method_call_with_identity backend match`: No explicit arm for this plugin; default at line 2356 echoes arguments in success envelope.

<a id="wireguard"></a>

## `wireguard`

**Purpose.** Defines shared WireGuard device/peer state and device, peer and configuration operations.

**Source status.** Compiled schema-only plugin. It declares set_device, get_device, add_peer, remove_peer, list_peers and set_config; read methods have typed outputs but canonical Call echoes inputs.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/wireguard.rs:280 — wireguard_schema / WireGuardState`

**Dispatch.** `generic_echo`

**Backing.** WireGuardPlugin is stateless and invokes no kernel WireGuard API, daemon, configuration-file writer or interface-management client. Other plugins embedding its types do not supply this missing dispatch.

**State effects.** Diff is empty; apply reports success without changes. Verification always returns true, checkpoint contains null and rollback does nothing despite advertised rollback support. No device, peer or cryptographic key is installed by these paths.

**Owner topology.** Owner-provided topology: WireGuard terminates at the decoy; there is no WireGuard on this server. Identity is transported by WARP. This topology statement is owner-provided and was not live-validated.

**Test evidence.** No local test block found in the inspected plugin module; the full verification gate is recorded at the end of this baseline.

**Uncertainty.** The schema is source inventory only: WireGuard terminates at the decoy and there is no WireGuard on this server according to owner-provided topology; identity is transported by WARP. Live transport, peer connectivity and interface state remain unverified.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/mod.rs](../../../crates/op-plugins/src/state_plugins/mod.rs) — line 86, `pub mod wireguard`: Compiled module membership; source registration does not prove live blob presence.
- [crates/op-plugins/src/state_plugins/wireguard.rs](../../../crates/op-plugins/src/state_plugins/wireguard.rs) — line 199, `WireGuardPlugin`: Stateless implementation.
- [crates/op-plugins/src/state_plugins/wireguard.rs](../../../crates/op-plugins/src/state_plugins/wireguard.rs) — line 227, `StatePlugin for WireGuardPlugin`: No-op diff/apply/rollback and unconditional verification.
- [crates/op-plugins/src/state_plugins/wireguard.rs](../../../crates/op-plugins/src/state_plugins/wireguard.rs) — line 266, `capabilities`: Rollback capability despite no-op rollback.
- [crates/op-plugins/src/state_plugins/wireguard.rs](../../../crates/op-plugins/src/state_plugins/wireguard.rs) — line 280, `wireguard_schema`: Six method declarations and typed read outputs.
- [crates/op-plugins/src/state_plugins/wireguard.rs](../../../crates/op-plugins/src/state_plugins/wireguard.rs) — line 385, `inventory::submit! / PluginReg::new`: Source inventory constructor.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2187, `dispatch_method_call_with_identity backend match`: No explicit arm for this plugin; default at line 2356 echoes arguments in success envelope.

<a id="workflows"></a>

## `workflows`

**Purpose.** Advertise workflow templates and append execution requests to a journal for later processing.

**Source status.** All five method names reach an explicit dispatcher. query_workflows returns static templates; start_workflow queues known IDs; pause/resume/cancel deliberately return errors because no local executor owns transitions. 

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/workflows_plugin.rs:182 workflows_schema`

**Dispatch.** `partial`

**Backing.** A process mutex and append-only JSONL journal back admission. The default path is /var/lib/op-dbus/workflows/run-queue.jsonl; cognitive workflow_query/workflow_run reuse these handlers.

**State effects.** start creates an execution UUID, appends parameters with restrictive new-file permissions, calls sync_data and returns queued. It does not execute steps. Separate StatePlugin diff/apply remain no-ops.

**Test evidence.** Local schema/subid checks exist; no journal-consumer execution or lifecycle-completion test was found in the inspected plugin/bridge test sources.

**Uncertainty.** Acceptance establishes that journal append and sync_data completed. Containing-directory crash durability, executor consumption and private parameter retention require separate evidence. No runtime presence or deployment was checked.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/workflows_plugin.rs](../../../crates/op-plugins/src/state_plugins/workflows_plugin.rs) — line 182, `workflows_schema`: Plugin-owned contract and source description.
- [crates/op-plugins/src/state_plugins/workflows_plugin.rs](../../../crates/op-plugins/src/state_plugins/workflows_plugin.rs) — line 148, `StatePlugin::apply_state`: Separate state-application behavior described above.
- [crates/op-plugins/src/state_plugins/workflows_plugin.rs](../../../crates/op-plugins/src/state_plugins/workflows_plugin.rs) — line 416, `inventory::submit! / PluginReg::new`: Compiled source registration, not live blob presence.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2202, `workflows dispatch arm`: Canonical PluginV1 method dispatch explicitly routes workflows.
- [crates/op-plugins/src/state_plugins/workflows_plugin.rs](../../../crates/op-plugins/src/state_plugins/workflows_plugin.rs) — line 324, `dispatch_workflows_method`: Two implemented handlers and explicit lifecycle rejection.
- [crates/op-plugins/src/state_plugins/workflows_plugin.rs](../../../crates/op-plugins/src/state_plugins/workflows_plugin.rs) — line 302, `append_workflow_request`: Append, mode 0600 on create and file sync_data before acceptance.

<a id="xray"></a>

## `xray`

**Purpose.** Projects Xray configuration, statistics and selected lifecycle controls.

**Source status.** Compiled partial dispatcher: get_stats calls Xray StatsService; restart sends SIGHUP. Six user/inbound/trace methods explicitly error. The comment claiming only statistics works is stale.

**Schema source (reference only).** `crates/op-plugins/src/state_plugins/xray.rs:291 — xray_schema / XrayState; crates/op-plugins/src/state_plugins/xray_config_types.rs:1 — generated Rust configuration types`

**Dispatch.** `partial`

**Backing.** Statistics use the commander Unix socket through op_xray_daemon. Restart scans /proc for every xray process and signals each. Generated configuration structs cite an upstream extraction and temporary generator; this is source provenance, not current live-version evidence.

**State effects.** StatePlugin apply ignores diff and signals discovered processes according to self.config.enabled: SIGHUP when enabled, SIGTERM otherwise. It does not write configuration. Defaults use the mandated /etc/xray/xray_config.json; verification is unconditional and checkpoint/rollback unsupported.

**Test evidence.** Local all_subids_are_valid tests identifiers; an existing get_stats example is a manual runtime probe, not executed in this source-only pass.

**Uncertainty.** Direct host process discovery/signaling conflicts with scoped D-Bus/container ownership intent. Canonical restart is reload behavior, and declared configuration does not establish live configuration or successful reload.

**Source evidence.**

- [crates/op-plugins/src/state_plugins/mod.rs](../../../crates/op-plugins/src/state_plugins/mod.rs) — line 88, `pub mod xray`: Compiled module membership; source registration does not prove live blob presence.
- [crates/op-plugins/src/state_plugins/xray.rs](../../../crates/op-plugins/src/state_plugins/xray.rs) — line 55, `XrayConfig::default`: Mandated default config path.
- [crates/op-plugins/src/state_plugins/xray.rs](../../../crates/op-plugins/src/state_plugins/xray.rs) — line 235, `apply_state`: Ignores diff and signals discovered processes; no config writing.
- [crates/op-plugins/src/state_plugins/xray_config_types.rs](../../../crates/op-plugins/src/state_plugins/xray_config_types.rs) — line 1, `Generated configuration provenance`: Temporary extraction/codegen provenance, not a checked current live binary.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 3114, `dispatch_xray_method`: Stats/reload implemented, six methods fail explicitly.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 3172, `find_xray_pids`: Unscoped /proc process discovery.
- [crates/op-plugins/src/state_plugins/xray.rs](../../../crates/op-plugins/src/state_plugins/xray.rs) — line 437, `all_subids_are_valid`: Local identifier test.
- [crates/op-grpc-bridge/examples/test_xray_get_stats.rs](../../../crates/op-grpc-bridge/examples/test_xray_get_stats.rs) — line 6, `main`: Manual runtime example; not executed.
- [crates/op-plugins/src/state_plugins/xray.rs](../../../crates/op-plugins/src/state_plugins/xray.rs) — line 519, `inventory::submit! / PluginReg::new`: Source inventory constructor.
- [crates/op-plugins/src/state_plugins/xray.rs](../../../crates/op-plugins/src/state_plugins/xray.rs) — line 291, `xray_schema`: PluginSchema source and declared contract.
- [crates/op-grpc-bridge/src/mutation_engine.rs](../../../crates/op-grpc-bridge/src/mutation_engine.rs) — line 2187, `dispatch_method_call_with_identity backend match`: Canonical backend routing for this plugin.

## Final verification

- `sh scripts/blitzy-check.sh verify`: completed with exit 0.
- `/var/tmp/odbus-plugin-baseline-verify.log`: **1126 passed, 0 failed, 8 ignored**.
- mdBook tools were absent in the verification environment.

This baseline is limited to source inspection and does not certify live health, sealed blob presence, runtime reachability, deployment state, or credential usability.
