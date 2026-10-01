//! Pure projection of manifest-pinned contracts and committed public SHM state.
//!
//! This module performs no I/O. State must already have passed both the private
//! state boundary and the sealed schema's public-state policy. Even then, only
//! explicitly selected infrastructure fields are retained here.
use op_cozo_store::{
    GraphAssertion, GraphEdge, GraphEvidence, GraphNode, GraphProvenance, GraphSnapshot,
};
use op_state_store::PluginSchema;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// The collector owns catalog pinning, snapshot consistency and public filtering.
#[derive(Debug, Clone)]
pub struct GraphPluginProjection {
    pub schema: PluginSchema,
    pub schema_hash: String,
    pub state: Option<Value>,
}

const MAX_DOMAIN_RECORDS: usize = 4096;

pub fn project_system_graph(
    plugins: &BTreeMap<String, GraphPluginProjection>,
    mut provenance: GraphProvenance,
) -> GraphSnapshot {
    let mut graph = Projection::default();
    for (plugin, input) in plugins {
        let plugin_id = id("plugin", &[plugin]);
        graph.node(&plugin_id, "plugin", plugin, json!({
            "schema_hash": input.schema_hash,
            "schema_version": input.schema.version,
            "category": input.schema.category,
            "state_coverage": if input.state.is_some() { "public_projection" } else { "unavailable" },
            "authority": "sealed_catalog",
        }));
        if input.state.is_none() {
            graph.limit(format!(
                "{plugin}: committed public state unavailable; no absence inferred"
            ));
        }
    }
    for (plugin, input) in plugins {
        let plugin_id = id("plugin", &[plugin]);
        let source = format!("sealed:{plugin}:{}", input.schema_hash);
        for dependency in &input.schema.dependencies {
            if plugins.contains_key(dependency) {
                graph.edge(
                    &plugin_id,
                    "depends_on",
                    &id("plugin", &[dependency]),
                    declared(&source, "schema.dependencies"),
                );
            } else {
                graph.limit(format!(
                    "{plugin}: declared dependency {dependency} unresolved in pinned catalog"
                ));
            }
        }
        let methods: BTreeMap<_, _> = input.schema.methods.iter().collect();
        for (key, method) in methods {
            let method_id = id("method", &[plugin, key]);
            graph.node(
                &method_id,
                "method",
                key,
                json!({
                    "transport_name": method.name,
                    "side_effect": method.side_effect,
                    "idempotent": method.idempotent,
                    "availability": "declared_only",
                }),
            );
            let location = format!("schema.methods.{key}");
            graph.edge(
                &plugin_id,
                "declares_method",
                &method_id,
                declared(&source, &location),
            );
            if let Some(capability) = &method.required_capability {
                let capability_id = id("capability", &[capability]);
                graph.node(
                    &capability_id,
                    "capability",
                    capability,
                    json!({"grants": "not_projected"}),
                );
                graph.edge(
                    &method_id,
                    "requires_capability",
                    &capability_id,
                    declared(&source, &format!("{location}.required_capability")),
                );
            }
            if !method.subid.is_empty() {
                let subid = id("operational_taxonomy", &[&method.subid]);
                graph.node(&subid, "operational_taxonomy", &method.subid, json!({}));
                graph.edge(
                    &method_id,
                    "classified_by",
                    &subid,
                    declared(&source, &format!("{location}.subid")),
                );
            }
        }
        if let Some(state) = input.state.as_ref().filter(|state| state.is_object()) {
            let state_source = format!("public_shm:{plugin}:{}", input.schema_hash);
            match plugin.as_str() {
                "incus" => graph.incus(state, &plugin_id, &state_source, &input.schema),
                "btrfs" => graph.btrfs(state, &plugin_id, &state_source),
                "qdrant" => graph.qdrant(state, &plugin_id, &state_source),
                "mail_server" => graph.mail(state, &plugin_id, &state_source),
                "embedding_model" => graph.embedding(state, &plugin_id, &state_source),
                "memory" => {
                    graph.extend(&plugin_id, select(state, &["backend", "isolation"]));
                    graph.limit(
                        "memory: private namespaces, entries and content deliberately excluded"
                            .into(),
                    );
                }
                "cognitive_mcp" => graph.extend(
                    &plugin_id,
                    select(state, &["bridge_owned", "execution_model"]),
                ),
                "json_render" => graph.ui(state, &plugin_id, &state_source),
                _ => {}
            }
        }
    }
    graph.source_declarations(plugins);
    graph.resolve_container_references();
    provenance.coverage.extend([
        "sealed_plugin_methods_capabilities_taxonomy".into(),
        "allowlisted_public_infrastructure_state".into(),
        "source_declared_subsystems_separate_from_observations".into(),
    ]);
    provenance.limitations.extend(graph.limitations);
    provenance.coverage.sort();
    provenance.coverage.dedup();
    provenance.limitations.sort();
    provenance.limitations.dedup();
    // The store bounds receipt metadata independently of node/edge count. A
    // large catalog with withheld state must remain readable as an explicitly
    // incomplete inventory, rather than fail just because its warnings grow.
    if provenance.limitations.len() > 256 {
        let omitted = provenance.limitations.len() - 255;
        provenance.limitations.truncate(255);
        provenance.limitations.push(format!(
            "{omitted} additional coverage limitations omitted by the metadata bound; this inventory is incomplete"
        ));
    }
    GraphSnapshot {
        format_version: 1,
        namespace: "runtime:public".into(),
        provenance,
        nodes: graph.nodes.into_values().collect(),
        edges: graph.edges.into_values().collect(),
    }
}

