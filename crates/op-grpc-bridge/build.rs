use std::env;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
#[path = "src/json_schema_proto.rs"]
mod json_schema_proto;
use json_schema_proto::json_schema_type_to_proto;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    for name in [
        "THREETCHED_CAPTURE_BIN",
        "THREETCHED_CAPTURE_MANIFEST",
        "THREETCHED_BIN",
        "THREETCHED_MANIFEST",
        "TCHED_ROUTER_CONFIG_DIR",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    for (capture, runtime, default) in [
        ("THREETCHED_CAPTURE_BIN", "THREETCHED_BIN", "/usr/local/libexec/op-dbus/threetched/threetched"),
        (
            "THREETCHED_CAPTURE_MANIFEST",
            "THREETCHED_MANIFEST",
            "/usr/share/op-dbus/threetched-manifests/tched-router.json",
        ),
    ] {
        let selected = PathBuf::from(
            env::var_os(capture)
                .or_else(|| env::var_os(runtime))
                .unwrap_or_else(|| default.into()),
        );
        // Watch both the selected link and its resolved release artifact.
        println!("cargo:rerun-if-changed={}", selected.display());
        if let Ok(resolved) = std::fs::canonicalize(&selected)
            && resolved != selected
        {
            println!("cargo:rerun-if-changed={}", resolved.display());
        }
    }
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let generated_proto = out_dir.join("plugin_methods.proto");
    let generated_routes = out_dir.join("plugin_method_routes.rs");
    let generated_contracts = out_dir.join("compiled_method_contracts.json");
    op_plugins::state_plugins::threetched::prepare_schema_capture()?;
    let plugin_methods = collect_plugin_methods();

    std::fs::write(
        &generated_proto,
        generate_plugin_methods_proto(&plugin_methods),
    )?;
    std::fs::write(
        &generated_routes,
        generate_plugin_method_routes(&plugin_methods),
    )?;
    // Compatibility evidence for this binary, never a runtime plugin catalog.
    // Preserve full method/state declarations from the collection producing routes.
    std::fs::write(
        &generated_contracts,
        generate_compiled_method_contracts(&plugin_methods)?,
    )?;

    // Compile all domain protos into a single combined FileDescriptorSet so
    // that tonic-reflection exposes every service in one query.
    //
    // Adding a new domain proto:
    //   1. Add the .proto file under proto/
    //   2. Add it to the compile_protos list below
    //   3. Add rerun-if-changed below
    //   4. Add the generated server/client to grpc_server.rs
    tonic_build::configure()
        // Generated inputs live in OUT_DIR; watching them would make this
        // script invalidate itself on the next Cargo invocation. The authored
        // proto roots and plugin inputs are explicitly watched below.
        .emit_rerun_if_changed(false)
        .build_server(true)
        .build_client(true)
        .file_descriptor_set_path(out_dir.join("operation_descriptor.bin"))
        .compile_protos(
            &[
                "proto/operation.proto",
                "proto/mail.proto",
                "proto/privacy_network.proto",
                "proto/registration.proto",
                "proto/registry.proto",
                "proto/emqx_exhook_v3.proto",
                "../op-chat/proto/chat.proto",
                "src/grpc/chat_router.proto",
                generated_proto
                    .to_str()
                    .ok_or("generated plugin proto path is not valid UTF-8")?,
            ],
            &[
                "proto",
                "../op-chat/proto",
                "src/grpc",
                out_dir
                    .to_str()
                    .ok_or("generated proto include path is not valid UTF-8")?,
            ],
        )?;

    // Watch imported authored protos as well as the entry points.
    println!("cargo:rerun-if-changed=proto");
    println!("cargo:rerun-if-changed=../op-chat/proto");
    println!("cargo:rerun-if-changed=src/grpc");
    println!("cargo:rerun-if-changed=proto/operation.proto");
    println!("cargo:rerun-if-changed=proto/mail.proto");
    println!("cargo:rerun-if-changed=proto/privacy_network.proto");
    println!("cargo:rerun-if-changed=proto/registration.proto");
    println!("cargo:rerun-if-changed=proto/registry.proto");
    println!("cargo:rerun-if-changed=proto/emqx_exhook_v3.proto");
    println!("cargo:rerun-if-changed=../op-chat/proto/chat.proto");
    println!("cargo:rerun-if-changed=src/grpc/chat_router.proto");
    println!("cargo:rerun-if-changed=../op-plugins/src/state_plugins");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/json_schema_proto.rs");

    Ok(())
}

