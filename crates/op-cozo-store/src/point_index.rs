use super::*;
use anyhow::{Context, ensure};

pub const VOYAGE_POINT_FAMILY: &str = "voyage-4";
pub const VOYAGE_POINT_DIM: usize = 1024;
const POINT_NAMESPACE: uuid::Uuid =
    uuid::Uuid::from_u128(0x926b_e3f4_4ec1_4c8d_a648_74f4_1462_e9b2);

#[derive(Debug, serde::Serialize)]
pub struct PointIndexEntry {
    pub entity: String,
    pub point_id: String,
    pub collection: String,
    pub model_family: String,
    pub dim: usize,
    pub chunker_ver: String,
}

pub fn vector_point_id(content_hash: &str, model_family: &str, chunker_ver: &str) -> uuid::Uuid {
    let identity = serde_json::json!([content_hash, model_family, chunker_ver]).to_string();
    uuid::Uuid::new_v5(&POINT_NAMESPACE, identity.as_bytes())
}

impl CozoGraphShuttle {
    /// Store provenance and graph links atomically before publishing a derived vector.
    pub fn register_vector_point(
        &self,
        entry: &PointIndexEntry,
        links: &[(String, String)],
    ) -> Result<()> {
        ensure!(
            !entry.entity.is_empty()
                && !entry.collection.is_empty()
                && !entry.chunker_ver.is_empty(),
            "point registry requires an entity, collection and chunker version"
        );
        uuid::Uuid::parse_str(&entry.point_id).context("point registry requires a UUID")?;
        ensure!(
            entry.model_family == VOYAGE_POINT_FAMILY && entry.dim == VOYAGE_POINT_DIM,
            "expected {VOYAGE_POINT_FAMILY} and {VOYAGE_POINT_DIM} dimensions"
        );
        let rows = links
            .iter()
            .map(|(kind, value)| {
                DataValue::List(vec![
                    entry.point_id.as_str().into(),
                    kind.as_str().into(),
                    value.as_str().into(),
                ])
            })
            .collect();
        cozo_run(
            &self.db,
            r#"
            { ?[entity] := *point_index{collection: $collection, point_id: $id, entity, model_family, dim, chunker_ver},
                entity != $entity || model_family != $family || dim != $dim || chunker_ver != $chunker
              :assert none }
            { ?[collection, point_id, entity, model_family, dim, chunker_ver] <-
                [[$collection, $id, $entity, $family, $dim, $chunker]]
              :put point_index {collection, point_id => entity, model_family, dim, chunker_ver} }
            { ?[point_id, kind] := point_id = $id, *chain_vector_link{point_id, kind}
              :rm chain_vector_link {point_id, kind} }
            { ?[point_id, kind, value] <- $links :put chain_vector_link {point_id, kind => value} }
        "#,
            Params::from([
                ("collection".into(), entry.collection.as_str().into()),
                ("id".into(), entry.point_id.as_str().into()),
                ("entity".into(), entry.entity.as_str().into()),
                ("family".into(), entry.model_family.as_str().into()),
                ("dim".into(), i64::try_from(entry.dim)?.into()),
                ("chunker".into(), entry.chunker_ver.as_str().into()),
                ("links".into(), DataValue::List(rows)),
            ]),
        )?;
        Ok(())
    }

    /// Graph read for the IDs returned by Qdrant, scoped to their source collection.
    pub fn vector_point_links(&self, collection: &str, ids: &[String]) -> Result<NamedRows> {
        cozo_run(
            &self.db,
            r#"
            ?[point_id, entity, kind, value] := point_id in $ids,
                *point_index{collection: $collection, point_id, entity},
                *chain_vector_link{point_id, kind, value}
        "#,
            Params::from([
                ("collection".into(), collection.into()),
                (
                    "ids".into(),
                    DataValue::List(ids.iter().map(|id| id.as_str().into()).collect()),
                ),
            ]),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_provenance_without_vectors_and_refuses_conflicting_identity() -> Result<()> {
        let db = CozoGraphShuttle::new_in_memory()?;
        let mut entry = PointIndexEntry {
            entity: "block:1".into(),
            point_id: vector_point_id("hash", VOYAGE_POINT_FAMILY, "v1").to_string(),
            collection: "footprints".into(),
            model_family: VOYAGE_POINT_FAMILY.into(),
            dim: VOYAGE_POINT_DIM,
            chunker_ver: "v1".into(),
        };
        let links = [("event_hash".into(), "hash".into())];
        db.register_vector_point(&entry, &links)?;
        db.register_vector_point(&entry, &links)?;
        entry.entity = "block:2".into();
        assert!(db.register_vector_point(&entry, &[]).is_err());
        let ids = [entry.point_id];
        assert_eq!(db.vector_point_links("footprints", &ids)?.rows.len(), 1);
        assert!(
            db.vector_point_links("another_collection", &ids)?
                .rows
                .is_empty()
        );
        Ok(())
    }
}
