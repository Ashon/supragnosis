//! The resources: a URI read, and the URI forms parsed.

use super::*;

/// One resource read: the URI's content, or the error that stands for it.
pub(crate) fn read_uri(engine: &Engine, uri: String) -> Result<ReadResourceResult, ErrorData> {
    match parse_resource_uri(&uri) {
        // Workspace list (for discovery) - array of workspace names that hold knowledge.
        Some(ResourceUri::Workspaces) => match engine.workspaces() {
            Ok(list) => {
                Ok(ReadResourceResult::new(vec![ResourceContents::text(to_json(&list), uri)]))
            }
            Err(e) => Err(ErrorData::internal_error(
                format!("storage backend failure (not a missing resource): {e}"),
                None,
            )),
        },
        Some(ResourceUri::Graph(ws)) => {
            match engine.graph(Some(ws).filter(|w| !is_all(w))) {
                Ok(graph) => {
                    Ok(ReadResourceResult::new(vec![ResourceContents::text(to_json(&graph), uri)]))
                }
                // A backend failure is surfaced as an internal error, not not_found (absence) - preserving the Principle 5 distinction.
                Err(e) => Err(ErrorData::internal_error(
                    format!("storage backend failure (not a missing resource): {e}"),
                    None,
                )),
            }
        }
        // Co-occurrence second-order structure (Principle 11): the entity sets an observation asserted together, as hyperedges.
        Some(ResourceUri::Hypergraph(ws)) => match engine
            .hypergraph(Some(ws).filter(|w| !is_all(w)))
        {
            Ok(hg) => Ok(ReadResourceResult::new(vec![ResourceContents::text(to_json(&hg), uri)])),
            Err(e) => Err(ErrorData::internal_error(
                format!("storage backend failure (not a missing resource): {e}"),
                None,
            )),
        },
        // T-Box glossary (Principle 8/11): the workspace's type definitions (entity + relation types + meanings).
        Some(ResourceUri::Types(ws)) => match engine.types(Some(ws).filter(|w| !is_all(w))) {
            Ok(types) => {
                Ok(ReadResourceResult::new(vec![ResourceContents::text(to_json(&types), uri)]))
            }
            Err(e) => Err(ErrorData::internal_error(
                format!("storage backend failure (not a missing resource): {e}"),
                None,
            )),
        },
        // Observation back-reference (Principles 2/14): whoever knows the id can query the substance (raw content/provenance/lineage).
        Some(ResourceUri::Observation(id)) => match engine.get_observation(id) {
            Ok(Some(obs)) => {
                Ok(ReadResourceResult::new(vec![ResourceContents::text(to_json(&obs), uri)]))
            }
            Ok(None) => Err(unknown_observation(id)),
            Err(e) => Err(ErrorData::internal_error(
                format!("storage backend failure (not a missing resource): {e}"),
                None,
            )),
        },
        // An entity by its id (Principle 14), as get_entity answers it.
        // A proposal by its id (Principle 14), read where it lives.
        Some(ResourceUri::Proposal(id)) => match engine.get_proposal(None, id) {
            Ok(Some(view)) => {
                Ok(ReadResourceResult::new(vec![ResourceContents::text(to_json(&view), uri)]))
            }
            Ok(None) => Err(unknown_proposal_resource(id)),
            Err(e) => Err(ErrorData::internal_error(
                format!("storage backend failure (not a missing resource): {e}"),
                None,
            )),
        },
        Some(ResourceUri::Entity(id)) => match engine.get_entity(id) {
            Ok(Some(view)) => {
                Ok(ReadResourceResult::new(vec![ResourceContents::text(to_json(&view), uri)]))
            }
            Ok(None) => Err(unknown_entity_resource(id)),
            Err(e) => Err(ErrorData::internal_error(
                format!("storage backend failure (not a missing resource): {e}"),
                None,
            )),
        },
        None => Err(ErrorData::resource_not_found(
            format!(
                "unknown resource URI: {uri} - the forms are supragnosis://workspaces, \
                 supragnosis://workspace/{{workspace}}/graph, .../hypergraph and .../types, \
                 supragnosis://observation/{{id}}, supragnosis://entity/{{id}} and \
                 supragnosis://proposal/{{id}}"
            ),
            None,
        )),
    }
}

