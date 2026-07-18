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
    cosine_similarity, Assertions, Entity, KnowledgeStore, Observation, Provenance, Relation,
    SearchHit, SearchHitKind, StoreError, TraverseHit,
};

pub struct CozoStore {
    db: Db<RocksDbStorage>,
    /// 벡터 인덱스 차원. Some(n) 이면 obs_vec 관계 + HNSW 인덱스를 두고 의미 검색을
    /// 네이티브 ANN 으로 처리한다. None 이면 임베딩을 data JSON 에만 두고 브루트포스한다.
    vector_dim: Option<usize>,
}

impl CozoStore {
    /// 벡터 인덱스 없이 연다(의미 검색은 브루트포스 코사인).
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        Self::open_inner(path, None)
    }

    /// 임베딩 차원을 지정해 연다 - obs_vec 관계 + HNSW 인덱스를 만들어 의미 검색을
    /// 네이티브 ANN 으로 가속한다. `dim` 은 임베딩 공급자의 차원과 일치해야 한다.
    pub fn open_with_embedding_dim(path: impl AsRef<Path>, dim: usize) -> Result<Self, StoreError> {
        Self::open_inner(path, Some(dim))
    }

    fn open_inner(path: impl AsRef<Path>, vector_dim: Option<usize>) -> Result<Self, StoreError> {
        let path = path.as_ref();
        std::fs::create_dir_all(path).map_err(|e| StoreError::Backend(e.to_string()))?;
        let path_str = path.to_string_lossy().to_string();
        let db =
            cozo::new_cozo_rocksdb(&path_str).map_err(|e| StoreError::Backend(e.to_string()))?;
        let store = Self { db, vector_dim };
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
            .filter_map(|r| {
                r.get(name_idx)
                    .and_then(|v| v.get_str())
                    .map(str::to_string)
            })
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
        // 벡터 인덱스: obs_vec/ent_vec 관계 + HNSW. relation 과 index 를 함께 만들므로
        // 관계 부재를 둘 다의 부재로 본다(create-together 불변식). 관측과 엔티티를 각각
        // 색인해 시맨틱 검색이 관측 본문과 엔티티 이름 양쪽으로 회상하게 한다(회상 공백 제거).
        if let Some(dim) = self.vector_dim {
            for rel in ["obs_vec", "ent_vec"] {
                if !have.contains(rel) {
                    self.run(
                        &format!(":create {rel} {{id: String => vec: <F32; {dim}>}}"),
                        BTreeMap::new(),
                        true,
                    )?;
                    self.run(
                        &format!(
                            "::hnsw create {rel}:idx {{dim: {dim}, dtype: F32, fields: [vec], distance: Cosine, m: 16, ef_construction: 32}}"
                        ),
                        BTreeMap::new(),
                        true,
                    )?;
                }
            }
        }
        Ok(())
    }

    /// f32 벡터를 CozoScript 리스트 리터럴 `[a,b,c]` 로 만든다. 값은 우리 f32 라 인젝션 위험 없음.
    /// :put 시 `<F32; N>` 컬럼으로 coerce 되고, 질의에서는 `vec([..])` 로 감싸 쓴다.
    fn list_literal(v: &[f32]) -> String {
        let mut s = String::from("[");
        for (i, x) in v.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            // 완전 정밀도로 왕복 가능한 표기.
            s.push_str(&format!("{x:?}"));
        }
        s.push(']');
        s
    }

    /// HNSW 인덱스로 근사 최근접(ANN) 검색. 관측(obs_vec/observation/content)과
    /// 엔티티(ent_vec/entity/name)가 같은 로직을 공유한다. 워크스페이스 필터는 조인 후
    /// 적용하므로 후보(k)를 limit 보다 넉넉히 잡아 필터로 잘려도 limit 를 채운다.
    fn semantic_hnsw(
        &self,
        query: &[f32],
        workspace: Option<&str>,
        limit: usize,
        target: &SemanticTarget,
    ) -> Vec<SearchHit> {
        let (index, relation, text_field, kind) =
            (target.index, target.relation, target.text_field, target.kind);
        let k = (limit * 4).max(16);
        let ef = (k * 2).max(32);
        let q = Self::list_literal(query);
        let mut p = params(&[]);
        // text_field 를 text 로 바인딩해 관측 content / 엔티티 name 을 한 형태로 다룬다.
        let script = match workspace {
            Some(ws) => {
                p.insert("ws".to_string(), DataValue::from(ws));
                format!(
                    "?[id, text, dist] := qv = vec({q}), ~{index}{{id | query: qv, k: {k}, ef: {ef}, bind_distance: dist}}, *{relation}{{id, {text_field}: text, workspace}}, workspace == $ws\n\
                     :order dist, id\n\
                     :limit {limit}"
                )
            }
            None => format!(
                "?[id, text, dist] := qv = vec({q}), ~{index}{{id | query: qv, k: {k}, ef: {ef}, bind_distance: dist}}, *{relation}{{id, {text_field}: text}}\n\
                 :order dist, id\n\
                 :limit {limit}"
            ),
        };
        let rows = match self.run(&script, p, false) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };
        rows.rows
            .iter()
            .filter_map(|r| {
                let id = r.first()?.get_str()?;
                let text = r.get(1)?.get_str()?;
                // Cozo Cosine 거리 = 1 - 코사인 유사도. 유사도로 되돌려 score 로 쓴다.
                let dist = r.get(2)?.get_float()? as f32;
                Some(SearchHit {
                    kind,
                    id: id.to_string(),
                    snippet: text.chars().take(160).collect(),
                    score: 1.0 - dist,
                })
            })
            .collect()
    }

    /// 관측 로그에서 id 로 관측을 복원한다 (재도착 병합의 기준 + 검사/테스트용).
    /// data JSON 에서 provenance(attestation 목록)/assertions/derived_from/embedding 을 되살린다.
    pub fn observation(&self, id: &str) -> Option<Observation> {
        let p = params(&[("id", id.to_string())]);
        let rows = self
            .run(
                "?[content, data] := *observation{id: $id, content, data}",
                p,
                false,
            )
            .ok()?;
        let row = rows.rows.into_iter().next()?;
        let content = row.first()?.get_str()?.to_string();
        let data: serde_json::Value = serde_json::from_str(row.get(1)?.get_str()?).ok()?;
        let provenance: Vec<Provenance> =
            serde_json::from_value(data.get("provenance")?.clone()).ok()?;
        let assertions: Assertions = data
            .get("assertions")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        let derived_from: Vec<String> = data
            .get("derived_from")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        let embedding: Option<Vec<f32>> = data
            .get("embedding")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        Some(Observation {
            id: id.to_string(),
            content,
            provenance,
            assertions,
            derived_from,
            embedding,
        })
    }

    /// 벡터 인덱스가 없을 때의 브루트포스 시맨틱 검색. data JSON 의 embedding 을 로드해
    /// Rust 에서 코사인 유사도로 랭킹한다. 관측(observation/content)과 엔티티(entity/name)가
    /// 공유하며, 두 관계 모두 workspace 컬럼이 있어 스토어에서 바로 필터한다.
    fn semantic_brute(
        &self,
        query: &[f32],
        workspace: Option<&str>,
        limit: usize,
        target: &SemanticTarget,
    ) -> Vec<SearchHit> {
        let (relation, text_field, kind) = (target.relation, target.text_field, target.kind);
        let mut p = params(&[]);
        let script = match workspace {
            Some(ws) => {
                p.insert("ws".to_string(), DataValue::from(ws));
                format!(
                    "?[id, text, data] := *{relation}{{id, {text_field}: text, workspace, data}}, workspace == $ws"
                )
            }
            None => {
                format!("?[id, text, data] := *{relation}{{id, {text_field}: text, data}}")
            }
        };
        let rows = match self.run(&script, p, false) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };
        let mut hits: Vec<SearchHit> = rows
            .rows
            .iter()
            .filter_map(|r| {
                let id = r.first()?.get_str()?;
                let text = r.get(1)?.get_str()?;
                let data: serde_json::Value = serde_json::from_str(r.get(2)?.get_str()?).ok()?;
                let emb: Vec<f32> = serde_json::from_value(data.get("embedding")?.clone()).ok()?;
                Some(SearchHit {
                    kind,
                    id: id.to_string(),
                    snippet: text.chars().take(160).collect(),
                    score: cosine_similarity(query, &emb),
                })
            })
            .collect();

        // 동점은 id 로 안정 정렬 - 질의 행 순서가 결과에 새지 않게 한다(원칙 16: 재현성).
        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        });
        hits.truncate(limit);
        hits
    }
}

