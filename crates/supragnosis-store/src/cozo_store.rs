//! Cozo(RocksDB) 어댑터 - 파일 기반 영속 저장소.
//!
//! 지식을 3개의 stored relation 으로 보관하고, CozoScript(Datalog)로 질의한다.
//! 복합 필드(aliases/properties/provenance)는 JSON 문자열 컬럼으로 저장한다.
//! `traverse` 는 Cozo 재귀 Datalog(min 집계)로 최단 홉 그래프 순회를 수행한다.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use cozo::{DataValue, Db, NamedRows, RocksDbStorage, ScriptMutability};
use serde_json::json;

use supragnosis_core::{
    Entity, KnowledgeStore, Observation, Relation, SearchHit, SearchHitKind, StoreError,
    TraverseHit,
};

pub struct CozoStore {
    db: Db<RocksDbStorage>,
}

impl CozoStore {
    /// 주어진 디렉터리에 RocksDB 백엔드로 열고(없으면 생성) 스키마를 보장한다.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let path = path.as_ref();
        std::fs::create_dir_all(path).map_err(|e| StoreError::Backend(e.to_string()))?;
        let path_str = path.to_string_lossy().to_string();
        let db = cozo::new_cozo_rocksdb(&path_str).map_err(|e| StoreError::Backend(e.to_string()))?;
        let store = Self { db };
        store.ensure_schema()?;
        Ok(store)
    }

    fn run(
        &self,
        script: &str,
        params: BTreeMap<String, DataValue>,
        mutable: bool,
    ) -> Result<NamedRows, StoreError> {
        let m = if mutable {
            ScriptMutability::Mutable
        } else {
            ScriptMutability::Immutable
        };
        self.db
            .run_script(script, params, m)
            .map_err(|e| StoreError::Backend(e.to_string()))
    }

    fn ensure_schema(&self) -> Result<(), StoreError> {
        let existing = self.run("::relations", BTreeMap::new(), false)?;
        let name_idx = existing
            .headers
            .iter()
            .position(|h| h == "name")
            .unwrap_or(0);
        let have: HashSet<String> = existing
            .rows
            .iter()
            .filter_map(|r| r.get(name_idx).and_then(|v| v.get_str()).map(str::to_string))
            .collect();

        if !have.contains("observation") {
            self.run(
                ":create observation {id: String => content: String, workspace: String, data: String}",
                BTreeMap::new(),
                true,
            )?;
        }
        if !have.contains("entity") {
            self.run(
                ":create entity {id: String => etype: String, name: String, workspace: String, data: String}",
                BTreeMap::new(),
                true,
            )?;
        }
        if !have.contains("relation") {
            self.run(
                ":create relation {id: String => src: String, dst: String, rtype: String, data: String}",
                BTreeMap::new(),
                true,
            )?;
        }
        Ok(())
    }
}

/// (키, 문자열값) 쌍을 CozoScript 파라미터 맵으로. (파라미터화로 인젝션 방지)
fn params(pairs: &[(&str, String)]) -> BTreeMap<String, DataValue> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), DataValue::from(v.as_str())))
        .collect()
}

impl KnowledgeStore for CozoStore {
    fn add_observation(&self, obs: Observation) -> Result<(), StoreError> {
        let workspace = obs.provenance.workspace.clone();
        // 복합 필드는 data JSON 컬럼에 (provenance + derived_from 계보).
        let data = json!({
            "provenance": obs.provenance,
            "derived_from": obs.derived_from,
        })
        .to_string();
        let p = params(&[
            ("id", obs.id),
            ("content", obs.content),
            ("workspace", workspace),
            ("data", data),
        ]);
        self.run(
            "?[id, content, workspace, data] <- [[$id, $content, $workspace, $data]]\n\
             :put observation {id => content, workspace, data}",
            p,
            true,
        )?;
        Ok(())
    }

    fn get_entity(&self, id: &str) -> Option<Entity> {
        let p = params(&[("id", id.to_string())]);
        let rows = self
            .run(
                "?[etype, name, data] := *entity{id: $id, etype, name, data}",
                p,
                false,
            )
            .ok()?;
        let row = rows.rows.into_iter().next()?;
        let etype = row.first()?.get_str()?.to_string();
        let name = row.get(1)?.get_str()?.to_string();
        let data_str = row.get(2)?.get_str()?.to_string();
        let data: serde_json::Value =
            serde_json::from_str(&data_str).unwrap_or(serde_json::Value::Null);
        let aliases = data
            .get("aliases")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        let properties = data
            .get("properties")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let provenance = data
            .get("provenance")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        Some(Entity {
            id: id.to_string(),
            kind: etype,
            canonical_name: name,
            aliases,
            properties,
            provenance,
        })
    }

