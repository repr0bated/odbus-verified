use super::*;
use anyhow::{Context, ensure};

pub const SEMANTIC_VECTOR_DIMENSIONS: usize = 1024;

pub struct SemanticPoint {
    pub id: String,
    pub vector: Vec<f32>,
    pub payload: Value,
}

pub struct SemanticHit {
    pub id: String,
    pub score: f32,
    pub payload: Value,
}

fn validate_relation(relation: &str) -> Result<()> {
    ensure!(
        relation.starts_with("semantic_")
            && relation.len() <= 80
            && relation
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'),
        "invalid semantic relation name"
    );
    Ok(())
}

fn vector_value(vector: Vec<f32>) -> Result<DataValue> {
    ensure!(
        vector.len() == SEMANTIC_VECTOR_DIMENSIONS
            && vector.iter().all(|value| value.is_finite())
            && vector.iter().any(|value| *value != 0.0),
        "invalid cosine embedding: expected {SEMANTIC_VECTOR_DIMENSIONS} finite components and a nonzero norm"
    );
    Ok(DataValue::Vec(Vector::F32(vector.into())))
}

impl CozoGraphShuttle {
    pub fn has_semantic_vectors(&self, relation: &str) -> Result<bool> {
        validate_relation(relation)?;
        let relations = cozo_run(&self.db, "::relations", Params::new())?;
        Ok(relations
            .rows
            .iter()
            .any(|row| row[0].get_str() == Some(relation)))
    }

    pub fn prepare_semantic_vectors(&self, relation: &str) -> Result<()> {
        validate_relation(relation)?;
        let index = format!("{relation}:semantic");
        let relations = cozo_run(&self.db, "::relations", Params::new())?;
        let exists = |name| {
            relations
                .rows
                .iter()
                .any(|row| row[0].get_str() == Some(name))
        };
        if !exists(relation) {
            cozo_run(
                &self.db,
                &format!(
                    ":create {relation} {{id: String => embedding: <F32; {SEMANTIC_VECTOR_DIMENSIONS}>, payload: Json}}"
                ),
                Params::new(),
            )?;
        }
        if !exists(&index) {
            cozo_run(
                &self.db,
                &format!(
                    "::hnsw create {index} {{dim: {SEMANTIC_VECTOR_DIMENSIONS}, dtype: F32, fields: [embedding], distance: Cosine, m: 16, ef_construction: 100}}"
                ),
                Params::new(),
            )?;
        }
        Ok(())
    }

    pub fn put_semantic_vectors(&self, relation: &str, points: Vec<SemanticPoint>) -> Result<()> {
        validate_relation(relation)?;
        let rows = points
            .into_iter()
            .map(|point| {
                ensure!(
                    point.payload.is_object(),
                    "semantic payload must be an object"
                );
                Ok(DataValue::List(vec![
                    point.id.into(),
                    vector_value(point.vector)?,
                    point.payload.into(),
                ]))
            })
            .collect::<Result<Vec<_>>>()?;
        if !rows.is_empty() {
            cozo_run(
                &self.db,
                &format!(
                    "?[id, embedding, payload] <- $rows :put {relation} {{id => embedding, payload}}"
                ),
                Params::from([("rows".into(), DataValue::List(rows))]),
            )?;
        }
        Ok(())
    }

    pub fn delete_semantic_vectors(
        &self,
        relation: &str,
        filters: &[(String, String)],
    ) -> Result<()> {
        validate_relation(relation)?;
        ensure!(!filters.is_empty(), "semantic delete requires a scope");
        let (condition, params) = semantic_filters(filters);
        cozo_run(
            &self.db,
            &format!("?[id] := *{relation}{{id, payload}}{condition} :rm {relation} {{id}}"),
            params,
        )?;
        Ok(())
    }