/// (키, 문자열값) 쌍을 CozoScript 파라미터 맵으로. (파라미터화로 인젝션 방지)
fn params(pairs: &[(&str, String)]) -> BTreeMap<String, DataValue> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), DataValue::from(v.as_str())))
        .collect()
}

/// 시맨틱 검색 대상 기술자: 어느 HNSW 인덱스/stored relation/텍스트 컬럼을 어떤 히트 종류로
/// 볼지. 관측(obs_vec/observation/content)과 엔티티(ent_vec/entity/name)가 같은 검색 로직을
/// 이 기술자만 바꿔 공유한다.
struct SemanticTarget {
    /// HNSW 인덱스 이름 (브루트포스 경로는 무시).
    index: &'static str,
    /// stored relation 이름.
    relation: &'static str,
    /// 스니펫 원천이 되는 텍스트 컬럼(관측 content / 엔티티 name).
    text_field: &'static str,
    /// 결과 히트의 종류.
    kind: SearchHitKind,
}

const OBS_TARGET: SemanticTarget = SemanticTarget {
    index: "obs_vec:idx",
    relation: "observation",
    text_field: "content",
    kind: SearchHitKind::Observation,
};

const ENT_TARGET: SemanticTarget = SemanticTarget {
    index: "ent_vec:idx",
    relation: "entity",
    text_field: "name",
    kind: SearchHitKind::Entity,
};