#[derive(Debug, Clone)]
struct PluginMethodSet {
    plugin_id: String,
    plugin_version: String,
    state_fields: std::collections::HashMap<String, op_state_store::FieldSchema>,
    service_name: String,
    server_module: String,
    server_type: String,
    trait_name: String,
    methods: Vec<PluginMethod>,
}

#[derive(Debug, Clone)]
struct PluginMethod {
    rpc_name: String,
    rust_name: String,
    schema_name: String,
    input_name: String,
    output_name: String,
    args: simd_json::OwnedValue,
    returns: simd_json::OwnedValue,
    declaration: op_state_store::MethodDecl,
}

fn collect_plugin_methods() -> Vec<PluginMethodSet> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build plugin schema collection runtime");
    rt.block_on(async {
        let store = Arc::new(op_state_store::MemoryStore::new());
        let registry = op_plugins::DefaultPluginRegistry::new(store);
        let mut sets = Vec::new();

        for plugin_id in op_plugins::DefaultPluginRegistry::available_plugins() {
            let Ok(plugin) = registry.load_plugin(&plugin_id).await else {
                continue;
            };
            let Some(schema) = plugin.schema() else {
                continue;
            };
            if schema.methods.is_empty() {
                continue;
            }

            let mut method_names = schema.methods.keys().cloned().collect::<Vec<_>>();
            method_names.sort();
            let methods = method_names
                .into_iter()
                .map(|schema_name| {
                    let method_decl = schema
                        .methods
                        .get(&schema_name)
                        .expect("method declaration must exist");
                    let rpc_name = to_pascal_ident(&schema_name);
                    let message_prefix = to_pascal_ident(&plugin_id);
                    PluginMethod {
                        rust_name: to_snake_ident(&rpc_name),
                        rpc_name: rpc_name.clone(),
                        schema_name,
                        input_name: format!("{message_prefix}{rpc_name}Request"),
                        output_name: format!("{message_prefix}{rpc_name}Response"),
                        args: method_decl.args.clone(),
                        returns: method_decl
                            .returns
                            .clone()
                            .unwrap_or_else(|| simd_json::json!({})),
                        declaration: method_decl.clone(),
                    }
                })
                .collect::<Vec<_>>();

            let trait_name = format!("{}PluginMethods", to_pascal_ident(&plugin_id));
            sets.push(PluginMethodSet {
                plugin_id: plugin_id.clone(),
                plugin_version: schema.version.clone(),
                state_fields: schema.fields.clone(),
                service_name: trait_name.clone(),
                server_module: format!("{}_server", to_snake_ident(&trait_name)),
                server_type: format!("{}Server", trait_name),
                trait_name,
                methods,
            });
        }

        sets.sort_by(|left, right| left.plugin_id.cmp(&right.plugin_id));
        sets
    })
}