#[derive(Default)]
struct Projection {
    nodes: BTreeMap<String, GraphNode>,
    edges: BTreeMap<(String, String, String), GraphEdge>,
    limitations: BTreeSet<String>,
    container_references: Vec<(String, String, GraphEvidence)>,
}

impl Projection {
    fn limit(&mut self, limitation: String) {
        self.limitations.insert(limitation);
    }

    fn node(&mut self, id: &str, kind: &str, label: &str, attributes: Value) {
        self.nodes.entry(id.into()).or_insert_with(|| GraphNode {
            id: id.into(),
            kind: kind.into(),
            label: label.into(),
            attributes: object(attributes),
        });
    }

    fn extend(&mut self, id: &str, attributes: BTreeMap<String, Value>) {
        if let Some(node) = self.nodes.get_mut(id) {
            node.attributes.extend(attributes);
        }
    }

    fn edge(&mut self, src: &str, relation: &str, dst: &str, evidence: GraphEvidence) {
        let edge = self
            .edges
            .entry((src.into(), relation.into(), dst.into()))
            .or_insert_with(|| GraphEdge {
                src: src.into(),
                relation: relation.into(),
                dst: dst.into(),
                evidence: Vec::new(),
                attributes: BTreeMap::new(),
            });
        if !edge.evidence.iter().any(|existing| {
            existing.source == evidence.source && existing.locator == evidence.locator
        }) {
            edge.evidence.push(evidence);
            edge.evidence
                .sort_by(|a, b| (&a.source, &a.locator).cmp(&(&b.source, &b.locator)));
        }
    }

    fn resource(
        &mut self,
        owner: &str,
        kind: &str,
        name: &str,
        attributes: Value,
        source: &str,
        locator: &str,
    ) -> String {
        let node = id(kind, &[owner, name]);
        self.node(&node, kind, name, attributes);
        self.edge(owner, "exposes", &node, observed(source, locator));
        node
    }