/// entity 행(id, etype, name, data JSON)을 도메인 Entity 로 복원한다. 조회/열거가 공유.
/// data JSON 이 깨져도 스키마 컬럼(id/type/name)은 살리고 복합 필드만 비운다(방어적).
fn entity_from_parts(id: String, etype: String, name: String, data_str: &str) -> Entity {
    let data: serde_json::Value =
        serde_json::from_str(data_str).unwrap_or(serde_json::Value::Null);
    Entity {
        id,
        kind: etype,
        canonical_name: name,
        aliases: data
            .get("aliases")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default(),
        properties: data
            .get("properties")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
        provenance: data
            .get("provenance")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default(),
        // 임베딩을 복원해 업서트(get -> put) 왕복에서 소실되지 않게 한다. 없으면(구 스키마) None.
        embedding: data
            .get("embedding")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default(),
    }
}

/// relation 행(id, src, dst, rtype, data JSON)을 도메인 Relation 으로 복원한다.
/// provenance 파싱이 실패하면 None - provenance 없는 관계는 원칙 2 위반이라 흘리지 않는다.
fn relation_from_parts(
    id: &str,
    src: &str,
    dst: &str,
    rtype: &str,
    data_str: &str,
) -> Option<Relation> {
    let data: serde_json::Value = serde_json::from_str(data_str).ok()?;
    Some(Relation {
        id: id.to_string(),
        from: src.to_string(),
        to: dst.to_string(),
        kind: rtype.to_string(),
        provenance: serde_json::from_value(data.get("provenance")?.clone()).ok()?,
        valid_from: data.get("valid_from").and_then(|v| v.as_u64()),
        valid_to: data.get("valid_to").and_then(|v| v.as_u64()),
    })
}

