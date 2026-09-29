//! Shared, iterative resource gates for reference and private Canvas specs.
use std::collections::{HashMap, VecDeque};

use anyhow::{ensure, Context, Result};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JsonLimits {
    pub max_depth: usize,
    pub max_nodes: usize,
    pub max_bytes: usize,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphLimits {
    pub max_elements: usize,
    pub max_edges: usize,
    pub max_depth: usize,
    pub max_expanded_nodes: usize,
}

/// Check depth/nodes before entering recursive serializers or validators.
/// Byte counting then uses a bounded sink without duplicating the document.
pub fn check_json(value: &Value, limits: JsonLimits) -> Result<()> {
    let mut pending = vec![(value, 0usize)];
    let mut nodes = 0usize;
    while let Some((value, depth)) = pending.pop() {
        nodes = nodes.checked_add(1).context("JSON node count overflow")?;
        ensure!(nodes <= limits.max_nodes, "JSON node limit exceeded");
        ensure!(depth <= limits.max_depth, "JSON depth limit exceeded");
        let count = match value {
            Value::Object(object) => object.len(),
            Value::Array(array) => array.len(),
            _ => 0,
        };
        ensure!(
            nodes.saturating_add(pending.len()).saturating_add(count) <= limits.max_nodes,
            "JSON node limit exceeded"
        );
        match value {
            Value::Object(object) => {
                pending.extend(object.values().map(|child| (child, depth + 1)))
            }
            Value::Array(array) => pending.extend(array.iter().map(|child| (child, depth + 1))),
            _ => {}
        }
    }
    struct Counter {
        written: usize,
        maximum: usize,
    }
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.written = self
                .written
                .checked_add(bytes.len())
                .filter(|size| *size <= self.maximum)
                .ok_or_else(|| std::io::Error::other("JSON byte limit exceeded"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(
        &mut Counter {
            written: 0,
            maximum: limits.max_bytes,
        },
        value,
    )
    .context("JSON byte limit exceeded")
}

/// Validate every child edge, including unreachable cycles, and count actual
/// static DAG expansion. Dynamic repeats are a separate profile admission.
pub fn check_children_graph(
    root: &str,
    elements: &Map<String, Value>,
    limits: GraphLimits,
) -> Result<()> {
    ensure!(
        !elements.is_empty() && elements.len() <= limits.max_elements,
        "element graph count limit exceeded"
    );
    let keys: Vec<_> = elements.keys().map(String::as_str).collect();
    let indexes: HashMap<_, _> = keys.iter().enumerate().map(|(i, key)| (*key, i)).collect();
    let root = *indexes.get(root).context("root element does not exist")?;
    let mut edges = vec![Vec::new(); keys.len()];
    let mut indegree = vec![0usize; keys.len()];
    let mut edge_count = 0usize;
    for (index, key) in keys.iter().enumerate() {
        let children = elements[*key]
            .get("children")
            .and_then(Value::as_array)
            .context("element children must be an array")?;
        for child in children {
            edge_count = edge_count
                .checked_add(1)
                .context("element graph edge count overflow")?;
            ensure!(
                edge_count <= limits.max_edges,
                "element graph exceeds child edges limit"
            );
            let child = child.as_str().context("child reference must be a string")?;
            let target = *indexes
                .get(child)
                .context("child references missing element")?;
            edges[index].push(target);
            indegree[target] += 1;
        }
    }
    let mut queue: VecDeque<_> = indegree
        .iter()
        .enumerate()
        .filter_map(|(i, count)| (*count == 0).then_some(i))
        .collect();
    let mut order = Vec::with_capacity(keys.len());
    let mut depth = vec![1usize; keys.len()];
    while let Some(source) = queue.pop_front() {
        order.push(source);
        for &target in &edges[source] {
            depth[target] = depth[target].max(depth[source].saturating_add(1));
            indegree[target] -= 1;
            if indegree[target] == 0 {
                queue.push_back(target);
            }
        }
    }
    ensure!(
        order.len() == keys.len(),
        "element graph contains a cycle, including disconnected cycles"
    );
    ensure!(
        depth.iter().all(|value| *value <= limits.max_depth),
        "element graph exceeds maximum depth"
    );
    let cap = limits.max_expanded_nodes.saturating_add(1);
    let mut copies = vec![0usize; keys.len()];
    copies[root] = 1;
    let mut expanded = 1usize;
    for source in order {
        if copies[source] == 0 {
            continue;
        }
        for &target in &edges[source] {
            copies[target] = copies[target].saturating_add(copies[source]).min(cap);
            expanded = expanded.saturating_add(copies[source]).min(cap);
            ensure!(
                expanded <= limits.max_expanded_nodes,
                "expanded render graph exceeds node limit"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    const GRAPH: GraphLimits = GraphLimits {
        max_elements: 4096,
        max_edges: 16384,
        max_depth: 64,
        max_expanded_nodes: 4096,
    };
    fn node(children: Vec<String>) -> Value {
        json!({"children":children})
    }
    #[test]
    fn rejects_hidden_disconnected_cycles_and_bounded_map_exponential_expansion() {
        let mut elements = Map::new();
        elements.insert("root".into(), node(vec![]));
        elements.insert("hidden".into(), node(vec!["hidden".into()]));
        assert!(check_children_graph("root", &elements, GRAPH)
            .unwrap_err()
            .to_string()
            .contains("cycle"));
        elements.clear();
        for index in 0..14 {
            let next = (index + 1).to_string();
            elements.insert(
                index.to_string(),
                node(if index == 13 {
                    vec![]
                } else {
                    vec![next.clone(), next]
                }),
            );
        }
        assert!(check_children_graph("0", &elements, GRAPH)
            .unwrap_err()
            .to_string()
            .contains("expanded"));
    }
    #[test]
    fn bounds_depth_before_recursive_serialization_and_counts_bytes() {
        let limits = JsonLimits {
            max_depth: 64,
            max_nodes: 1024,
            max_bytes: 4096,
        };
        let mut deep = Value::Null;
        for _ in 0..66 {
            deep = json!([deep]);
        }
        assert!(check_json(&deep, limits)
            .unwrap_err()
            .to_string()
            .contains("depth"));
        assert!(check_json(&json!("x".repeat(4096)), limits).is_err());
        assert!(check_json(&json!([1, 2, 3]), limits).is_ok());
    }
}
