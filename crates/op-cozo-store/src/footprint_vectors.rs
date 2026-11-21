use super::*;
use anyhow::{Context, ensure};

pub struct FootprintVector {
    pub id: String,
    pub vector: Vec<f32>,
    pub metadata: Value,
    pub links: Vec<(String, String)>,
}

pub struct FootprintMatch {
    pub score: f32,
    pub payload: Value,
}

impl CozoGraphShuttle {
    pub fn prepare_footprint_index(&self, dimensions: u64) -> Result<()> {
        let relations = cozo_run(&self.db, "::relations", Params::new())?;
        if !relations
            .rows
            .iter()
            .any(|row| row[0].get_str() == Some("footprint_vector"))
        {
            cozo_run(
                &self.db,
                &format!(
                    ":create footprint_vector {{point_id: String => embedding: <F32; {dimensions}>, block_num: Int?, event_hash: String, plugin_id: String, actor_id: String, session_id: String, metadata: Json}}"
                ),
                Params::new(),
            )?;
        }
        if !relations
            .rows
            .iter()
            .any(|row| row[0].get_str() == Some("footprint_vector:semantic"))
        {
            cozo_run(
                &self.db,
                &format!(
                    "::hnsw create footprint_vector:semantic {{dim: {dimensions}, m: 16, dtype: F32, fields: [embedding], distance: Cosine, ef_construction: 100}}"
                ),
                Params::new(),
            )?;
        }
        if !relations
            .rows
            .iter()
            .any(|row| row[0].get_str() == Some("footprint_vector:owner"))
        {
            cozo_run(
                &self.db,
                "::index create footprint_vector:owner {actor_id, session_id}",
                Params::new(),
            )?;
        }
        Ok(())
    }

    pub fn footprint_count(&self) -> Result<u64> {
        let result = cozo_run(
            &self.db,
            "?[count(point_id)] := *footprint_vector{point_id}",
            Params::new(),
        )?;
        let count = result
            .rows
            .first()
            .and_then(|row| row[0].get_int())
            .unwrap_or(0);
        Ok(u64::try_from(count)?)
    }

    pub fn indexed_footprint_ids(&self, ids: &[String]) -> Result<Vec<String>> {
        let params = Params::from([(
            "ids".into(),
            DataValue::List(ids.iter().cloned().map(DataValue::from).collect()),
        )]);
        let rows = cozo_run(
            &self.db,
            "?[point_id] := point_id in $ids, *footprint_vector{point_id}",
            params,
        )?;
        rows.rows
            .iter()
            .map(|row| {
                row[0]
                    .get_str()
                    .map(str::to_owned)
                    .context("invalid indexed footprint ID")
            })
            .collect()
    }

    pub fn put_footprint_vectors(&self, points: Vec<FootprintVector>) -> Result<()> {
        if points.is_empty() {
            return Ok(());
        }
        let mut ids = Vec::with_capacity(points.len());
        let mut vectors = Vec::with_capacity(points.len());
        let mut links = Vec::new();
        for point in points {
            ensure!(
                point.vector.iter().all(|value| value.is_finite())
                    && point.vector.iter().any(|value| *value != 0.0),
                "non-finite footprint embedding"
            );
            let id = DataValue::from(point.id);
            ids.push(id.clone());
            let field = |key: &str| DataValue::from(point.metadata[key].as_str().unwrap_or(""));
            vectors.push(DataValue::List(vec![
                id.clone(),
                DataValue::Vec(Vector::F32(point.vector.into())),
                point.metadata["block_num"]
                    .as_i64()
                    .map(DataValue::from)
                    .unwrap_or(DataValue::Null),
                field("event_hash"),
                field("plugin_id"),
                field("actor_id"),
                field("session_id"),
                DataValue::from(point.metadata),
            ]));
            links.extend(
                point.links.into_iter().map(|(kind, value)| {
                    DataValue::List(vec![id.clone(), kind.into(), value.into()])
                }),
            );
        }
        cozo_run(
            &self.db,
            r#"
            { ?[point_id, kind] := point_id in $ids, *chain_vector_link{point_id, kind}
              :rm chain_vector_link {point_id, kind} }
            { ?[point_id, kind, value] <- $links
              :put chain_vector_link {point_id, kind => value} }
            { ?[point_id, embedding, block_num, event_hash, plugin_id, actor_id, session_id, metadata] <- $vectors
              :put footprint_vector {point_id => embedding, block_num, event_hash, plugin_id, actor_id, session_id, metadata} }
        "#,
            Params::from([
                ("ids".into(), DataValue::List(ids)),
                ("links".into(), DataValue::List(links)),
                ("vectors".into(), DataValue::List(vectors)),
            ]),
        )?;
        Ok(())
    }