impl KnowledgeStore for CozoStore {
    fn add_observation(&self, obs: Observation) -> Result<(), StoreError> {
        // 같은 콘텐츠 주소의 재도착은 덮어쓰기가 아니라 단조 합집합으로 흡수한다
        // (원칙 3: 로그 불변). 기존 행을 읽어 attestation/계보를 병합한 뒤 쓴다.
        let obs = match self.observation(&obs.id) {
            Some(mut existing) => {
                existing.absorb(obs);
                existing
            }
            None => obs,
        };
        let workspace = obs.workspace().to_string();
        let obs_id = obs.id.clone();
        let embedding = obs.embedding.clone();
        // 복합 필드는 data JSON 컬럼에 (provenance + 주장 + derived_from 계보 + 임베딩).
        // assertions 는 재프로젝션의 입력이므로 반드시 로그와 함께 영속한다 (원칙 1).
        // 임베딩은 회상 보조일 뿐 정체성이 아니다(원칙 19) - 스키마 컬럼이 아닌 data 에 둔다.
        let data = json!({
            "provenance": obs.provenance,
            "assertions": obs.assertions,
            "derived_from": obs.derived_from,
            "embedding": obs.embedding,
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

        // 벡터 인덱스가 켜져 있고 임베딩 차원이 맞으면 obs_vec(HNSW)에도 적재한다.
        // data JSON 의 임베딩이 원천이고, obs_vec 은 ANN 가속을 위한 물질화 인덱스다.
        if let (Some(dim), Some(emb)) = (self.vector_dim, embedding) {
            if emb.len() == dim {
                let script = format!(
                    "?[id, vec] := id = $id, vec = {}\n:put obs_vec {{id => vec}}",
                    Self::list_literal(&emb)
                );
                self.run(&script, params(&[("id", obs_id)]), true)?;
            }
        }
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
        let data_str = row.get(2)?.get_str()?;
        Some(entity_from_parts(id.to_string(), etype, name, data_str))
    }

    fn put_entity(&self, entity: Entity) -> Result<(), StoreError> {
        let workspace = entity
            .provenance
            .first()
            .map(|p| p.workspace.clone())
            .unwrap_or_default();
        let embedding = entity.embedding.clone();
        let entity_id = entity.id.clone();
        // 임베딩은 회상 보조일 뿐 정체성이 아니다(원칙 19) - 스키마 컬럼이 아닌 data 에 둔다.
        // data JSON 이 원천이고 ent_vec 은 ANN 가속을 위한 물질화 인덱스다(obs_vec 와 대칭).
        let data = json!({
            "aliases": entity.aliases,
            "properties": entity.properties,
            "provenance": entity.provenance,
            "embedding": entity.embedding,
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

        // 벡터 인덱스가 켜져 있고 임베딩 차원이 맞으면 ent_vec(HNSW)에도 적재한다.
        if let (Some(dim), Some(emb)) = (self.vector_dim, embedding) {
            if emb.len() == dim {
                let script = format!(
                    "?[id, vec] := id = $id, vec = {}\n:put ent_vec {{id => vec}}",
                    Self::list_literal(&emb)
                );
                self.run(&script, params(&[("id", entity_id)]), true)?;
            }
        }
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
        let mut rels: Vec<Relation> = rows
            .rows
            .iter()
            .filter_map(|r| {
                relation_from_parts(
                    r.first()?.get_str()?,
                    r.get(1)?.get_str()?,
                    r.get(2)?.get_str()?,
                    r.get(3)?.get_str()?,
                    r.get(4)?.get_str()?,
                )
            })
            .collect();
        // id 정렬 - 행 순서가 응답에 새지 않게 명시한다 (InMemory 와 parity, 원칙 16).
        rels.sort_by(|a, b| a.id.cmp(&b.id));
        rels
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
            None => {
                "?[id, content] := *observation{id, content}, str_includes(lowercase(content), $q)"
            }
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

        // 동점은 id 로 안정 정렬 - 질의 행 순서가 결과에 새지 않게 한다(원칙 16: 재현성).
        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        });
        hits.truncate(limit);
        hits
    }

    fn traverse(&self, start_id: &str, max_depth: usize, limit: usize) -> Vec<TraverseHit> {
        let md = max_depth.max(1);
        // 재귀 Datalog: min 집계로 최단 홉 거리를 구하고 엔티티 메타를 조인.
        // :order 없이는 결과가 id(첫 컬럼) 순으로 나와 :limit 절단이 depth 를 무시했다
        // - (depth, id) 로 명시 정렬해 가까운 이웃 우선 + 재현 가능한 절단으로 만들고
        // InMemory 어댑터와 절단 의미론을 일치시킨다 (원칙 16).
        let script = format!(
            "reach[dst, min(d)] := *relation{{src: $start, dst}}, d = 1\n\
             reach[dst, min(d)] := reach[nid, d0], *relation{{src: nid, dst}}, d = d0 + 1, d <= {md}\n\
             ?[id, depth, name, etype] := reach[id, depth], *entity{{id, name, etype}}\n\
             :order depth, id\n\
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

    fn all_entities(&self, workspace: Option<&str>) -> Vec<Entity> {
        // entity 테이블은 workspace 컬럼을 지니므로 스토어에서 바로 필터한다.
        let mut p = params(&[]);
        let script = match workspace {
            Some(ws) => {
                p.insert("ws".to_string(), DataValue::from(ws));
                "?[id, etype, name, data] := *entity{id, etype, name, workspace, data}, workspace == $ws"
            }
            None => "?[id, etype, name, data] := *entity{id, etype, name, data}",
        };
        let rows = match self.run(script, p, false) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };
        rows.rows
            .iter()
            .filter_map(|r| {
                Some(entity_from_parts(
                    r.first()?.get_str()?.to_string(),
                    r.get(1)?.get_str()?.to_string(),
                    r.get(2)?.get_str()?.to_string(),
                    r.get(3)?.get_str()?,
                ))
            })
            .collect()
    }

    fn all_relations(&self, workspace: Option<&str>) -> Vec<Relation> {
        // relation 테이블엔 workspace 컬럼이 없다 - 워크스페이스는 data JSON 의
        // provenance.workspace 에 있으므로 복원 후 Rust 에서 필터한다.
        let rows = match self.run(
            "?[id, src, dst, rtype, data] := *relation{id, src, dst, rtype, data}",
            params(&[]),
            false,
        ) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };
        rows.rows
            .iter()
            .filter_map(|r| {
                relation_from_parts(
                    r.first()?.get_str()?,
                    r.get(1)?.get_str()?,
                    r.get(2)?.get_str()?,
                    r.get(3)?.get_str()?,
                    r.get(4)?.get_str()?,
                )
            })
            .filter(|rel| workspace.is_none_or(|ws| rel.provenance.workspace == ws))
            .collect()
    }

    fn search_semantic(
        &self,
        query_embedding: &[f32],
        workspace: Option<&str>,
        limit: usize,
    ) -> Vec<SearchHit> {
        // 벡터 인덱스가 켜져 있으면 네이티브 HNSW ANN 으로.
        if self.vector_dim.is_some() {
            return self.semantic_hnsw(query_embedding, workspace, limit, &OBS_TARGET);
        }
        // 아니면 브루트포스: 후보를 로드해 Rust 에서 코사인 유사도로 랭킹한다.
        self.semantic_brute(query_embedding, workspace, limit, &OBS_TARGET)
    }

    fn search_semantic_entities(
        &self,
        query_embedding: &[f32],
        workspace: Option<&str>,
        limit: usize,
    ) -> Vec<SearchHit> {
        // 엔티티도 관측과 대칭: HNSW(ent_vec) 가속, 없으면 브루트포스 코사인.
        if self.vector_dim.is_some() {
            return self.semantic_hnsw(query_embedding, workspace, limit, &ENT_TARGET);
        }
        self.semantic_brute(query_embedding, workspace, limit, &ENT_TARGET)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InMemoryStore;
    use supragnosis_core::Provenance;

    fn tmp_dir() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        // 프로세스 원자 카운터로 유일성을 확정한다 - 벽시계 해상도에 기대면 동시 실행되는
        // 두 Cozo 테스트가 같은 나노초에 같은 경로를 잡아 RocksDB 락 충돌로 open 이 실패한다.
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "supragnosis-cozo-{}-{nanos}-{seq}",
            std::process::id()
        ))
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
            embedding: None,
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
            assert!(
                ids.contains(&b) && ids.contains(&c),
                "traverse got {hits:?}"
            );
        }

