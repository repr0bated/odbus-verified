//! Object-backed graph validation and bounded data flow; no raw tool executor.
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use anyhow::{Result, Context, ensure};
use op_agents::{object_graph::{ObjectWorkflow, ObjectNode, ObjectBinding, TypedPort, CheckpointRequirement, DefinitionStatus, ObjectConnection}, recipes::digest};
use op_state_store::PluginSchema;
use serde_json::{Value, json};

pub const MAX_EXPANDED_TASKS: usize = 64;
pub const MAX_PLAN_BYTES: usize = 128 * 1024;
pub const MAX_STEP_BYTES: usize = 16 * 1024;

/// Hash precisely the authored wire shape with its digest field empty, without
/// cloning the graph (including its bounded but potentially large arguments).
pub fn content_digest(graph: &ObjectWorkflow) -> Result<String> {
    #[derive(serde::Serialize)]
    struct Unsigned<'a> {
        workflow_id: &'a str, version: u64, name: &'a str, description: &'a str,
        status: &'a DefinitionStatus, content_digest: &'static str,
        nodes: &'a [ObjectNode], connections: &'a [ObjectConnection],
    }
    digest(&Unsigned { workflow_id:&graph.workflow_id, version:graph.version,
        name:&graph.name, description:&graph.description, status:&graph.status,
        content_digest:"", nodes:&graph.nodes, connections:&graph.connections })
}

fn annotation(key: &str) -> bool {
    matches!(key, "title"|"description"|"default"|"examples"|"$comment"|"deprecated"|"readOnly"|"writeOnly")
}