    pub fn footprint_embedding(&self, id: &str) -> Result<Vec<f32>> {
        let found = cozo_run(
            &self.db,
            "?[embedding] := point_id = $id, *footprint_vector{point_id, embedding}",
            Params::from([("id".into(), id.into())]),
        )?;
        match found.rows.first().and_then(|row| row.first()) {
            Some(DataValue::Vec(Vector::F32(vector))) => Ok(vector.to_vec()),
            _ => anyhow::bail!("footprint embedding {id} is missing"),
        }
    }

    pub fn search_footprint_vectors(
        &self,
        vector: Vec<f32>,
        limit: u64,
        scope: Option<(&str, &str)>,
    ) -> Result<Vec<FootprintMatch>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        ensure!(
            vector.iter().all(|value| value.is_finite())
                && vector.iter().any(|value| *value != 0.0),
            "invalid query embedding for cosine search"
        );
        let limit = i64::try_from(limit)?;
        let mut params = Params::from([
            ("query".into(), DataValue::Vec(Vector::F32(vector.into()))),
            ("limit".into(), limit.into()),
            ("ef".into(), limit.max(100).into()),
        ]);
        let rule = match scope {
            Some((actor, session)) => {
                params.insert("actor".into(), actor.into());
                params.insert("session".into(), session.into());
                "?[distance, metadata] := *footprint_vector{actor_id, session_id, embedding, metadata}, actor_id = $actor, session_id = $session, distance = cos_dist($query, embedding)"
            }
            None => {
                "?[distance, metadata] := ~footprint_vector:semantic{metadata | query: $query, k: $limit, ef: $ef, bind_distance: distance}"
            }
        };
        let rows = cozo_run(
            &self.db,
            &format!("{rule}\n:order distance\n:limit {limit}"),
            params,
        )?;
        rows.rows
            .into_iter()
            .map(|mut row| {
                let distance = row[0].get_float().context("invalid footprint score")?;
                Ok(FootprintMatch {
                    score: (1.0 - distance) as f32,
                    payload: row.remove(1).into(),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footprint_vectors_replay_search_and_filter_by_verified_scope() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("footprints");
        let store = CozoGraphShuttle::new_persistent_private(path.clone())?;
        store.prepare_footprint_index(3)?;
        store.prepare_footprint_index(3)?;
        let point = |id: &str, actor: &str, vector: Vec<f32>| FootprintVector {
            id: id.into(),
            vector,
            metadata: serde_json::json!({"block_num": 1, "actor_id": actor, "session_id": "session", "event_hash": id}),
            links: vec![("actor_id".into(), actor.into())],
        };
        store.put_footprint_vectors(vec![
            point("alpha", "alice", vec![1.0, 0.0, 0.0]),
            point("beta", "bob", vec![0.0, 1.0, 0.0]),
        ])?;
        store.put_footprint_vectors(vec![point("alpha", "alice", vec![1.0, 0.0, 0.0])])?;
        assert_eq!(
            store.indexed_footprint_ids(&["alpha".into(), "missing".into()])?,
            ["alpha"]
        );
        let hits = store.search_footprint_vectors(store.footprint_embedding("alpha")?, 2, None)?;
        assert_eq!(hits.len(), 2);
        assert_eq!(store.footprint_count()?, 2);
        assert_eq!(hits[0].payload["event_hash"], "alpha");
        assert_eq!(hits[0].score, 1.0);
        let scoped =
            store.search_footprint_vectors(vec![1.0, 0.0, 0.0], 2, Some(("bob", "session")))?;
        assert_eq!(scoped.len(), 1);
        assert_eq!(scoped[0].payload["event_hash"], "beta");
        let retry = point("alpha", "changed", vec![1.0, 0.0]);
        assert!(store.put_footprint_vectors(vec![retry]).is_err());
        let links = cozo_run(
            &store.db,
            "?[value] := *chain_vector_link{point_id: 'alpha', kind: 'actor_id', value}",
            Params::new(),
        )?;
        assert_eq!(links.rows[0][0].get_str(), Some("alice"));
        drop(store);
        let store = CozoGraphShuttle::new_persistent_private(path)?;
        store.prepare_footprint_index(3)?;
        assert_eq!(store.footprint_embedding("alpha")?, [1.0, 0.0, 0.0]);
        Ok(())
    }
}