fn generate_compiled_method_contracts(
    sets: &[PluginMethodSet],
) -> Result<Vec<u8>, serde_json::Error> {
    let contracts = sets
        .iter()
        .map(|set| {
            let methods = set
                .methods
                .iter()
                .map(|method| (&method.schema_name, &method.declaration))
                .collect::<std::collections::BTreeMap<_, _>>();
            (
                &set.plugin_id,
                serde_json::json!({
                    "version": set.plugin_version,
                    "fields": set.state_fields,
                    "methods": methods
                }),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    serde_json::to_vec(&contracts)
}

fn generate_plugin_methods_proto(sets: &[PluginMethodSet]) -> String {
    let mut proto = String::new();
    writeln!(proto, "syntax = \"proto3\";").unwrap();
    writeln!(proto).unwrap();
    writeln!(proto, "package operation.plugin.v1;").unwrap();
    writeln!(proto).unwrap();
    writeln!(proto, "import \"google/protobuf/struct.proto\";").unwrap();
    writeln!(proto).unwrap();

    for set in sets {
        writeln!(proto, "// PluginSchema source: {}", set.plugin_id).unwrap();
        for method in &set.methods {
            writeln!(
                proto,
                "{}",
                schema_message_proto(&method.input_name, &method.args)
            )
            .unwrap();
            writeln!(
                proto,
                "{}",
                schema_message_proto(&method.output_name, &method.returns)
            )
            .unwrap();
        }
        writeln!(proto, "service {} {{", set.service_name).unwrap();
        for method in &set.methods {
            writeln!(
                proto,
                "  rpc {}({}) returns ({});",
                method.rpc_name, method.input_name, method.output_name
            )
            .unwrap();
        }
        writeln!(proto, "}}").unwrap();
        writeln!(proto).unwrap();
    }

    proto
}

fn generate_plugin_method_routes(sets: &[PluginMethodSet]) -> String {
    let mut rust = String::new();
    writeln!(rust, "#[allow(clippy::all)]").unwrap();
    writeln!(rust, "#[allow(unused_qualifications)]").unwrap();
    writeln!(rust, "mod generated_plugin_method_routes {{").unwrap();
    writeln!(rust, "    use crate::grpc_server::OperationGrpcServer;").unwrap();
    writeln!(rust, "    use tonic::{{Request, Response, Status}};").unwrap();
    writeln!(rust).unwrap();

    for set in sets {
        writeln!(
            rust,
            "    #[tonic::async_trait]\n    impl crate::proto::plugin_methods::{}::{} for OperationGrpcServer {{",
            set.server_module, set.trait_name
        )
        .unwrap();
        for method in &set.methods {
            writeln!(
                rust,
                "        async fn {}(&self, request: Request<crate::proto::plugin_methods::{}>) -> Result<Response<crate::proto::plugin_methods::{}>, Status> {{",
                method.rust_name, method.input_name, method.output_name
            )
            .unwrap();
            writeln!(
                rust,
                "            self.call_generated_plugin_method_typed({:?}, {:?}, {:?}, {:?}, request).await",
                set.plugin_id,
                method.schema_name,
                format!("operation.plugin.v1.{}", method.input_name),
                format!("operation.plugin.v1.{}", method.output_name)
            )
            .unwrap();
            writeln!(rust, "        }}").unwrap();
        }
        writeln!(rust, "    }}").unwrap();
        writeln!(rust).unwrap();
    }

    writeln!(
        rust,
        "    pub(crate) fn add_routes(mut routes: tonic::service::Routes, server: OperationGrpcServer, validator: std::sync::Arc<crate::oracle_assertion::AssertionValidator>) -> tonic::service::Routes {{"
    )
    .unwrap();
    for set in sets {
        writeln!(
            rust,
            "        routes = routes.add_service(tonic_web::enable(crate::proto::plugin_methods::{}::{}::with_interceptor(server.clone(), crate::interceptor::make_ghostbridge_interceptor(validator.clone()))));",
            set.server_module, set.server_type
        )
        .unwrap();
    }
    writeln!(rust, "        routes").unwrap();
    writeln!(rust, "    }}").unwrap();
    writeln!(rust).unwrap();

    // Emitted from the same `sets` that produced `add_routes`, so the names
    // reflection advertises can never drift from the ones actually mounted.
    writeln!(
        rust,
        "    pub(crate) const LEGACY_PLUGIN_METHOD_SERVICES: &[(&str, &str)] = &["
    )
    .unwrap();
    for set in sets {
        writeln!(
            rust,
            "        ({:?}, {:?}),",
            set.plugin_id,
            format!("operation.plugin.v1.{}", set.service_name)
        )
        .unwrap();
    }
    writeln!(rust, "    ];").unwrap();
    writeln!(rust, "}}").unwrap();
    writeln!(
        rust,
        "pub(crate) use generated_plugin_method_routes::{{add_routes, LEGACY_PLUGIN_METHOD_SERVICES}};"
    )
    .unwrap();
    rust
}

fn schema_message_proto(message_name: &str, schema: &simd_json::OwnedValue) -> String {
    let fields = schema_fields(schema);
    let mut proto = format!("message {} {{\n", message_name);

    if fields.is_empty() {
        proto.push_str("  // Empty schema object.\n");
    }

    for field in fields {
        let repeated = if field.repeated {
            "repeated "
        } else if json_schema_proto::needs_scalar_presence(&field.proto_type, field.repeated) {
            "optional "
        } else {
            ""
        };
        proto.push_str(&format!(
            "  {}{} {} = {}{};\n",
            repeated,
            field.proto_type,
            field.name,
            field.number,
            field.json_name.as_ref().map_or_else(String::new, |name| {
                format!(" [json_name = {}]", proto_string_literal(name))
            })
        ));
    }

    proto.push_str("}\n");
    proto
}

#[derive(Debug, Clone)]
struct ProtoField {
    name: String,
    json_name: Option<String>,
    proto_type: String,
    repeated: bool,
    number: i32,
}

fn schema_fields(schema: &simd_json::OwnedValue) -> Vec<ProtoField> {
    let Ok(json_schema) = serde_json::to_value(schema) else {
        return vec![fallback_field("payload", "google.protobuf.Value", false)];
    };

    let Some(properties) = json_schema
        .get("properties")
        .and_then(|value| value.as_object())
    else {
        return Vec::new();
    };

    let mut fields = properties.iter().collect::<Vec<_>>();
    fields.sort_by_key(|(name, _)| *name);
    op_blob::descriptor::validate_proto_field_aliases(properties.keys().map(String::as_str))
        .expect("JSON Schema properties have ambiguous protobuf field aliases");

    fields
        .into_iter()
        .map(|(name, property)| {
            let (proto_type, repeated) = json_schema_type_to_proto(property);
            let proto_name = op_blob::descriptor::proto_field_name(name);
            ProtoField {
                json_name: (proto_name != *name).then(|| name.clone()),
                name: proto_name,
                proto_type,
                repeated,
                number: stable_field_number(name),
            }
        })
        .collect()
}

fn fallback_field(name: &str, proto_type: &str, repeated: bool) -> ProtoField {
    ProtoField {
        name: name.to_string(),
        json_name: None,
        proto_type: proto_type.to_string(),
        repeated,
        number: stable_field_number(name),
    }
}

fn stable_field_number(name: &str) -> i32 {
    const FNV_OFFSET: u32 = 0x811c9dc5;
    const FNV_PRIME: u32 = 0x01000193;
    const MAX_FIELD: u32 = 536_870_911;
    const RESERVED_START: u32 = 19_000;
    const RESERVED_END: u32 = 19_999;

    let mut hash = FNV_OFFSET;
    for byte in name.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    let mut number = (hash % MAX_FIELD).max(1);
    if (RESERVED_START..=RESERVED_END).contains(&number) {
        number += RESERVED_END - RESERVED_START + 1;
    }
    number as i32
}

fn proto_string_literal(value: &str) -> String {
    let mut literal = String::from("\"");
    for byte in value.bytes() {
        match byte {
            b'"' => literal.push_str("\\\""),
            b'\\' => literal.push_str("\\\\"),
            0x20..=0x7e => literal.push(char::from(byte)),
            _ => write!(literal, "\\{byte:03o}").unwrap(),
        }
    }
    literal.push('"');
    literal
}

fn to_pascal_ident(value: &str) -> String {
    let mut ident = String::new();
    let mut uppercase_next = true;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            if uppercase_next {
                ident.push(ch.to_ascii_uppercase());
                uppercase_next = false;
            } else {
                ident.push(ch);
            }
        } else {
            uppercase_next = true;
        }
    }
    if ident.is_empty() {
        "Generated".to_string()
    } else if ident.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        format!("P{}", ident)
    } else {
        ident
    }
}

fn to_snake_ident(value: &str) -> String {
    let mut ident = String::new();
    let mut previous_was_underscore = false;
    for (idx, ch) in value.chars().enumerate() {
        if ch.is_ascii_alphanumeric() {
            if ch.is_ascii_uppercase() {
                if idx > 0 && !previous_was_underscore {
                    ident.push('_');
                }
                ident.push(ch.to_ascii_lowercase());
                previous_was_underscore = false;
            } else {
                ident.push(ch);
                previous_was_underscore = false;
            }
        } else if !previous_was_underscore && !ident.is_empty() {
            ident.push('_');
            previous_was_underscore = true;
        }
    }
    while ident.ends_with('_') {
        ident.pop();
    }
    if ident.is_empty() {
        "generated".to_string()
    } else if ident.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        format!("p_{}", ident)
    } else {
        ident
    }
}