    fn put_entity(&self, entity: Entity) -> Result<(), StoreError> {
        let workspace = entity
            .provenance
            .first()
            .map(|p| p.workspace.clone())
            .unwrap_or_default();
        let data = json!({
            "aliases": entity.aliases,
            "properties": entity.properties,
            "provenance": entity.provenance,
        })
        .to_string();
        let p = params(&[
            ("id", entity.id),
            ("etype", entity.kind),
            ("name", entity.canonical_name),
            ("workspace", workspace),
            ("data", data),
        ]);
        self.run(
            "?[id, etype, name, workspace, data] <- [[$id, $etype, $name, $workspace, $data]]\n\
             :put entity {id => etype, name, workspace, data}",
            p,
            true,
        )?;
        Ok(())
    }

    fn add_relation(&self, rel: Relation) -> Result<(), StoreError> {
        // 복합/시간 필드는 data JSON 컬럼에 (provenance + 유효구간).
        let data = json!({
            "provenance": rel.provenance,
            "valid_from": rel.valid_from,
            "valid_to": rel.valid_to,
        })
        .to_string();
        let p = params(&[
            ("id", rel.id),
            ("src", rel.from),
            ("dst", rel.to),
            ("rtype", rel.kind),
            ("data", data),
        ]);
        self.run(
            "?[id, src, dst, rtype, data] <- [[$id, $src, $dst, $rtype, $data]]\n\
             :put relation {id => src, dst, rtype, data}",
            p,
            true,
        )?;
        Ok(())
    }