    // Select named records deterministically, withholding conflicting duplicates.
    fn records<'a>(
        &mut self,
        value: &'a Value,
        key: &str,
        source: &str,
    ) -> BTreeMap<String, &'a Value> {
        let Some(array) = value.as_array() else {
            self.limit(format!(
                "{source}: inventory field unavailable or malformed"
            ));
            return BTreeMap::new();
        };
        if array.len() > MAX_DOMAIN_RECORDS {
            self.limit(format!(
                "{source}: inventory exceeds projection bound; withheld"
            ));
            return BTreeMap::new();
        }
        let mut records = BTreeMap::new();
        let mut duplicate = BTreeSet::new();
        for record in array {
            if let Some(name) = text(record, key).filter(|name| safe_name(name)) {
                if records.insert(name.to_string(), record).is_some() {
                    duplicate.insert(name.to_string());
                }
            } else {
                self.limit(format!("{source}: record has missing or unsafe {key}"));
            }
        }
        for name in duplicate {
            records.remove(&name);
            self.limit(format!(
                "{source}: duplicate {key} {name}; relationships withheld"
            ));
        }
        records
    }

    fn incus(&mut self, state: &Value, plugin: &str, source: &str, schema: &PluginSchema) {
        let names_withheld = !schema.public_state.is_empty()
            && !schema.public_state.paths.iter().any(|path| {
                matches!(
                    path.as_str(),
                    "/instances" | "/instances/*" | "/instances/*/name"
                )
            });
        if names_withheld {
            self.limit(format!("{source}: instance names and relationships withheld by sealed public policy; aggregate counts only"));
            let Some(instances) = state["instances"].as_array() else {
                self.limit(format!(
                    "{source}: aggregate instance inventory unavailable or malformed"
                ));
                return;
            };
            if instances.len() > MAX_DOMAIN_RECORDS {
                self.limit(format!(
                    "{source}: aggregate instance inventory exceeds projection bound; withheld"
                ));
                return;
            }
            // Fixed categories avoid unbounded or attacker-authored map keys.
            // Missing/unrecognized approved values remain explicit, not inferred.
            let mut statuses = BTreeMap::<&str, usize>::new();
            let mut types = BTreeMap::<&str, usize>::new();
            for instance in instances {
                let status = match text(instance, "status") {
                    Some(
                        value @ ("Running" | "Stopped" | "Frozen" | "Error" | "Starting"
                        | "Stopping" | "Freezing" | "Thawed" | "Aborting"),
                    ) => value,
                    _ => "other_or_missing",
                };
                let kind = match text(instance, "type") {
                    Some(value @ ("container" | "virtual-machine")) => value,
                    _ => "other_or_missing",
                };
                *statuses.entry(status).or_default() += 1;
                *types.entry(kind).or_default() += 1;
            }
            self.extend(
                plugin,
                BTreeMap::from([(
                    "instance_summary".into(),
                    json!({
                        "authority": "public_shm", "total": instances.len(),
                        "status_counts": statuses, "type_counts": types,
                        "named_inventory": "withheld_by_public_policy"
                    }),
                )]),
            );
            return;
        }
        let instances = self.records(&state["instances"], "name", source);
        for (name, instance) in instances {
            let container = id("container", &[&name]);
            self.node(
                &container,
                "container",
                &name,
                json!({"authority": "public_shm"}),
            );
            self.extend(
                &container,
                select(
                    instance,
                    &["status", "type", "architecture", "ephemeral", "stateful"],
                ),
            );
            self.edge(
                plugin,
                "manages",
                &container,
                observed(source, &format!("instances[name={name}]")),
            );
            // The expanded inventory is effective; never union stale local devices.
            let (devices, field) = if instance
                .get("expanded_devices")
                .and_then(Value::as_array)
                .is_some_and(|items| !items.is_empty())
            {
                (&instance["expanded_devices"], "expanded_devices")
            } else {
                (&instance["devices"], "devices")
            };
            if devices.is_null() {
                self.limit(format!("incus: {name} device inventory unavailable"));
                continue;
            }
            let devices = self.records(devices, "name", &format!("incus:{name}:{field}"));
            for (device_name, record) in devices {
                let device = &record["device"];
                let Some(kind) = text(device, "type") else {
                    continue;
                };
                let location = format!("instances[name={name}].{field}[name={device_name}].device");
                let device_id = id("container_device", &[&name, &device_name]);
                self.node(
                    &device_id,
                    "container_device",
                    &device_name,
                    json!({"type":kind, "inventory":field}),
                );
                self.edge(
                    &container,
                    "has_device",
                    &device_id,
                    observed(source, &location),
                );
                match kind {
                    "disk" => {
                        self.extend(
                            &device_id,
                            select(device, &["readonly", "recursive", "shift"]),
                        );
                        if let Some(path) = text(device, "path").filter(|path| safe_path(path)) {
                            self.extend(&device_id, object(json!({"mount_path":path})));
                        }
                        if let Some(path) = text(device, "source").filter(|path| safe_path(path)) {
                            let path_id = id("filesystem_path", &[path]);
                            self.node(
                                &path_id,
                                "filesystem_path",
                                path,
                                json!({"filesystem_type":"unknown"}),
                            );
                            self.edge(
                                &device_id,
                                "mounts_source",
                                &path_id,
                                observed(source, &format!("{location}.source")),
                            );
                        }
                        if let Some(pool) = text(device, "pool").filter(|name| safe_name(name)) {
                            let pool_id = id("storage_pool", &[pool]);
                            self.node(&pool_id, "storage_pool", pool, json!({"backend":"unknown"}));
                            self.edge(
                                &device_id,
                                "uses_pool",
                                &pool_id,
                                observed(source, &format!("{location}.pool")),
                            );
                        }
                    }
                    "proxy" => {
                        for field in ["listen", "connect"] {
                            if let Some(endpoint) =
                                text(device, field).filter(|endpoint| safe_endpoint(endpoint))
                            {
                                self.resource(
                                    &device_id,
                                    "endpoint",
                                    endpoint,
                                    json!({"role":field}),
                                    source,
                                    &format!("{location}.{field}"),
                                );
                            }
                        }
                    }
                    "nic" => self.extend(
                        &device_id,
                        select(device, &["nictype", "network", "parent", "connected"]),
                    ),
                    _ => {}
                }
            }
        }
    }

    fn btrfs(&mut self, state: &Value, plugin: &str, source: &str) {
        // Never derive inventories from operation args or withdrawn global caps.
        let Some(filesystems) = state.get("filesystems").and_then(Value::as_object) else {
            self.limit(
                "btrfs: filesystem/subvolume inventory unavailable in committed public state"
                    .into(),
            );
            return;
        };
        if filesystems.len() > MAX_DOMAIN_RECORDS {
            self.limit("btrfs: filesystem inventory exceeds projection bound; withheld".into());
            return;
        }
        let filesystems: BTreeMap<_, _> = filesystems.iter().collect();
        for (uuid, filesystem) in filesystems {
            if !safe_name(uuid) {
                continue;
            }
            let fs_id = id("btrfs_filesystem", &[uuid]);
            self.node(&fs_id, "btrfs_filesystem", uuid, json!({"uuid":uuid}));
            self.edge(
                plugin,
                "manages",
                &fs_id,
                observed(source, &format!("filesystems.{uuid}")),
            );
            if let Some(path) = text(filesystem, "path").filter(|path| safe_path(path)) {
                let path_id = id("filesystem_path", &[path]);
                self.node(
                    &path_id,
                    "filesystem_path",
                    path,
                    json!({"filesystem_type":"btrfs"}),
                );
                self.edge(
                    &fs_id,
                    "observed_at_path",
                    &path_id,
                    observed(source, &format!("filesystems.{uuid}.path")),
                );
            }
            let Some(subvolumes) = filesystem.get("subvolumes").and_then(Value::as_array) else {
                self.limit(format!("btrfs: {uuid} subvolume inventory unavailable"));
                continue;
            };
            if subvolumes.len() > MAX_DOMAIN_RECORDS {
                self.limit(format!("btrfs: {uuid} subvolume inventory exceeds bound"));
                continue;
            }
            let mut records = BTreeMap::new();
            let mut duplicate = BTreeSet::new();
            for subvolume in subvolumes {
                if let Some(number) = subvolume.get("id").and_then(Value::as_u64) {
                    if records.insert(number, subvolume).is_some() {
                        duplicate.insert(number);
                    }
                }
            }
            for number in duplicate {
                records.remove(&number);
                self.limit(format!(
                    "btrfs: {uuid} duplicate subvolume {number}; withheld"
                ));
            }
            for (number, subvolume) in &records {
                let sub_id = id("btrfs_subvolume", &[uuid, &number.to_string()]);
                self.node(
                    &sub_id,
                    "btrfs_subvolume",
                    &format!("{uuid}/{number}"),
                    json!({"filesystem_uuid":uuid}),
                );
                self.extend(
                    &sub_id,
                    select(
                        subvolume,
                        &[
                            "id",
                            "parent_id",
                            "generation",
                            "uuid",
                            "parent_uuid",
                            "received_uuid",
                            "readonly",
                        ],
                    ),
                );
                if let Some(name) = text(subvolume, "name").filter(|name| safe_path(name)) {
                    self.extend(&sub_id, object(json!({"name":name})));
                }
                self.edge(
                    &fs_id,
                    "contains_subvolume",
                    &sub_id,
                    observed(
                        source,
                        &format!("filesystems.{uuid}.subvolumes[id={number}]"),
                    ),
                );
            }
            for (number, subvolume) in &records {
                if parent_cycle(*number, &records) {
                    self.limit(format!("btrfs: {uuid}/{number} contradictory cyclic parent hierarchy; edge withheld"));
                    continue;
                }
                if let Some(parent) = subvolume
                    .get("parent_id")
                    .and_then(Value::as_u64)
                    .filter(|parent| *parent != 0 && *parent != *number)
                {
                    if records.contains_key(&parent) {
                        self.edge(
                            &id("btrfs_subvolume", &[uuid, &parent.to_string()]),
                            "contains_subvolume",
                            &id("btrfs_subvolume", &[uuid, &number.to_string()]),
                            observed(
                                source,
                                &format!("filesystems.{uuid}.subvolumes[id={number}].parent_id"),
                            ),
                        );
                    } else if parent != 5 {
                        self.limit(format!(
                            "btrfs: {uuid}/{number} parent {parent} unavailable"
                        ));
                    }
                }
            }
        }
    }

    fn qdrant(&mut self, state: &Value, plugin: &str, source: &str) {
        self.extend(plugin, select(state, &["version", "commit"]));
        for key in ["http_endpoint", "grpc_endpoint"] {
            if let Some(endpoint) = text(state, key).filter(|endpoint| safe_endpoint(endpoint)) {
                self.resource(
                    plugin,
                    "endpoint",
                    endpoint,
                    json!({"protocol":key}),
                    source,
                    key,
                );
            }
        }
        for (name, collection) in self.records(&state["collections"], "name", source) {
            let collection_id = id("qdrant_collection", &[&name]);
            self.node(&collection_id, "qdrant_collection", &name, json!({}));
            self.extend(
                &collection_id,
                select(
                    collection,
                    &[
                        "status",
                        "optimizer_status",
                        "points_count",
                        "indexed_vectors_count",
                        "segments_count",
                    ],
                ),
            );
            // Payload schemas/metadata/points may contain private field names and content.
            let vectors = collection.pointer("/config/params/vectors");
            if let Some(vectors) = vectors {
                if vectors.get("size").is_some() {
                    self.extend(
                        &collection_id,
                        select(vectors, &["size", "distance", "on_disk", "datatype"]),
                    );
                } else if let Some(named) = vectors.as_object() {
                    let mut summaries = BTreeMap::new();
                    for (name, params) in named {
                        if safe_name(name) {
                            summaries.insert(
                                name.clone(),
                                select(params, &["size", "distance", "on_disk", "datatype"]),
                            );
                        }
                    }
                    self.extend(&collection_id, object(json!({"named_vectors":summaries})));
                }
            } else {
                self.limit(format!("qdrant: {name} vector configuration unavailable"));
            }
            if let Some(hnsw) = collection.pointer("/config/hnsw_config") {
                self.extend(&collection_id,object(json!({"hnsw":select(hnsw,&["m","ef_construct","full_scan_threshold","max_indexing_threads","on_disk"])})));
            }
            self.edge(
                plugin,
                "contains_collection",
                &collection_id,
                observed(source, &format!("collections[name={name}]")),
            );
        }
    }

    fn mail(&mut self, state: &Value, plugin: &str, source: &str) {
        // Unknown is the provider's bootstrap declaration, not a native observation.
        let configured_only = !matches!(
            text(state, "container_status"),
            Some("Running" | "Stopped" | "Frozen")
        );
        let declaration_source = format!("declared_configuration:{source}");
        let source = if configured_only {
            declaration_source.as_str()
        } else {
            source
        };
        self.extend(plugin, select(state, &["container_status"]));
        if configured_only {
            self.limit("mail_server: native status unavailable; configuration references are declared only".into());
        } else {
            self.extend(plugin, select(state, &["healthy"]));
        }
        if let Some(container) = text(state, "container_name").filter(|name| safe_name(name)) {
            self.container_references.push((
                plugin.into(),
                container.into(),
                observed(source, "container_name"),
            ));
        }
        let mut domains = BTreeSet::new();
        if let Some(domain) = text(state, "domain").filter(|domain| safe_domain(domain)) {
            domains.insert(domain.to_string());
        }
        if let Some(list) = state
            .pointer("/administration/domains")
            .and_then(Value::as_array)
        {
            if list.len() <= MAX_DOMAIN_RECORDS {
                for domain in list {
                    if let Some(domain) = domain.as_str().filter(|domain| safe_domain(domain)) {
                        domains.insert(domain.to_string());
                    }
                }
            }
        }
        for domain in domains {
            self.resource(
                plugin,
                "mail_domain",
                &domain,
                json!({}),
                source,
                "domain/administration.domains",
            );
        }
        for key in [
            "smtp_submission",
            "smtp_tls",
            "imap",
            "imaps",
            "dovecot_lmtp",
            "postfix_pickup",
        ] {
            if let Some(endpoint) = state
                .get("endpoints")
                .and_then(|value| text(value, key))
                .filter(|endpoint| safe_endpoint(endpoint))
            {
                self.resource(
                    plugin,
                    "endpoint",
                    endpoint,
                    json!({"protocol":key}),
                    source,
                    &format!("endpoints.{key}"),
                );
            }
        }
        if let Some(path) = text(state, "xray_socket_path").filter(|path| safe_path(path)) {
            self.resource(
                plugin,
                "unix_socket",
                path,
                json!({"role":"xray_routing"}),
                source,
                "xray_socket_path",
            );
        }
        self.limit("mail_server: accounts, mailbox/user lists, aliases, queue contents and configuration contents excluded".into());
    }

    fn embedding(&mut self, state: &Value, plugin: &str, source: &str) {
        self.extend(
            plugin,
            select(
                state,
                &[
                    "config_revision",
                    "provider",
                    "model_id",
                    "dimensions",
                    "status",
                    "model_digest",
                ],
            ),
        );
        if let Some(endpoint) = text(state, "endpoint").filter(|endpoint| safe_endpoint(endpoint)) {
            self.resource(
                plugin,
                "endpoint",
                endpoint,
                json!({"role":"embedding_configuration","reachability":"not_inferred"}),
                source,
                "endpoint",
            );
        }
    }

    fn ui(&mut self, state: &Value, plugin: &str, source: &str) {
        self.extend(plugin, select(state, &["status"]));
        for (kind, field, key) in [
            ("ui_package", "packages", "package_name"),
            ("ui_component", "components", "name"),
            ("ui_action", "actions", "name"),
        ] {
            for (name, record) in self.records(&state[field], key, &format!("json_render:{field}"))
            {
                let node = self.resource(
                    plugin,
                    kind,
                    &name,
                    json!({"authority":"catalog_declaration"}),
                    source,
                    &format!("{field}[{key}={name}]"),
                );
                self.extend(
                    &node,
                    select(record, &["version", "role", "category", "mutates_state"]),
                );
            }
        }
    }

    fn source_declarations(&mut self, plugins: &BTreeMap<String, GraphPluginProjection>) {
        // These describe current source ownership, never deployment or readiness.
        let declarations = [
            (
                "cognitive_mcp",
                "cognitive_runtime",
                "in-process cognitive MCP",
                "crates/op-cognitive-mcp/src/server.rs",
                "reasoning_retrieval",
                "reasoning and retrieval",
            ),
            (
                "cognitive_mcp",
                "reasoning_retrieval",
                "reasoning and retrieval",
                "crates/op-cognitive-mcp/src/code_tools.rs",
                "code_vectorization",
                "source code indexing and vectorization",
            ),
            (
                "ctl_plane_chatbot",
                "private_episode_memory",
                "identity-scoped episode memory",
                "crates/op-grpc-bridge/src/chatbot_episode_state.rs",
                "private_episode_cozo",
                "private episode Cozo persistence",
            ),
            (
                "memory",
                "memory_backend",
                "principal/session memory backend",
                "crates/op-plugins/src/state_plugins/memory_plugin.rs",
                "private_memory_cozo",
                "private memory Cozo persistence",
            ),
            (
                "cognitive_mcp",
                "compliance_policy",
                "compliance policy evaluation",
                "crates/op-cozo-store/src/lib.rs",
                "operational_taxonomy",
                "operational taxonomy registry",
            ),
            (
                "json_render",
                "chat_ui",
                "json-render chat UI",
                "vendor/3tched-json-render-ui/source.tar.gz#src/chat/model-catalog.ts",
                "model_picker",
                "shared model picker",
            ),
        ];
        for (plugin, from, from_label, source, to, to_label) in declarations {
            if !plugins.contains_key(plugin) {
                continue;
            }
            let from_id = id("subsystem", &[from]);
            let to_id = id("subsystem", &[to]);
            self.node(
                &from_id,
                "subsystem",
                from_label,
                json!({"authority":"source_declaration","runtime_status":"unknown"}),
            );
            self.node(
                &to_id,
                "subsystem",
                to_label,
                json!({"authority":"source_declaration","runtime_status":"unknown"}),
            );
            self.edge(
                &id("plugin", &[plugin]),
                "declares_subsystem",
                &from_id,
                declared(source, "source ownership"),
            );
            self.edge(
                &from_id,
                "declares_component",
                &to_id,
                declared(source, "source ownership"),
            );
        }
        if plugins.contains_key("json_render") {
            let ui = id("subsystem", &["chat_ui"]);
            let door = id("subsystem", &["grpc_web_door"]);
            self.node(
                &door,
                "subsystem",
                "generic gRPC-Web plugin door",
                json!({"authority":"source_declaration","runtime_status":"unknown"}),
            );
            self.edge(
                &ui,
                "calls",
                &door,
                declared(
                    "vendor/3tched-json-render-ui/source.tar.gz#src/grpc/door.ts",
                    "PluginService/CallMethod",
                ),
            );
            for (backend, locator) in [
                ("model_registry", "src/chat/model-catalog.ts#MODEL_REGISTRY"),
                ("tched_router", "src/chat/door-chat-adapter.ts#TCHED_ROUTER"),
            ] {
                if plugins.contains_key(backend) {
                    self.edge(
                        &id("subsystem", &["model_picker"]),
                        "declares_backend",
                        &id("plugin", &[backend]),
                        declared("vendor/3tched-json-render-ui/source.tar.gz", locator),
                    );
                }
            }
        }
        if plugins.contains_key("cognitive_mcp") && plugins.contains_key("qdrant") {
            self.edge(
                &id("subsystem", &["reasoning_retrieval"]),
                "declares_backend",
                &id("plugin", &["qdrant"]),
                declared(
                    "crates/op-cognitive-mcp/src/code_tools.rs",
                    "CodeSearchTool/CodeIndexTool",
                ),
            );
        }
        if plugins.contains_key("cognitive_mcp") && plugins.contains_key("embedding_model") {
            self.edge(
                &id("subsystem", &["code_vectorization"]),
                "declares_backend",
                &id("plugin", &["embedding_model"]),
                declared(
                    "crates/op-cognitive-mcp/src/server.rs",
                    "call-time embedding configuration",
                ),
            );
        }
        self.limit("Source declarations do not establish active runtime bindings, reachable endpoints or automatic vectorization".into());
    }

    fn resolve_container_references(&mut self) {
        for (owner, name, evidence) in std::mem::take(&mut self.container_references) {
            let container = id("container", &[&name]);
            if self.nodes.contains_key(&container) {
                self.edge(&owner, "runs_in", &container, evidence);
            } else {
                self.limit(format!(
                    "{owner}: referenced container {name} unavailable in Incus inventory"
                ));
            }
        }
    }
}