// A private comparison view, never a replacement schema or a grant. Traverse
// schema-bearing keywords only: properties named "description", and literal
// enum/const/default data, must not be treated as schema annotation keys.
struct ValidationContract<'a>(&'a Value);
struct ShapeState<'a> {
    root: &'a Value,
    active: std::cell::RefCell<[Option<&'a Value>;128]>,
    visited: std::cell::Cell<usize>,
}
struct SchemaView<'a,'s> { node:&'a Value, state:&'s ShapeState<'a>, depth:usize }
struct SchemaMap<'a,'s> { values:&'a serde_json::Map<String,Value>, state:&'s ShapeState<'a>, depth:usize, dependencies:bool }
struct SchemaArray<'a,'s> { values:&'a [Value], state:&'s ShapeState<'a>, depth:usize }

impl serde::Serialize for ValidationContract<'_> {
    fn serialize<S:serde::Serializer>(&self,serializer:S)->std::result::Result<S::Ok,S::Error>{
        use serde::ser::SerializeMap;
        let state=ShapeState{root:self.0,active:std::cell::RefCell::new([None;128]),visited:std::cell::Cell::new(0)};
        let mut map=serializer.serialize_map(Some(2))?;
        map.serialize_entry("dialect",self.0.get("$schema").and_then(Value::as_str).unwrap_or(op_state_store::DEFAULT_SCHEMA_DIALECT))?;
        map.serialize_entry("validation",&SchemaView{node:self.0,state:&state,depth:0})?;
        map.end()
    }
}
impl serde::Serialize for SchemaMap<'_,'_> {
    fn serialize<S:serde::Serializer>(&self,serializer:S)->std::result::Result<S::Ok,S::Error>{
        use serde::ser::SerializeMap;
        let mut map=serializer.serialize_map(Some(self.values.len()))?;
        for(name,value)in self.values{
            if self.dependencies&&value.is_array(){map.serialize_entry(name,value)?;}
            else{map.serialize_entry(name,&SchemaView{node:value,state:self.state,depth:self.depth})?;}
        }
        map.end()
    }
}
impl serde::Serialize for SchemaArray<'_,'_> {
    fn serialize<S:serde::Serializer>(&self,serializer:S)->std::result::Result<S::Ok,S::Error>{
        use serde::ser::SerializeSeq;
        let mut seq=serializer.serialize_seq(Some(self.values.len()))?;
        for node in self.values{seq.serialize_element(&SchemaView{node,state:self.state,depth:self.depth})?;}
        seq.end()
    }
}
impl serde::Serialize for SchemaView<'_,'_> {
    fn serialize<S:serde::Serializer>(&self,serializer:S)->std::result::Result<S::Ok,S::Error>{
        use serde::ser::{SerializeMap,Error};
        if self.depth>=128||self.state.visited.get()>=65_536{
            return Err(S::Error::custom("schema expansion bound exceeded"));
        }
        self.state.visited.set(self.state.visited.get()+1);
        if self.state.active.borrow()[..self.depth].iter().flatten().any(|node|std::ptr::eq(*node,self.node)){
            return Err(S::Error::custom("recursive local schema reference unsupported"));
        }
        self.state.active.borrow_mut()[self.depth]=Some(self.node);
        if let Value::Bool(value)=self.node{return serializer.serialize_bool(*value)}
        let object=self.node.as_object().ok_or_else(||S::Error::custom("schema node is not object or boolean"))?;
        if ["$dynamicRef","$recursiveRef","$anchor","$dynamicAnchor","$recursiveAnchor"].iter().any(|key|object.contains_key(*key))
            ||(!std::ptr::eq(self.node,self.state.root)&&(object.contains_key("$id")||object.contains_key("$schema"))){
            return Err(S::Error::custom("dynamic or nested schema resource unsupported"));
        }
        let referenced=if let Some(reference)=object.get("$ref"){
            let pointer=reference.as_str().and_then(|value|value.strip_prefix('#'))
                .filter(|value|(value.is_empty()||value.starts_with('/'))&&!value.contains('%'))
                .ok_or_else(||S::Error::custom("external or noncanonical local reference denied"))?;
            Some(self.state.root.pointer(pointer).ok_or_else(||S::Error::custom("local reference unresolved"))?)
        }else{None};
        if let Some(node)=referenced{
            if object.keys().all(|key|annotation(key)||matches!(key.as_str(),"$ref"|"$defs"|"definitions"|"$schema")
                ||(key=="$id"&&std::ptr::eq(self.node,self.state.root))){
                return serde::Serialize::serialize(&SchemaView{node,state:self.state,depth:self.depth+1},serializer);
            }
        }
        let mut map=serializer.serialize_map(None)?;
        for(key,value)in object{
            if annotation(key)||matches!(key.as_str(),"$defs"|"definitions"|"$schema")
                ||(key=="$id"&&std::ptr::eq(self.node,self.state.root)){continue}
            match key.as_str(){
                "$ref"=>map.serialize_entry(key,&SchemaView{node:referenced.ok_or_else(||S::Error::custom("local reference absent"))?,state:self.state,depth:self.depth+1})?,
                "properties"|"patternProperties"|"dependentSchemas"|"dependencies"=>{
                    let values=value.as_object().ok_or_else(||S::Error::custom("schema map invalid"))?;
                    map.serialize_entry(key,&SchemaMap{values,state:self.state,depth:self.depth+1,dependencies:key=="dependencies"})?;
                }
                "allOf"|"anyOf"|"oneOf"|"prefixItems"=>{
                    let values=value.as_array().ok_or_else(||S::Error::custom("schema array invalid"))?;
                    map.serialize_entry(key,&SchemaArray{values,state:self.state,depth:self.depth+1})?;
                }
                "items" if value.is_array()=>map.serialize_entry(key,&SchemaArray{values:value.as_array().ok_or_else(||S::Error::custom("tuple items invalid"))?,state:self.state,depth:self.depth+1})?,
                "items"|"additionalItems"|"additionalProperties"|"propertyNames"|"contains"|"if"|"then"|"else"|"not"|"unevaluatedProperties"|"unevaluatedItems"|"contentSchema"=>
                    map.serialize_entry(key,&SchemaView{node:value,state:self.state,depth:self.depth+1})?,
                _=>map.serialize_entry(key,value)?,
            }
        }
        map.end()
    }
}

/// Compile an actual sealed method schema in its declared dialect. This is not
/// the stricter recipe/agent schema profile and must not replace that profile.
pub fn compile_method_contract(contract:&Value)->Result<jsonschema::Validator>{
    op_state_store::method_receipt::validate_json_size(contract,MAX_PLAN_BYTES)?;
    op_state_store::exact_schema::compile_method_schema(contract)
        .context("invalid self-contained method contract")
}
fn compatible(source:&Value,target:&Value)->Result<bool>{
    Ok(digest(&ValidationContract(source))?==digest(&ValidationContract(target))?)
}

fn port(name: &str, pointer: String, contract: Value, required: bool) -> Result<TypedPort> {
    let validator=compile_method_contract(&contract)?;
    let default = contract.get("default").cloned();
    if let Some(value) = &default { validator.validate(value).map_err(|_| anyhow::anyhow!("schema default is invalid"))?; }
    Ok(TypedPort { name:name.into(), schema_pointer:pointer, contract_digest:digest(&contract)?, contract, required, default })
}
fn ports(contract: &Value, input: bool) -> Result<Vec<TypedPort>> {
    let mut result = Vec::new();
    if let Some(properties) = contract.get("properties").and_then(Value::as_object) {
        let required: BTreeSet<_> = contract.get("required").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
        for (name, property) in properties {
            let property=op_state_store::exact_schema::self_contained_method_field_schema(property,contract)?;
            result.push(port(name, format!("/properties/{}", name.replace('~',"~0").replace('/',"~1")), property, required.contains(name.as_str()))?);
        }
    } else { result.push(port("$", "".into(), contract.clone(), input)?); }
    Ok(result)
}

/// Derive each operation variant from the same actual discovered object schema.
/// The caller supplies actual object path and current method visibility; no child paths are synthesized.
pub fn discover_operations(plugin_id: &str, object_path: &str, schema_hash: &str, schema: &PluginSchema, policy_revision: u64, visible_methods: &BTreeSet<String>) -> Result<Vec<ObjectNode>> {
    ensure!(!plugin_id.is_empty() && object_path.starts_with("/org/opdbus/v1/plugins/") && !schema_hash.is_empty() && policy_revision > 0, "object pin incomplete");
    let mut nodes=Vec::new();
    let mut methods: Vec<_>=schema.methods.iter().collect(); methods.sort_by_key(|(id,_)| *id);
    for (method, declaration) in methods {
        if !visible_methods.contains(method) { continue; }
        let Some(capability)=declaration.required_capability.as_ref() else { continue; };
        let input=serde_json::to_value(&declaration.args)?;
        let Some(output)=&declaration.returns else { continue; };
        let output=serde_json::to_value(output)?;
        compile_method_contract(&input)?; compile_method_contract(&output)?;
        nodes.push(ObjectNode {
            node_id: format!("{plugin_id}:{method}"), plugin_id:plugin_id.into(), object_path:object_path.into(),
            schema_hash:schema_hash.into(), schema_version:schema.version.clone(), policy_revision,
            binding:ObjectBinding::Operation { method:method.clone(), required_capability:capability.clone() },
            input_ports:ports(&input,true)?, output_ports:ports(&output,false)?, arguments:BTreeMap::new(), checkpoint:None,
        });
    }
    Ok(nodes)
}
fn same_binding(node: &ObjectNode, discovered: &ObjectNode) -> bool {
    node.plugin_id==discovered.plugin_id && node.object_path==discovered.object_path && node.schema_hash==discovered.schema_hash
        && node.schema_version==discovered.schema_version && node.policy_revision==discovered.policy_revision && node.binding==discovered.binding
        && node.input_ports==discovered.input_ports && node.output_ports==discovered.output_ports
}

/// Returns topological node IDs only after all pins, ports, defaults and bindings validate.
pub fn validate(graph: &ObjectWorkflow, discovered: &[ObjectNode]) -> Result<Vec<String>> {
    ensure!(!graph.workflow_id.is_empty() && graph.workflow_id.len()<=256 && graph.version>0, "invalid graph identity");
    ensure!(!graph.nodes.is_empty() && graph.nodes.len()<=MAX_EXPANDED_TASKS && graph.connections.len()<=MAX_EXPANDED_TASKS*MAX_EXPANDED_TASKS, "graph capacity exceeded");
    ensure!(serde_json::to_vec(graph)?.len()<=MAX_PLAN_BYTES, "workflow exceeds plan byte bound");
    ensure!(graph.content_digest==content_digest(graph)?, "workflow digest mismatch");
    let nodes:BTreeMap<_,_>=graph.nodes.iter().map(|n| (n.node_id.as_str(),n)).collect();
    ensure!(nodes.len()==graph.nodes.len(), "duplicate node ID");
    let mut incoming:BTreeMap<&str,usize>=nodes.keys().map(|id|(*id,0)).collect();
    let mut adjacency:BTreeMap<&str,Vec<&str>>=BTreeMap::new();
    let mut connected=BTreeSet::new();
    for node in &graph.nodes {
        ensure!(!node.node_id.is_empty() && node.node_id.len()<=256, "invalid node ID");
        ensure!(discovered.iter().any(|d| same_binding(node,d)), "node absent, unauthorized or stale");
        ensure!(serde_json::to_vec(&node.arguments)?.len()<=MAX_STEP_BYTES, "node arguments exceed byte bound");
        for (name,value) in &node.arguments {
            let port=node.input_ports.iter().find(|p| &p.name==name).context("argument has no declared input port")?;
            compile_method_contract(&port.contract)?.validate(value).map_err(|_| anyhow::anyhow!("invalid typed node argument"))?;
        }
        if let Some(checkpoint)=&node.checkpoint {
            match checkpoint {
                CheckpointRequirement::Approval { approver_capability,preview } => ensure!(!approver_capability.is_empty() && preview.len()<=4096, "invalid approval checkpoint"),
                CheckpointRequirement::Input { approver_capability,preview,response_schema,input_port } => {
                    ensure!(!approver_capability.is_empty() && preview.len()<=4096, "invalid input checkpoint");
                    compile_method_contract(response_schema)?;
                    let port=node.input_ports.iter().find(|p| &p.name==input_port).context("checkpoint input port missing")?;
                    ensure!(digest(response_schema)?==port.contract_digest, "checkpoint schema differs from input port");
                    ensure!(!node.arguments.contains_key(input_port), "checkpoint input also has a static argument");
                }
            }
        }
    }
    for edge in &graph.connections {
        let source=nodes.get(edge.from_node.as_str()).context("connection source missing")?;
        let target=nodes.get(edge.to_node.as_str()).context("connection target missing")?;
        let output=source.output_ports.iter().find(|p|p.name==edge.from_port).context("source output port missing")?;
        let input=target.input_ports.iter().find(|p|p.name==edge.to_port).context("target input port missing")?;
        ensure!(edge.expected_contract_digest==output.contract_digest, "source port pin is stale");
        ensure!(compatible(&output.contract,&input.contract)?, "port validation contracts incompatible");
        ensure!(output.required||output.schema_pointer.is_empty()||!input.required||input.default.is_some(),
            "required input cannot rely on an optional output without a default");
        ensure!(!matches!(&target.checkpoint,Some(CheckpointRequirement::Input { input_port,.. }) if input_port==&edge.to_port), "checkpoint input also has an incoming edge");
        ensure!(!target.arguments.contains_key(&edge.to_port) && connected.insert((edge.to_node.as_str(),edge.to_port.as_str())), "duplicate/default-overridden input binding");
        *incoming.get_mut(edge.to_node.as_str()).context("target missing")?+=1;
        adjacency.entry(edge.from_node.as_str()).or_default().push(edge.to_node.as_str());
    }
    for node in &graph.nodes {
        for port in &node.input_ports {
            let checkpoint_input=matches!(&node.checkpoint,Some(CheckpointRequirement::Input { input_port,.. }) if input_port==&port.name);
            ensure!(!port.required || port.default.is_some() || node.arguments.contains_key(&port.name) || connected.contains(&(node.node_id.as_str(),port.name.as_str())) || checkpoint_input, "required input port unbound");
        }
    }
    let mut queue:VecDeque<_>=incoming.iter().filter(|(_,n)|**n==0).map(|(id,_)|*id).collect();
    let mut ordered=Vec::with_capacity(nodes.len());
    while let Some(id)=queue.pop_front() {
        ordered.push(id.to_owned());
        for target in adjacency.get(id).into_iter().flatten() {
            let count=incoming.get_mut(target).context("edge target disappeared")?; *count-=1;
            if *count==0 { queue.push_back(target); }
        }
    }
    ensure!(ordered.len()==nodes.len(), "workflow contains cycle"); Ok(ordered)
}

/// Resolve only declared predecessor ports and schema defaults. Pending input
/// remains absent until a schema-valid checkpoint response is consumed.
pub fn bindings(graph: &ObjectWorkflow, node: &ObjectNode, results: &BTreeMap<String,Value>) -> Result<BTreeMap<String,Value>> {
    let mut args=node.arguments.clone();
    for edge in graph.connections.iter().filter(|e|e.to_node==node.node_id) {
        let source=graph.nodes.iter().find(|n|n.node_id==edge.from_node).context("source node missing")?;
        let port=source.output_ports.iter().find(|p|p.name==edge.from_port).context("source port missing")?;
        let result=results.get(&edge.from_node).context("dependency not complete")?;
        let value=if port.schema_pointer.is_empty(){Some(result)}else{result.get(&port.name)};
        let Some(value)=value else{ensure!(!port.required,"required predecessor output absent");continue};
        compile_method_contract(&port.contract)?.validate(value).map_err(|_|anyhow::anyhow!("predecessor result violates pinned port"))?;
        args.insert(edge.to_port.clone(),value.clone());
    }
    for port in &node.input_ports { if !args.contains_key(&port.name) { if let Some(value)=&port.default { args.insert(port.name.clone(),value.clone()); } } }
    ensure!(serde_json::to_vec(&args)?.len()<=MAX_STEP_BYTES,"resolved port bindings exceed step bound");
    Ok(args)
}

pub fn arguments(graph: &ObjectWorkflow, node: &ObjectNode, results: &BTreeMap<String,Value>) -> Result<Value> {
    let mut args=bindings(graph,node,results)?;
    for port in &node.input_ports {
        ensure!(!port.required || args.contains_key(&port.name), "required resolved input absent");
        if let Some(value)=args.get(&port.name) {
            compile_method_contract(&port.contract)?.validate(value).map_err(|_|anyhow::anyhow!("resolved input violates pinned port"))?;
        }
    }
    let value=if node.input_ports.len()==1 && node.input_ports[0].schema_pointer.is_empty() { args.remove(&node.input_ports[0].name).context("root input missing")? } else { json!(args) };
    ensure!(serde_json::to_vec(&value)?.len()<=MAX_STEP_BYTES,"resolved arguments exceed step bound"); Ok(value)
}

/// Validate actual data before releasing any dependent node. Whole-operation
/// contracts are additionally checked by Runtime against the sealed declaration.
pub fn validate_output(node: &ObjectNode, result: &Value) -> Result<()> {
    ensure!(serde_json::to_vec(result)?.len()<=MAX_STEP_BYTES,"node result exceeds step bound");
    for port in &node.output_ports {
        let value=if port.schema_pointer.is_empty() { Some(result) } else { result.get(&port.name) };
        if let Some(value)=value {
            compile_method_contract(&port.contract)?.validate(value).map_err(|_|anyhow::anyhow!("result violates pinned output port"))?;
        } else { ensure!(!port.required,"required output port absent"); }
    }
    Ok(())
}