    fn relations_of(&self, entity_id: &str) -> Vec<Relation> {
        let p = params(&[("id", entity_id.to_string())]);
        let script = "?[id, src, dst, rtype, data] := *relation{id, src, dst, rtype, data}, src == $id\n\
                      ?[id, src, dst, rtype, data] := *relation{id, src, dst, rtype, data}, dst == $id";
        let rows = match self.run(script, p, false) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };
        rows.rows
            .iter()
            .filter_map(|r| {
                let data: serde_json::Value = serde_json::from_str(r.get(4)?.get_str()?).ok()?;
                Some(Relation {
                    id: r.first()?.get_str()?.to_string(),
                    from: r.get(1)?.get_str()?.to_string(),
                    to: r.get(2)?.get_str()?.to_string(),
                    kind: r.get(3)?.get_str()?.to_string(),
                    provenance: serde_json::from_value(data.get("provenance")?.clone()).ok()?,
                    valid_from: data.get("valid_from").and_then(|v| v.as_u64()),
                    valid_to: data.get("valid_to").and_then(|v| v.as_u64()),
                })
            })
            .collect()
    }

    fn search(&self, query: &str, workspace: Option<&str>, limit: usize) -> Vec<SearchHit> {
        let q = query.trim().to_lowercase();
        let mut hits: Vec<SearchHit> = Vec::new();

        // 엔티티: 정규명 부분일치.
        let mut ent_params = params(&[("q", q.clone())]);
        let ent_script = match workspace {
            Some(ws) => {
                ent_params.insert("ws".to_string(), DataValue::from(ws));
                "?[id, name] := *entity{id, name, workspace}, str_includes(lowercase(name), $q), workspace == $ws"
            }
            None => "?[id, name] := *entity{id, name}, str_includes(lowercase(name), $q)",
        };
        if let Ok(rows) = self.run(ent_script, ent_params, false) {
            for r in &rows.rows {
                if let (Some(id), Some(name)) = (
                    r.first().and_then(|v| v.get_str()),
                    r.get(1).and_then(|v| v.get_str()),
                ) {
                    hits.push(SearchHit {
                        kind: SearchHitKind::Entity,
                        id: id.to_string(),
                        snippet: name.to_string(),
                        score: 1.0,
                    });
                }
            }
        }

        // 관측: 본문 부분일치.
        let mut obs_params = params(&[("q", q.clone())]);
        let obs_script = match workspace {
            Some(ws) => {
                obs_params.insert("ws".to_string(), DataValue::from(ws));
                "?[id, content] := *observation{id, content, workspace}, str_includes(lowercase(content), $q), workspace == $ws"
            }
            None => "?[id, content] := *observation{id, content}, str_includes(lowercase(content), $q)",
        };
        if let Ok(rows) = self.run(obs_script, obs_params, false) {
            for r in &rows.rows {
                if let (Some(id), Some(content)) = (
                    r.first().and_then(|v| v.get_str()),
                    r.get(1).and_then(|v| v.get_str()),
                ) {
                    hits.push(SearchHit {
                        kind: SearchHitKind::Observation,
                        id: id.to_string(),
                        snippet: content.chars().take(160).collect(),
                        score: 0.7,
                    });
                }
            }
        }

        hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        hits.truncate(limit);
        hits
    }

    fn traverse(&self, start_id: &str, max_depth: usize, limit: usize) -> Vec<TraverseHit> {
        let md = max_depth.max(1);
        // 재귀 Datalog: min 집계로 최단 홉 거리를 구하고 엔티티 메타를 조인.
        let script = format!(
            "reach[dst, min(d)] := *relation{{src: $start, dst}}, d = 1\n\
             reach[dst, min(d)] := reach[nid, d0], *relation{{src: nid, dst}}, d = d0 + 1, d <= {md}\n\
             ?[id, depth, name, etype] := reach[id, depth], *entity{{id, name, etype}}\n\
             :limit {limit}"
        );
        let p = params(&[("start", start_id.to_string())]);
        let rows = match self.run(&script, p, false) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };
        rows.rows
            .iter()
            .filter_map(|r| {
                Some(TraverseHit {
                    id: r.first()?.get_str()?.to_string(),
                    depth: r.get(1)?.get_int()? as usize,
                    name: r.get(2)?.get_str()?.to_string(),
                    kind: r.get(3)?.get_str()?.to_string(),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use supragnosis_core::Provenance;

    fn tmp_dir() -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("supragnosis-cozo-{}-{}", std::process::id(), nanos))
    }

    fn prov() -> Provenance {
        Provenance {
            host: "h".into(),
            on_behalf_of: Some("ashon".into()),
            workspace: "ws1".into(),
            source_ref: None,
            observed_at: 1,
            confidence: 1.0,
            trust_tier: supragnosis_core::TrustTier::default(),
        }
    }

    fn ent(name: &str) -> Entity {
        Entity {
            id: Entity::make_id("ws1", name),
            kind: "Concept".into(),
            canonical_name: name.into(),
            aliases: vec![],
            properties: serde_json::Value::Null,
            provenance: vec![prov()],
        }
    }

    #[test]
    fn cozo_roundtrip_search_traverse_persist() {
        let dir = tmp_dir();
        {
            let store = CozoStore::open(&dir).unwrap();

            for n in ["a", "b", "c"] {
                store.put_entity(ent(n)).unwrap();
            }
            let (a, b, c) = (
                Entity::make_id("ws1", "a"),
                Entity::make_id("ws1", "b"),
                Entity::make_id("ws1", "c"),
            );
            store
                .add_relation(Relation {
                    id: Relation::make_id(&a, "rel", &b),
                    from: a.clone(),
                    to: b.clone(),
                    kind: "rel".into(),
                    provenance: prov(),
                    valid_from: None,
                    valid_to: None,
                })
                .unwrap();
            store
                .add_relation(Relation {
                    id: Relation::make_id(&b, "rel", &c),
                    from: b.clone(),
                    to: c.clone(),
                    kind: "rel".into(),
                    provenance: prov(),
                    valid_from: None,
                    valid_to: None,
                })
                .unwrap();
            store
                .add_observation(Observation::new("hello rust world".into(), prov()))
                .unwrap();

            // 조회
            assert_eq!(store.get_entity(&a).unwrap().canonical_name, "a");
            // b 는 a->b, b->c 두 관계에 등장
            assert_eq!(store.relations_of(&b).len(), 2);
            // 검색
            assert!(!store.search("rust", Some("ws1"), 10).is_empty());
            assert!(store.search("rust", Some("other"), 10).is_empty());
            // 순회: a -> b(1홉), c(2홉)
            let hits = store.traverse(&a, 5, 100);
            let ids: HashSet<_> = hits.iter().map(|h| h.id.clone()).collect();
            assert!(ids.contains(&b) && ids.contains(&c), "traverse got {hits:?}");
        }

        // 영속성: 재오픈 후에도 데이터 유지
        {
            let store2 = CozoStore::open(&dir).unwrap();
            let a = Entity::make_id("ws1", "a");
            assert!(store2.get_entity(&a).is_some(), "data should persist");
        }

        let _ = std::fs::remove_dir_all(&dir);
    }
}