fn declared(source: &str, locator: &str) -> GraphEvidence {
    GraphEvidence {
        source: source.into(),
        locator: locator.into(),
        assertion: GraphAssertion::Declared,
    }
}
fn observed(source: &str, locator: &str) -> GraphEvidence {
    GraphEvidence {
        source: source.into(),
        locator: locator.into(),
        assertion: if source.starts_with("declared_configuration:") {
            GraphAssertion::Declared
        } else {
            GraphAssertion::Observed
        },
    }
}
fn object(value: Value) -> BTreeMap<String, Value> {
    value
        .as_object()
        .map(|object| {
            object
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        })
        .unwrap_or_default()
}
fn parent_cycle(start: u64, records: &BTreeMap<u64, &Value>) -> bool {
    let mut visited = BTreeSet::new();
    let mut current = start;
    while let Some(record) = records.get(&current) {
        if !visited.insert(current) {
            return true;
        }
        let Some(parent) = record.get("parent_id").and_then(Value::as_u64) else {
            return false;
        };
        if parent == 0 {
            return false;
        }
        current = parent;
    }
    false
}
fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)?
        .as_str()
        .filter(|text| !text.is_empty() && text.len() <= 512 && !text.chars().any(char::is_control))
}
fn select(value: &Value, keys: &[&str]) -> BTreeMap<String, Value> {
    keys.iter()
        .filter_map(|key| {
            value
                .get(*key)
                .filter(|value| {
                    value.is_boolean()
                        || value.is_number()
                        || value.as_str().is_some_and(|text| {
                            text.len() <= 256 && !text.chars().any(char::is_control)
                        })
                })
                .map(|value| ((*key).into(), value.clone()))
        })
        .collect()
}
fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 256
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.:@/+".contains(c))
}
fn safe_domain(domain: &str) -> bool {
    domain.len() <= 253
        && domain.contains('.')
        && domain
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
}
fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 512
        && !path.chars().any(char::is_control)
        && !path
            .split('/')
            .any(|part| matches!(part, "secrets" | "credentials") || part == "..")
        && !path.ends_with(".key")
        && !path.ends_with(".env")
}
fn safe_endpoint(endpoint: &str) -> bool {
    endpoint.len() <= 512
        && !endpoint.contains('@')
        && !endpoint.contains('?')
        && !endpoint.contains('#')
        && !endpoint.chars().any(char::is_whitespace)
        && safe_path(endpoint)
}
fn id(kind: &str, parts: &[&str]) -> String {
    let mut output = kind.to_string();
    for part in parts {
        output.push(':');
        for byte in part.bytes() {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'/') {
                output.push(char::from(byte));
            } else {
                output.push_str(&format!("%{byte:02X}"));
            }
        }
    }
    if output.len() > 512 {
        format!("{kind}:sha256:{:x}", Sha256::digest(output.as_bytes()))
    } else {
        output
    }
}

#[cfg(test)]
#[path = "system_graph_projection_tests.rs"]
mod tests;