/// Kinds of resource URI.
enum ResourceUri<'a> {
    /// `supragnosis://workspaces` - list of workspaces that hold knowledge (intro/discovery).
    Workspaces,
    /// `supragnosis://workspace/<ws>/graph` - workspace ontology graph.
    Graph(&'a str),
    /// `supragnosis://workspace/<ws>/hypergraph` - co-occurrence second-order structure (Principle 11 second-order structure).
    Hypergraph(&'a str),
    /// `supragnosis://workspace/<ws>/types` - the T-Box type glossary (Principle 8/11).
    Types(&'a str),
    /// `supragnosis://observation/<id>` - observation back-reference (Principles 2/14).
    Observation(&'a str),
    /// `supragnosis://entity/<id>` - an entity, as get_entity answers it (Principle 14).
    Entity(&'a str),
    /// `supragnosis://proposal/<id>` - a proposal, read where it lives (Principle 14).
    Proposal(&'a str),
}

/// The mark an index line puts after an observation's id (docs/recall.md Section 7.1, D12). The
/// record read is the same with or without it; the usage log notes that it was found in an index.
const VIA_INDEX: &str = "?via=index";

/// An id as `derived_from` keeps it: an agent that copies a record's link from an index line or a
/// search hit names the record, so the link's scheme and the index mark are taken off (docs/recall.md
/// Section 7.1). Anything else is kept as written.
pub(crate) fn lineage_id(given: &str) -> String {
    let id = given.trim();
    let id = id.strip_prefix("supragnosis://observation/").unwrap_or(id);
    id.strip_suffix(VIA_INDEX).unwrap_or(id).to_string()
}

/// A resource read's kind, the workspace its URI names, and `index` when an index line gave the
/// URI, for the usage log (docs/usage.md): the kind's word only, never an id (UL2).
pub(crate) fn resource_use(uri: &str) -> (&'static str, Option<String>, Option<&'static str>) {
    let (kind, workspace) = match parse_resource_uri(uri) {
        Some(ResourceUri::Workspaces) => ("workspaces", None),
        Some(ResourceUri::Graph(ws)) => ("graph", Some(ws.to_string())),
        Some(ResourceUri::Hypergraph(ws)) => ("hypergraph", Some(ws.to_string())),
        Some(ResourceUri::Types(ws)) => ("types", Some(ws.to_string())),
        Some(ResourceUri::Observation(_)) => ("observation", None),
        Some(ResourceUri::Entity(_)) => ("entity", None),
        Some(ResourceUri::Proposal(_)) => ("proposal", None),
        None => ("unknown", None),
    };
    let via = (kind == "observation" && uri.ends_with(VIA_INDEX)).then_some("index");
    (kind, workspace, via)
}

/// Resource URI parser. Returns None on malformed input. A segment must not contain `/`.
fn parse_resource_uri(uri: &str) -> Option<ResourceUri<'_>> {
    // Exact match first - "workspaces" (plural) does not overlap the "workspace/" (singular+slash) prefix.
    if uri == "supragnosis://workspaces" {
        return Some(ResourceUri::Workspaces);
    }
    if let Some(rest) = uri.strip_prefix("supragnosis://workspace/") {
        // Check "/hypergraph" first - "hypergraph" does not end with "/graph" (preceded by 'r'),
        // but we keep the explicit order to make the intent clear. Reject if a segment contains '/' (single segment).
        if let Some(ws) = rest.strip_suffix("/hypergraph") {
            if ws.is_empty() || ws.contains('/') {
                return None;
            }
            return Some(ResourceUri::Hypergraph(ws));
        }
        if let Some(ws) = rest.strip_suffix("/graph") {
            if ws.is_empty() || ws.contains('/') {
                return None;
            }
            return Some(ResourceUri::Graph(ws));
        }
        if let Some(ws) = rest.strip_suffix("/types") {
            if ws.is_empty() || ws.contains('/') {
                return None;
            }
            return Some(ResourceUri::Types(ws));
        }
        return None;
    }
    if let Some(id) = uri.strip_prefix("supragnosis://observation/") {
        let id = id.strip_suffix(VIA_INDEX).unwrap_or(id);
        if id.is_empty() || id.contains(['/', '?', '#']) {
            return None;
        }
        return Some(ResourceUri::Observation(id));
    }
    if let Some(id) = uri.strip_prefix("supragnosis://entity/") {
        if id.is_empty() || id.contains('/') {
            return None;
        }
        return Some(ResourceUri::Entity(id));
    }
    if let Some(id) = uri.strip_prefix("supragnosis://proposal/") {
        if id.is_empty() || id.contains('/') {
            return None;
        }
        return Some(ResourceUri::Proposal(id));
    }
    None
}

#[cfg(test)]
mod usage_tests {
    use super::{lineage_id, parse_resource_uri, resource_use, ResourceUri};

    /// recall.md Section 7.1: a record's link copied into `derived_from` names the record, and an
    /// observation URI takes the index mark and no other query.
    #[test]
    fn a_copied_link_names_its_record() {
        let id = "ab".repeat(32);
        for given in [
            id.clone(),
            format!("supragnosis://observation/{id}"),
            format!("supragnosis://observation/{id}?via=index"),
            format!("{id}?via=index"),
        ] {
            assert_eq!(lineage_id(&given), id, "{given}");
        }
        assert_eq!(lineage_id("something else"), "something else");
        let marked = format!("supragnosis://observation/{id}?via=index");
        assert!(
            matches!(parse_resource_uri(&marked), Some(ResourceUri::Observation(i)) if i == id)
        );
        assert_eq!(resource_use(&marked).2, Some("index"));
        for not in [format!("{marked}?via=index"), format!("supragnosis://observation/{id}?x=1")] {
            assert!(parse_resource_uri(&not).is_none(), "{not}");
        }
    }
}