    pub fn search_semantic_vectors(
        &self,
        relation: &str,
        vector: Vec<f32>,
        limit: u64,
        filters: &[(String, String)],
    ) -> Result<Vec<SemanticHit>> {
        validate_relation(relation)?;
        if limit == 0 {
            return Ok(Vec::new());
        }
        let limit = i64::try_from(limit)?;
        let (condition, mut params) = semantic_filters(filters);
        params.insert("query".into(), vector_value(vector)?);
        params.insert("limit".into(), limit.into());
        params.insert("ef".into(), limit.max(100).into());
        let rule = if filters.is_empty() {
            format!(
                "?[distance, id, payload] := ~{relation}:semantic{{id, payload | query: $query, k: $limit, ef: $ef, bind_distance: distance}}"
            )
        } else {
            format!(
                "?[distance, id, payload] := *{relation}{{id, embedding, payload}}{condition}, distance = cos_dist($query, embedding)"
            )
        };
        let result = cozo_run(
            &self.db,
            &format!("{rule}\n:order distance, id\n:limit {limit}"),
            params,
        )?;
        result
            .rows
            .into_iter()
            .map(|mut row| {
                let score = 1.0 - row[0].get_float().context("invalid semantic distance")?;
                let id = row[1]
                    .get_str()
                    .context("invalid semantic point id")?
                    .to_owned();
                Ok(SemanticHit {
                    id,
                    score: score as f32,
                    payload: row.remove(2).into(),
                })
            })
            .collect()
    }

    pub fn list_semantic_vectors(
        &self,
        relation: &str,
        limit: u32,
        filters: &[(String, String)],
    ) -> Result<Vec<SemanticPoint>> {
        validate_relation(relation)?;
        let (condition, params) = semantic_filters(filters);
        let result = cozo_run(
            &self.db,
            &format!(
                "?[id, embedding, payload] := *{relation}{{id, embedding, payload}}{condition}\n:order id\n:limit {limit}"
            ),
            params,
        )?;
        result
            .rows
            .into_iter()
            .map(|mut row| {
                let id = row[0]
                    .get_str()
                    .context("invalid semantic point id")?
                    .to_owned();
                let payload = row.remove(2).into();
                let DataValue::Vec(Vector::F32(vector)) = row.remove(1) else {
                    anyhow::bail!("invalid stored semantic vector");
                };
                Ok(SemanticPoint {
                    id,
                    vector: vector.to_vec(),
                    payload,
                })
            })
            .collect()
    }

    pub fn semantic_count(&self, relation: &str) -> Result<u64> {
        validate_relation(relation)?;
        let result = cozo_run(
            &self.db,
            &format!("?[count(id)] := *{relation}{{id}}"),
            Params::new(),
        )?;
        Ok(u64::try_from(
            result
                .rows
                .first()
                .and_then(|row| row[0].get_int())
                .unwrap_or(0),
        )?)
    }
}

fn semantic_filters(filters: &[(String, String)]) -> (String, Params) {
    let mut condition = String::new();
    let mut params = Params::new();
    for (index, (field, value)) in filters.iter().enumerate() {
        condition.push_str(&format!(
            ", get(payload, $filter_key_{index}, null) == $filter_value_{index}"
        ));
        params.insert(format!("filter_key_{index}"), field.as_str().into());
        params.insert(format!("filter_value_{index}"), value.as_str().into());
    }
    (condition, params)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_vectors_preserve_scope_metadata_and_delete_exactly() -> Result<()> {
        let db = CozoGraphShuttle::new_in_memory()?;
        let relation = "semantic_fixture";
        db.prepare_semantic_vectors(relation)?;
        let vector = |axis: usize| {
            let mut values = vec![0.0; SEMANTIC_VECTOR_DIMENSIONS];
            values[axis] = 1.0;
            values
        };
        let point = |id: &str, container: &str, axis| SemanticPoint {
            id: id.into(),
            vector: vector(axis),
            payload: serde_json::json!({
                "container_id": container, "entry_key": "pref", "symbols": ["render"]
            }),
        };
        db.put_semantic_vectors(relation, vec![point("a", "alice", 0), point("b", "bob", 1)])?;
        let filters = [
            ("container_id".into(), "bob".into()),
            ("entry_key".into(), "pref".into()),
        ];
        let matches = db.search_semantic_vectors(relation, vector(0), 2, &[])?;
        assert_eq!(matches[0].id, "a");
        assert_eq!(matches[0].payload["symbols"][0], "render");
        let matches = db.search_semantic_vectors(relation, vector(0), 2, &filters)?;
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].id, "b");
        assert_eq!(
            db.list_semantic_vectors(relation, 3, &filters)?[0].vector,
            vector(1)
        );
        db.delete_semantic_vectors(relation, &filters)?;
        assert_eq!(db.semantic_count(relation)?, 1);
        assert!(
            db.search_semantic_vectors(relation, vector(0), 2, &filters)?
                .is_empty()
        );
        Ok(())
    }
}