        // 영속성: 재오픈 후에도 데이터 유지
        {
            let store2 = CozoStore::open(&dir).unwrap();
            let a = Entity::make_id("ws1", "a");
            assert!(store2.get_entity(&a).is_some(), "data should persist");
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// traverse 는 (depth asc, id asc)로 결정적이고 limit 절단은 얕은 depth 우선이다 -
    /// 두 어댑터의 순서/절단 의미론이 일치한다 (원칙 16, 검색 정렬 tie-break 의 잔여).
    /// 픽스처는 해시 선별: id 최소 후보를 depth-2 손자로 둬, id 순 절단이던 회귀라면
    /// 손자가 depth-1 이웃을 밀어냈을 조건을 만든다.
    #[test]
    fn traverse_order_and_truncation_parity_across_adapters() {
        let mut cands: Vec<String> = (0..30).map(|i| format!("child-{i:02}")).collect();
        cands.sort_by_key(|n| Entity::make_id("ws1", n));
        let grand = cands[0].clone();
        let children: Vec<String> = cands[cands.len() - 6..].to_vec();
        assert!(
            Entity::make_id("ws1", &grand) < Entity::make_id("ws1", &children[0]),
            "픽스처 전제: 손자 id 가 모든 자식 id 보다 작다"
        );

        let fill = |store: &dyn KnowledgeStore| {
            store.put_entity(ent("root")).unwrap();
            for name in &children {
                store.put_entity(ent(name)).unwrap();
                let (f, t) = (Entity::make_id("ws1", "root"), Entity::make_id("ws1", name));
                store
                    .add_relation(Relation {
                        id: Relation::make_id(&f, "rel", &t),
                        from: f,
                        to: t,
                        kind: "rel".into(),
                        provenance: prov(),
                        valid_from: None,
                        valid_to: None,
                    })
                    .unwrap();
            }
            store.put_entity(ent(&grand)).unwrap();
            let (f, t) = (
                Entity::make_id("ws1", &children[0]),
                Entity::make_id("ws1", &grand),
            );
            store
                .add_relation(Relation {
                    id: Relation::make_id(&f, "rel", &t),
                    from: f,
                    to: t,
                    kind: "rel".into(),
                    provenance: prov(),
                    valid_from: None,
                    valid_to: None,
                })
                .unwrap();
        };
        let check = |store: &dyn KnowledgeStore, label: &str| -> Vec<(usize, String)> {
            let root = Entity::make_id("ws1", "root");
            let full = store.traverse(&root, 5, 100);
            let keys: Vec<(usize, String)> =
                full.iter().map(|h| (h.depth, h.id.clone())).collect();
            let mut sorted = keys.clone();
            sorted.sort();
            assert_eq!(keys, sorted, "{label}: (depth, id) 순서여야 한다");
            // 절단은 가까운 이웃 우선 - id 최소인 손자(depth 2)가 depth-1 을 밀어내지 않는다.
            let cut = store.traverse(&root, 5, 4);
            assert!(
                cut.iter().all(|h| h.depth == 1),
                "{label}: limit 절단은 얕은 depth 우선이어야 한다: {cut:?}"
            );
            keys
        };

        // InMemory: 인스턴스(HashMap 시드)와 무관하게 같은 결과.
        let m1 = InMemoryStore::new();
        fill(&m1);
        let k1 = check(&m1, "mem-1");
        let m2 = InMemoryStore::new();
        fill(&m2);
        let k2 = check(&m2, "mem-2");
        assert_eq!(k1, k2, "InMemory 인스턴스 간 재현성");

        // Cozo: 같은 순서/절단 의미론 (어댑터 parity).
        let dir = tmp_dir();
        {
            let store = CozoStore::open(&dir).unwrap();
            fill(&store);
            let kc = check(&store, "cozo");
            assert_eq!(k1, kc, "InMemory <-> Cozo parity");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 원칙 3: Cozo 어댑터도 재도착을 합집합으로 흡수하고, 병합 결과가 영속한다.
    #[test]
    fn cozo_reobservation_accumulates_attestations() {
        let dir = tmp_dir();
        let make = |host: &str, conf: f32, derived: &str| {
            let mut o = Observation::new(
                "same fact".into(),
                Provenance {
                    host: host.into(),
                    confidence: conf,
                    ..prov()
                },
            );
            o.derived_from = vec![derived.into()];
            o
        };
        let id = make("host-a", 0.9, "o1").id;
        {
            let store = CozoStore::open(&dir).unwrap();
            store.add_observation(make("host-a", 0.9, "o1")).unwrap();
            store.add_observation(make("host-b", 0.1, "o2")).unwrap();

            let o = store.observation(&id).unwrap();
            assert_eq!(o.provenance.len(), 2, "attestation 누적: {:?}", o.provenance);
            assert_eq!(o.derived_from, vec!["o1".to_string(), "o2".to_string()]);
        }
        // 병합 결과가 재오픈 후에도 유지된다.
        {
            let store = CozoStore::open(&dir).unwrap();
            let o = store.observation(&id).unwrap();
            assert_eq!(o.provenance.len(), 2);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn prov_ws(ws: &str) -> Provenance {
        Provenance {
            workspace: ws.into(),
            ..prov()
        }
    }

    fn obs_with_emb(content: &str, ws: &str, emb: [f32; 3]) -> Observation {
        let mut o = Observation::new(content.into(), prov_ws(ws));
        o.embedding = Some(emb.to_vec());
        o
    }

    /// 벡터 차원을 지정해 열면 의미 검색이 네이티브 HNSW 를 쓴다. 최근접 순위 + 워크스페이스
    /// 필터를 검증하고, 재오픈 후에도 인덱스가 유지되는지 확인한다.
    #[test]
    fn cozo_semantic_search_uses_hnsw() {
        let dir = tmp_dir();
        let a = Observation::new("x axis".into(), prov_ws("ws1")).id;
        let c = Observation::new("near x".into(), prov_ws("ws1")).id;
        {
            let store = CozoStore::open_with_embedding_dim(&dir, 3).unwrap();
            store
                .add_observation(obs_with_emb("x axis", "ws1", [1.0, 0.0, 0.0]))
                .unwrap();
            store
                .add_observation(obs_with_emb("y axis", "ws1", [0.0, 1.0, 0.0]))
                .unwrap();
            store
                .add_observation(obs_with_emb("near x", "ws1", [0.9, 0.1, 0.0]))
                .unwrap();
            // 다른 워크스페이스의 완전 일치 - ws1 질의에 나오면 안 된다.
            store
                .add_observation(obs_with_emb("other x", "ws2", [1.0, 0.0, 0.0]))
                .unwrap();

            let hits = store.search_semantic(&[1.0, 0.0, 0.0], Some("ws1"), 2);
            let ids: Vec<String> = hits.iter().map(|h| h.id.clone()).collect();
            assert_eq!(ids.len(), 2, "top-2 within ws1, got {ids:?}");
            assert!(
                ids.contains(&a) && ids.contains(&c),
                "nearest should be x/near-x, got {ids:?}"
            );
            // 최근접(완전 일치)이 1위.
            assert_eq!(hits[0].id, a, "exact match should rank first");
        }

        // 재오픈: HNSW 인덱스와 벡터가 영속한다.
        {
            let store = CozoStore::open_with_embedding_dim(&dir, 3).unwrap();
            let hits = store.search_semantic(&[1.0, 0.0, 0.0], Some("ws1"), 1);
            assert_eq!(hits.first().map(|h| h.id.clone()), Some(a.clone()));
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn ent_with_emb(name: &str, ws: &str, emb: [f32; 3]) -> Entity {
        Entity {
            id: Entity::make_id(ws, name),
            kind: "Concept".into(),
            canonical_name: name.into(),
            aliases: vec![],
            properties: serde_json::Value::Null,
            provenance: vec![prov_ws(ws)],
            embedding: Some(emb.to_vec()),
        }
    }

    /// 엔티티 시맨틱 검색(ent_vec HNSW): 이름 임베딩으로 노드에 도달한다. 최근접 순위 +
    /// 워크스페이스 필터 + 재오픈 영속을 관측 경로(obs_vec)와 대칭으로 검증한다.
    #[test]
    fn cozo_entity_semantic_search_uses_hnsw() {
        let dir = tmp_dir();
        let x = Entity::make_id("ws1", "x axis");
        let near = Entity::make_id("ws1", "near x");
        {
            let store = CozoStore::open_with_embedding_dim(&dir, 3).unwrap();
            store.put_entity(ent_with_emb("x axis", "ws1", [1.0, 0.0, 0.0])).unwrap();
            store.put_entity(ent_with_emb("y axis", "ws1", [0.0, 1.0, 0.0])).unwrap();
            store.put_entity(ent_with_emb("near x", "ws1", [0.9, 0.1, 0.0])).unwrap();
            // 다른 워크스페이스의 완전 일치 - ws1 질의에 나오면 안 된다.
            store.put_entity(ent_with_emb("other x", "ws2", [1.0, 0.0, 0.0])).unwrap();

            let hits = store.search_semantic_entities(&[1.0, 0.0, 0.0], Some("ws1"), 2);
            let ids: Vec<String> = hits.iter().map(|h| h.id.clone()).collect();
            assert_eq!(ids.len(), 2, "top-2 within ws1, got {ids:?}");
            assert!(
                ids.contains(&x) && ids.contains(&near),
                "nearest should be x/near-x entities, got {ids:?}"
            );
            assert_eq!(hits[0].id, x, "exact-match entity should rank first");
            // 엔티티 히트여야 한다(관측과 섞이지 않음).
            assert!(hits.iter().all(|h| h.kind == SearchHitKind::Entity));
        }

        // 재오픈: ent_vec 인덱스와 벡터가 영속한다.
        {
            let store = CozoStore::open_with_embedding_dim(&dir, 3).unwrap();
            let hits = store.search_semantic_entities(&[1.0, 0.0, 0.0], Some("ws1"), 1);
            assert_eq!(hits.first().map(|h| h.id.clone()), Some(x.clone()));
        }

        let _ = std::fs::remove_dir_all(&dir);
    }
}
