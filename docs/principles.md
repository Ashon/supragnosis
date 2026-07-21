# supragnosis - Design Principles

> This document is the **normative document** of supragnosis. Every design/implementation/review
> decision must be justified against these principles, and an expedient decision that conflicts
> with a principle is not permitted without a revision of this document. If `architecture.md` is
> "what we build and how", this document is "why it must be built that way".
>
> Each clause is stated in the order **Principle -> Rationale -> Enforcement in supragnosis (what it mandates)**.
>
> Revision: 2026-07 - bitemporality made explicit (4), delegation chain (2), forgetting/consolidation (7),
> contamination defense (18), schema induction (11), MCP long-running tasks/confirmation flow (21).
> See Appendix C references for rationale.
> Revision: 2026-07 (2nd) - Chapter 5 Governance added, Principle 23 (Gate to Canon) added.
> Concrete design in [proposal-workflow.md](proposal-workflow.md).
> Revision: 2026-07 (3rd) - Observation merge norm (3): boundaries of event identity and the
> re-arrival merge rule made explicit. Query-response determinism (16): the scope of Principle 16
> extended to the read path (ordering/truncation). Valid-interval capture (4): the default
> interpretation stated to be an approximation, and capture of retroactive observations required at
> the ingest surface. Absorbing property of tombstones (3), confidence convention (2), extension of
> knowledge sovereignty to the query surface (17), trust tier as the receiver's evaluation (18),
> non-delegability of the recall verdict (23).
> Revision: 2026-07 (4th) - Two layers of query determinism (16): reproducibility and convergence
> distinguished, and recall aids (embedding/ANN indexes) excluded from the convergence norm (with guards).
> Preservation of unspecified confidence (2): an unspecified value is not substituted with a default.
> Revision: 2026-07 (5th) - Well-formedness of assertions (1): the boundary of ingest validation
> (well-formedness only, no content censorship) made explicit. Mechanical enforcement of structural
> evolution (14): field enumeration in the identity/total-order/merge functions must force review at
> the compile level when a field is added.
> Revision: 2026-07 (6th) - Substrate of induction (11): the substrate of schema induction made
> explicit as the second-order structure of co-occurrence (hyperedges). Hyperedges are a deterministic
> input to systematization (induction 11 / resolution 15 / consolidation 7 / contradiction
> localization 6 / health metrics 22) but not its judge - commitment goes through the existing gates
> (deterministic resolution, proposal 23, human confirmation 18).

---

## Chapter 1 - The Nature of Knowledge (Epistemology)

Most errors in a system that handles knowledge arise not from the code but from **the premises about
what knowledge is**. This chapter fixes those premises.

### Principle 1. Separation of Assertion and Fact (Assertion-Belief Separation)

**What is stored is not fact but assertion.** The unit recorded in the graph is not "X is the case"
but "host H, at time T, on the basis of S, observed/asserted X". "Fact (the current belief)" is a
**derived view computed** by the resolution layer over the set of assertions, not a unit of storage.

- **Rationale**: People and agents write to the same graph. An agent's assertion can be a
  hallucination, and a person's assertion can be stale. If the judgment that promotes an assertion to
  fact is made at storage time, the basis of that judgment is lost and cannot be undone.
- **Enforcement**:
  - The observation log is the single source of truth, and the entity/relation graph is a
    materialized projection (event sourcing).
  - Resolution policy (latest-wins, confidence-weighted, etc.) must be a **replaceable strategy**;
    changing the policy must allow recomputing a different belief from the same log.
  - No API may write a "fact that did not pass through an assertion" directly into the graph.
  - **Ingest validation goes only as far as well-formedness - rejection is not transformation**: the
    ingest surface may validate and reject whether an assertion holds as an assertion (that the
    referent is non-empty, that a value is within its domain - isomorphic to enforcing the confidence
    range), and because of the permanence of the append-only log (Principle 3) this validation must
    happen before ingest. However, it must not reject an assertion on the grounds of its **content**
    (choice of notation, whether it is true, schema conformance) or transform it before placing it in
    the log - notational fluctuation is preserved verbatim and normalization is the job of the
    projection. An entity assertion with an empty name, or a relation type that is whitespace once
    normalized, is not a "differently-notated assertion" but a **non-assertion** with no referent, so
    rejecting it does not conflict with loose ingest (Principles 11/22) - looseness is tolerance of
    the degree of structuring, not an abandonment of well-formedness.

### Principle 2. Provenance Is a First-Class Citizen, Identity Is a Delegation Chain (Provenance First, Identity as Delegation Chain)

**Knowledge without provenance cannot exist.** Every observation/relation necessarily carries
(host, workspace, source_ref, observed_at, confidence). And **an agent usually acts on behalf of
someone** - the "who" of provenance must be expressible not as a flat host id but as a **delegation
chain** (e.g., `ashon -> claude-code@macbook`).

- **Rationale**: In a multi-host environment, the value of knowledge lies as much in "who said it and
  on what basis" as in the content itself. Provenance is a precondition for trust weighting, conflict
  resolution, audit, and after-the-fact redaction. Moreover, the shared concern of agent-identity
  standardization efforts (A2A, IETF agent-identity drafts) is that "an agent's self-declared identity
  cannot be verified for delegation" - provenance that cannot answer "under **whose authority** did
  this knowledge enter" is only half of provenance.
- **Enforcement**:
  - An observation without provenance is rejected at the ingest stage (schema-level enforcement).
  - The subject field of provenance must be able to express a delegation chain (the acting host +
    the `on_behalf_of` principal). A legacy/external observation with no chain information is recorded
    with the acting host alone, but is treated correspondingly lower in trust evaluation (Principle 18).
  - A query result must always be able to carry provenance - we do not build a query API that cannot
    answer "where did this answer come from".
  - The provenance authenticity of an event that passed through a relay/peer is guaranteed by the
    origin node's signature.
  - **Confidence convention**: confidence is in [0.0, 1.0] and its range is enforced at ingest
    (schema-level). Because self-reported confidence cannot be trusted for calibration - isomorphic to
    the problem of not being able to verify a self-declared identity - confidence is used in
    resolution/ranking only as a sub-signal of the trust tier (Principle 18). The rule for combining
    confidence across multiple attestations is not a replaceable discretion but a mandatory element of
    the resolution-policy specification. **The unspecified value is preserved**: storing an assertion
    that did not specify confidence by substituting a default (particularly 1.0) loses the distinction
    between "did not assert" and "asserted with full marks" - this is capture loss (separation of
    capture and processing, the caveat of Principle 4). The unspecified value is stored as unspecified,
    and its interpretation (what default weight to assign) is deferred to the resolution policy.

### Principle 3. Supersede Instead of Delete (Supersede, Don't Delete)

**Knowledge accumulates append-only, and stale knowledge is not erased but superseded.** Destructive
overwrite is forbidden.

- **Rationale**: Deletion destroys information, whereas superseding adds information ("this superseded
  that" is itself knowledge). Moreover, append-only immutable events are the precondition for
  content-address dedup and topology-independent replication convergence, so if this principle breaks,
  the entire sync model breaks.
- **Enforcement**:
  - The observation log is immutable / content-addressed (blake3). A modification is expressed as a
    new observation, and a withdrawal as an explicit retraction observation.
  - **Re-arrival merge norm**: an observation that arrives again under the same content address is not
    overwritten. Identity fields (content/assertion) have their sameness guaranteed by the id, while
    non-identity fields (provenance attestations, `derived_from` lineage) are merged by **monotonic
    set union** - union is commutative/associative/idempotent, so it converges regardless of arrival
    order (Principle 16). Relay duplication (a fully identical attestation) is naturally deduped by the
    union, while an independent re-observation (an attestation differing in any field) accumulates.
    This norm resolves the point of collision between "provenance is first-class" (Principle 2) and
    "content is identity" (Principle 14) - content is the observation's identity, and provenance is
    the set of attestations about that observation.
  - Entity merge also preserves history - un-merge must be possible.
  - There is exactly one exception, a **destruction demand due to regulation/privacy**, and even in
    this case a tombstone recording "what was destroyed" is left behind. A tombstone is an **absorbing
    state** for that id (the absorbing concept of Principle 16) - a node that holds a tombstone refuses
    re-ingest / sync re-receipt of that id and keeps the tombstone, and the tombstone itself propagates
    via sync. Without this, destroyed knowledge would come back from a peer that still holds it and be
    resurrected.

### Principle 4. Bitemporal - Two Time Axes (Bi-Temporality)

**All knowledge sits on two time axes.** (1) **valid time** - the period during which it was true in
the world (`valid_from`/`valid_to`), (2) **transaction time** - the moment the system came to know it
(`observed_at`). The base premise is that knowledge decays, and decay happens on both axes.

- **Rationale**: "Team lead Kim belongs to Team A" is true only until the reorganization (valid time),
  and we may have learned that fact later (transaction time). A knowledge base with only one axis
  cannot distinguish "what was true at T" from "what we knew at T". The bitemporal model is also the
  standard practice that recent agent-memory systems (Graphiti/Zep, etc.) have converged on - only
  with this distinction is retroactive correction (learning now something that was true in the past)
  expressible.
- **Enforcement**:
  - An observation must always be able to express transaction time, and a relation its valid interval.
    A relation with no valid interval is interpreted as "from the observation time until it is
    disproven". However, this default is an **approximation** that borrows transaction time for valid
    time - a retroactive observation (learning now something that was true in the past) must specify
    `valid_from`/`valid_to`, and the ingest surface must always be able to accept them (separation of
    capture and processing - query logic may be deferred, but if capture is deferred information is
    lost).
  - Support **both kinds of time-travel query**: `as_of_valid(T)` (the world that was true at T) and
    `as_of_recorded(T)` (the system's belief as of T). The append-only log (Principle 3) provides the
    latter for free.
  - When a new observation disproves an existing belief, the old relation is handled not by deletion
    but by closing its valid interval (setting `valid_to`).

### Principle 5. Open World Assumption (Open World Assumption)

**Absence from the graph is not falsehood.** Absence is not negation but unknown.

- **Rationale**: Distributed hosts each hold only partial knowledge, and a node before sync is always
  incomplete. The closed world assumption (CWA) holds only in a single complete DB.
- **Enforcement**:
  - Query APIs and inference rules do not treat "absent -> false" inference as the default. When
    negation is needed, it is expressed as an explicit negative assertion.
  - MCP tool responses convey a distinction between "not found" and "is false" - this is reflected in
    the response schema so that an LLM client does not misread absence as negation.

### Principle 6. Conflict Is Information (Contradiction Is Signal)

**Contradictory assertions are not suppressed but surfaced.** When hosts A and B make conflicting
assertions, both are preserved along with their provenance, and the resolution layer computes the
current belief while leaving the existence of the contradiction itself queryable.

- **Rationale**: In a multi-host environment, a contradiction is not an error but the most valuable
  signal that "confirmation is needed here". A system that automatically erases one side destroys the
  signal.
- **Enforcement**:
  - Resolution is a choice, not a deletion - a defeated assertion also remains in the log and can be
    reinstated upon re-resolution.
  - Provide an introspection query that lists "points currently in a contradictory state" (an entry
    point that invites a human host's confirmation/mediation).

### Principle 7. Forgetting Is Demotion, Consolidation Is Re-Projection (Forgetting as Demotion, Consolidation as Re-Projection)

**The lifecycle of knowledge does not end at ingest.** Stale or low-value knowledge is forgotten not
by deletion but by **demotion of recall priority**, and an idle-time **consolidation** process
summarizes/promotes/demotes accumulated observations. The log is eternal, but recall is finite.

- **Rationale**: An append-only log grows without bound, and recall without curation is polluted by
  noise. This is the solution that recent agent-memory research has converged on - reprocessing memory
  during idle time (sleep-time) outside the user-response path to improve its representation (a
  computational analogue of memory consolidation during sleep). Separating storage (Principle 3:
  preserve everything) from recall (finite attention) keeps the two demands from conflicting.
- **Enforcement**:
  - Forgetting does not touch the observation log - it only adjusts weights in the projection/index
    layer (recency, usage frequency, trust tier). Demoted knowledge is always reachable by an explicit
    query.
  - Consolidation is by default a **deterministic re-projection** (consistent with Principle 16). When
    a probabilistic consolidation such as LLM summarization is used, its output is ingested as a
    **derived observation** carrying confidence and `derived_from` lineage (subsumed under Principles
    1/18/19) - consolidation does not supersede the original.
  - Consolidation/re-indexing runs off the critical path of user requests (idle/batch).

---

## Chapter 2 - Ontology Design (Ontology Engineering)

Apply Gruber's (1995) ontology design principles and OntoClean (Guarino & Welty) to supragnosis's
T-Box/A-Box design.

### Principle 8. Clarity - One Meaning per Concept (Clarity)

**Every type/relation in the T-Box has a single, objective definition and a natural-language
description.** One name used for two meanings, or two names for one meaning, is not permitted.

- **Rationale**: The SRP of ontology. A concept with blurry meaning is used differently by each host,
  and the graph becomes not a set of connections but a graveyard of homonyms.
- **Enforcement**:
  - `define_type` cannot create a type without a natural-language definition (description).
  - A relation type's direction and meaning must be readable from its name (like `depends_on`). A
    catch-all relation of the `related` kind is limited to the single `relates_to`, and its use is
    treated as a temporary state meaning "not yet classified".

### Principle 9. Coherence - Contradiction-Free Inference (Coherence)

**No contradiction may be derivable from the schema and the inference rules.** Unlike Principle 6,
which permits assertion conflicts in the A-Box, the T-Box (schema) and the inference rules themselves
must be logically coherent.

- **Rationale**: A conflict of assertions is a property of the world, but a contradiction in the
  schema is a bug in the system. The two must not be confused.
- **Enforcement**:
  - A T-Box change (`define_type`) must pass a consistency check against the existing schema (cyclic
    subtypes, domain/range conflicts, etc.).
  - Inference rules verify non-derivation of contradictions via unit tests.

### Principle 10. Schema Open to Extension, Closed to Modification (Extendibility / Open-Closed)

**The T-Box is a two-layer structure.** The core ontology (the meta level, such as Observation,
Entity, Relation, Provenance, Host, Workspace) is fixed, while the domain ontology (Project, Decision,
Tool...) is opened for extension without modifying the core.

- **Rationale**: Fixing the whole schema prevents agents from expressing knowledge, while opening all
  of it makes a garbage dump. A solution isomorphic to OOP's Open-Closed: a stable core + extension
  points.
- **Enforcement**:
  - A change to the core ontology demands caution on par with a revision of this principle (a
    migration path is mandatory).
  - Adding a domain type must not invalidate existing observations/entities - a schema change that
    breaks existing data is expressed as a new type + supersede.
  - Bootstrap is "a small default set + extension" (promoting the decision in architecture.md
    section 13 to a principle).

### Principle 11. Minimal Ontological Commitment, Schema Is Induced (Minimal Commitment, Induced Schema)

**Model only the minimum needed for knowledge sharing, and leave the rest open.** Reject the
temptation to build an elaborate classification scheme up front. The default path is for the schema to
be **induced bottom-up from instances and then promoted**, rather than designed from the top.

- **Rationale**: An excessive schema is the same disease as excessive abstraction. Elaboration that is
  never used leaves only maintenance cost behind, and a classification built before actual usage
  patterns demand it is mostly wrong. Ontology-building research in the LLM era has converged in the
  same direction - from hand-crafted hierarchy design toward an approach that induces concepts/relations
  from the instance graph (clustering/generalization).
- **Enforcement**:
  - A new type/relation is added **when actual observations demand it** (the YAGNI of knowledge).
  - Always leave an escape hatch for expressing knowledge of unknown type - ingest it first as
    `Concept` + `relates_to` + free-form properties, and promote it to a type once the pattern hardens.
  - Induction goes as far as a proposal, promotion is explicit: the system (or an LLM extractor) may
    **propose** type candidates from repeated patterns, but T-Box promotion happens only through an
    explicit `define_type` act (passing the consistency check of Principle 9).
  - **The scope of the T-Box is the workspace** (limited to the domain ontology - the core ontology is
    globally fixed per Principle 10). There is no global domain T-Box: as long as the schema is induced
    from usage, the workspace, which is the population from which it is induced, is the scope of the
    schema. Type connections across workspaces are expressed only through an explicit alignment
    assertion (bridge claim).
  - Prefer "loose ingest that can be refined later" over "a schema that blocks ingest".

**[6th revision] The substrate of induction is the second-order structure of co-occurrence
(Second-Order Structure as Induction Substrate).** If induction goes "as far as a proposal", there
must be an answer to *what* it induces from. That substrate is the **second-order structure** that
observations leave behind: the set of entities co-asserted by a single observation (a hyperedge) is a
derived view that deterministically recovers from the log the "what was said together" (context) that
the binary-relation projection discarded (Principles 1/16), and as "observation about observation"
(meta-knowledge) it is the very supra-gnosis this project promised in its name. This second-order
structure becomes the common reference point not only for schema induction but for systematization as
a whole - resolution (Principle 15), consolidation (Principle 7), contradiction localization
(Principle 6), curation health metrics (Principle 22) - so that the knowledge system raises its own
self-refinement agenda.

- **Enforcement (extension)**:
  - **A reference, not a judge.** A hyperedge only *generates* candidates/proposals; commitment
    (merge/promotion/schema definition/retraction) goes through the deterministic resolution rule
    (Principle 15), the proposal workflow (Principle 23), and human confirmation (Principle 18). If a
    derived view writes to the canon directly, it violates Principles 1/19 - this is not a new
    authority but a **new input** attached to the existing gated flows.
  - **A projection, not storage.** A hyperedge does not replace the binary Relation model (Principles
    10/12) - it coexists as an undirected/untyped/n-ary derived view, and binary paths such as
    traverse remain as they are. It is identified by its member set, and observations are its
    attestations (isomorphic to the identity model of Principles 3/14: multiple observations that
    produced the same member set are deduped and accumulate as attestations).
  - **Induction outputs are also lineage-bearing + low-trust.** A derived observation
    summarized/promoted from a hyperedge carries its source via `derived_from` and starts at the
    lowest trust - so that unverified co-occurrence is not laundered into the canon (Principle 18).
  - **The criterion is deterministic, the priority is discretionary.** Candidate *generation* is a
    deterministic function of the log, so it converges across nodes (Principle 16). The ordering of
    "what to review first" is curation UX, so heuristics are permitted. Because a corroboration
    (repeated co-assertion) signal cannot be verified against self-declaration, only **independent
    sources** (by delegation-chain principal) are counted (Principles 2/18).
  - **Co-occurrence is a weak signal.** Correlation is not identity/causation - repeated co-membership
    is only a hypothesis generator, so candidates are inherently low-confidence (this low-trust
    treatment prevents the classic failure of knowledge-graph induction that over-trusts co-occurrence
    as fact). A mismatch between asserted cohesion (a hyperedge) and emergent cohesion (a structural
    cluster) is not suppressed but left as a signal (Principle 6).

### Principle 12. Minimal Encoding Bias (Minimal Encoding Bias)

**The knowledge model is not tied to a particular store/representation format.** The model is defined
at the knowledge level, and whether the encoding is Cozo Datalog or RDF is the adapter's concern.

- **Rationale**: The store choice (architecture.md section 6) is a decision that can change. If
  encoding leaks into the model, the store holds the domain hostage.
- **Enforcement**:
  - `supragnosis-core` does not depend on any store crate (dependency direction is enforced).
  - It is a violation if a Cozo-specific concept (e.g., a particular index structure) is exposed in
    the domain model / MCP surface. The `query` passthrough tool is the only explicit exception, and it
    is documented as an "escape hatch for advanced users".

### Principle 13. Distinguishing Essence from Role (Rigidity - OntoClean)

**The is-a hierarchy is used only for essence (rigid).** "Person" is an essence, but "reviewer" is a
role. Roles/states/stages are modeled not as subtypes but as relations or as properties with a valid
time interval.

- **Rationale**: Making a role a subtype creates (isomorphically to a Liskov violation) the
  contradiction that the entity's type must change every time it changes roles. Overuse of is-a is the
  most common path to ontology decay.
- **Enforcement**:
  - Keep the T-Box subtype hierarchy shallow (2-3 levels by default).
  - `define_type` guideline: if the answer to "can this entity stop being that type?" is "yes", it is
    a relation, not a subtype.

---

## Chapter 3 - Connection and Trust (Identity, Federation & Trust)

The reason supragnosis exists is the **connection** of hosts. This chapter fixes the principles for
connection to actually hold, and the defenses against the threats that connection creates.

### Principle 14. Stable Identifiers for Everything (Stable Identifiers)

**Every entity/observation/type has a globally stable identifier** (an application of the Linked Data
principle). The identifier is invariant even when location/store/host changes.

- **Rationale**: Connection across hosts is, in the end, reference. If identifiers are unstable, every
  link in the graph becomes a dangling pointer.
- **Enforcement**:
  - The observation id is a content address (blake3) - since content is identity, path-independent
    dedup holds.
  - The entity id is a resolved canonical id, and even on merge the old id keeps a forwarding to the
    new id (referential integrity).
  - A `supragnosis://` URI must be dereferenceable - whoever knows an id can look up the thing it
    denotes (exposed as an MCP Resource).
  - **Mechanical enforcement of structural evolution**: functions that **define semantics by
    enumerating fields** - such as identity (content-address hash encoding), attestation total order
    (the basis of merge dedup), and re-arrival merge (absorb) - are written so that review of those
    functions is forced at the compile level when a field is added to the model (exhaustive
    destructuring, etc. - so that a missing enumeration becomes a compile error). Whether a new field
    is content identity, an attestation-distinguishing axis, or a merge target must be an explicit
    decision, not a silent default - an omission manifests as id collapse of distinct assertions (this
    principle) or loss of attestation dedup (Principle 3), and the diligence of a review is not a
    guarantee against it.

### Principle 15. Identity Resolution Is the Substrate's Responsibility (Resolution Is Substrate's Job)

**The "problem of calling the same thing by different names" is solved by supragnosis, not the
client.** Hosts speak in their own vocabularies, and the resolution layer makes the connection.

- **Rationale**: Pushing entity resolution onto the client produces as many different resolution
  policies as there are hosts, and the graph shatters into fragments of the same subject. In a system
  whose purpose is connection, resolution is not an add-on feature but a reason for being.
- **Enforcement**:
  - `observe` does not require a canonical id from the client - it lets the client speak by
    name/alias, and the server does the resolution.
  - Resolve conservatively: holding off (keeping the two entities + a candidate link) is better than a
    merge made without conviction. A wrong merge is more expensive than a wrong split (though, by
    Principle 3, both must be reversible).
  - The resolution history (what became what, on what basis) is retained as provenance.

### Principle 16. Topology-Independent Convergence (Topology-Independent Convergence)

**Nodes with the same set of observations materialize the same graph.** No matter which path
(hub/peer/hybrid) or in what order events arrived, the result is identical.

- **Rationale**: Without this property, federation becomes N systems each with a different semantics
  per topology. Only with deterministic convergence (CAS dedup + HLC ordering + deterministic
  projection) does the "manner of connection" become a purely operational choice.
- **Enforcement**:
  - No use of nondeterminism (wall clock, arrival order, random numbers) in projection/resolution
    logic. When ordering is needed, use HLC; when randomness is needed, derive the seed from the event.
  - **Query responses must be deterministic too**: search/traversal responses over the same graph
    state must be reproducible down to the sort order and the limit truncation. It is a violation if
    the iteration order of an internal data structure (hash map, row order) leaks into the response -
    ties/truncation are pinned by a stable key (id).
  - **Two layers of query determinism - reproducibility and convergence**: (a) **reproducibility**
    (same query against the same state of the same node -> same response) is the duty of every query
    surface - semantic recall (ANN) also upholds this by pinning ties/truncation with a stable key.
    (b) **convergence** (two nodes with the same observation set -> same response) is the duty of the
    deterministic read surfaces (exact lookup, graph traversal, keyword search). A semantic-recall
    index (embeddings, ANN graph) is not part of the materialized graph but a **node-local recall
    aid** - being an insertion-order-dependent approximate structure, it cannot promise convergence
    across nodes, and need not (the storage/recall separation of Principle 7, the recall/commit
    separation of Appendix A) - it is not subject to the convergence norm. This exemption, however,
    presumes two guards: when a recall result becomes an input to a commit (merge/promotion, etc.), it
    must pass through the deterministic rules again (Principle 19), and a query response must label
    which surface it came from (mode) so that a client can distinguish the convergence surface from the
    recall aid.
  - This property is verified continuously by test: maintain a property test that injects the same
    event set in random order/partitioning and checks graph identity.
  - **Convergence and monotonicity are different properties.** Convergence is "order-independent - the
    same set yields the same result", while monotonicity is "stability under set growth - an
    already-reached conclusion is not overturned by further events arriving". A deterministic fold
    gives convergence, but "from when may a conclusion be trusted and derivations stacked on it"
    (finality) holds only when the decision rule forms a semilattice (absorbing state) and is thus
    monotonic. An ordering function (of the first-writer-wins kind) converges but is not monotonic,
    because a late-arriving early-timestamped event can change the conclusion. Therefore a terminal
    state on which derivations accumulate is designed as an absorbing state (proposal-workflow.md I16).

### Principle 17. Knowledge Sovereignty - Selective Sharing (Knowledge Sovereignty)

**It must not be the default that all knowledge goes out.** What to share is decided by the host that
created the knowledge, and the sharing boundary is enforced at the sync layer.

- **Rationale**: Trust in a local-first system comes from "what is mine is under my control". If
  sharing is the default, people stop ingesting sensitive knowledge at all, and the system loses its
  most valuable knowledge.
- **Enforcement**:
  - Sharing is a workspace whitelist opt-in (default: no sharing).
  - Filters/redaction are applied at the sync boundary - what must not leave is filtered out before it
    reaches the peer node (not a delete request after it arrives).
  - **The sharing boundary applies to the remote query surface too**: if only the sync filter is
    blocked while the remote MCP query is left open, the same knowledge goes out through a different
    door. Reads by a remote (non-local) client are governed by the sharing whitelist, and a global
    query with no workspace scope is limited to the local trust surface (stdio, single user).
  - Provide a secret-redaction hook at ingest, but this is an aid to, not a replacement for, the
    sharing filter (defense in depth).

### Principle 18. Writes Are an Attack Surface (Writes Are an Attack Surface)

**Every observe is a potential source of contamination.** The integrity of knowledge is not
guaranteed by signatures (transport integrity) alone - a signature only proves "who sent it", not
"whether the content is true". Defense against contamination is done with the defense-in-depth of
trust tiers, lineage tracking, and quarantine.

- **Rationale**: Memory/context contamination is a real-world threat formally listed in the OWASP
  Agentic Top 10 (2026) (ASI06). The typical path: an agent reads a contaminated external document ->
  summarizes and ingests it into the knowledge base -> the contamination becomes persistent ->
  **cross-contamination** propagates into the outputs of other agents that read that knowledge. For
  supragnosis, whose purpose is to connect the knowledge of many hosts, this is not a peripheral
  threat but a frontal one.
- **Enforcement**:
  - **Provenance trust tier**: every observation carries a tier according to the verification level of
    its source (e.g., human-confirmed > signed trusted host > agent extraction from a signed host >
    unverified external source). The tier is reflected in resolution weighting and query ranking.
  - **Mandatory lineage**: an assertion derived from other knowledge/external content (summary,
    inference, extraction) must be traceable via `derived_from` back to the source observation. A
    derived assertion without lineage is not refused ingest (in tension with Principle 22) but marked
    as quarantine at the **lowest trust tier**.
  - **Sanitizability**: when a contaminated source is discovered, it must be possible to trace back
    its entire derived tree by lineage and retract them in bulk (the retraction observation of
    Principle 3) - lineage is the recall list.
  - **Promotion** of the trust tier happens **only explicitly** (human confirmation, independent
    cross-source verification, etc.). Trust does not rise by itself just because time has passed.
  - **The tier is the receiver's evaluation**: a trust tier is not a stored attribute that an event
    self-declares, but an evaluation the receiving node computes based on provenance (delegation chain,
    signature, verification history at its own node). A tier annotation carried on an observation that
    arrived via sync is only the sending node's evaluation record and does not bind the receiving node
    - a malicious peer's "human-confirmed" self-declaration must not be able to contaminate the trust
    of the receiving graph (isomorphic to the concern of Principle 2 about not being able to verify a
    self-declared identity).

---

## Chapter 4 - System Composition (System Design)

### Principle 19. Deterministic Core, Probabilistic Edge (Deterministic Core, Probabilistic Edge)

**The main body of supragnosis is a deterministic substrate.** Probabilistic components such as
LLM/embedding are replaceable adapters behind ports, and the correctness of the core must not depend
on them.

- **Rationale**: The reason this project exists is the diagnosis that "an LLM is probabilistic and
  therefore cannot be a knowledge base". It is self-contradictory if the core of that solution is
  itself probabilistic again. An LLM is an excellent **extractor/consumer** of knowledge, not a
  **store/judge**.
- **Enforcement**:
  - Extraction (Extractor) / embedding (EmbeddingProvider) are ports. Even without them the system
    operates, though degraded (no embedding -> keyword search).
  - When the output of a probabilistic component becomes knowledge, it must enter as an assertion
    carrying confidence (subsumed under Principle 1) - it is a violation if an LLM extraction result is
    turned into fact without annotation.
  - In entity resolution, embedding similarity goes only as far as candidate generation; merge
    commitment is done with a deterministic rule (canonical key, threshold, or human confirmation).

### Principle 20. Purity of the Domain (Hexagonal Purity)

**`supragnosis-core` has zero IO dependencies.** Store/network/embedding are all behind port traits,
and the dependency direction always points from the outside (adapters) to the inside (the domain).

- **Rationale**: The knowledge model is the longest-living asset in this system, while the
  store/protocol are the parts that change first. If things with different lifetimes are not isolated,
  replacing a part becomes a rewrite of the asset. (It is also the structural enforcement mechanism for
  Principle 12.)
- **Enforcement**:
  - It is a violation if an IO crate (tokio, cozo, reqwest, etc.) appears in `core`'s `Cargo.toml`.
  - All engine logic must be unit-testable with an in-memory adapter alone.

### Principle 21. A Narrow, Clear Surface for the LLM (Narrow, LLM-Legible Surface)

**MCP tools are few, and each tool expresses one intent.** The default surface is intent-level tools
such as `observe`/`search`/`traverse`/`assert_relation`, not the exposure of a general-purpose query
language.

- **Rationale**: The primary user is the LLM agent. An LLM uses narrow, honestly-named tools well, and
  uses an infinitely expressive query language (SPARQL/Datalog) poorly. The design of the tool surface
  is this system's UX.
- **Enforcement**:
  - The criterion for adding a new tool: "is it a recurring intent of the agent?" It is not added
    merely because it is expressible (the API version of Principle 11).
  - Tool descriptions/parameters/error messages are written so that the LLM can self-correct - a
    failure response includes "why it failed and what to do differently".
  - **Long-running work does not block**: work that takes a long time, such as sync/consolidation/bulk
    re-projection, is exposed as a pollable task handle (aligned with the MCP Tasks extension -
    polling-style tasks were fixed as an official extension in the 2026-07 spec). Points that need
    human confirmation (merge approval, contradiction mediation, trust promotion) are expressed via
    MCP's input-request flow (elicitation / multi round-trip), so that the "human mediation" of
    Principles 6/18 is possible at the protocol level.
  - `query` (Datalog passthrough) exists only as an advanced escape hatch under a permission guard.

### Principle 22. Knowledge Management Is a By-Product of Work (Knowledge as a By-Product)

**If knowledge ingest becomes a separate labor, the system dies.** supragnosis is designed so that
knowledge accumulates naturally within the flow in which a host works - the output of work becomes
knowledge.

- **Rationale**: The common cause of death of knowledge-management systems that relied on manual
  curation is "no one enters anything". The opportunity of the agent era is that agents can be made to
  shed observations while they work, and seizing this opportunity is what differentiates this project.
- **Enforcement**:
  - `observe` must have minimal friction: free text + optional structured assertions. It does not
    require perfect structuring as a precondition for ingest (resonating with Principle 11).
  - Prompts (`what-do-we-know-about`, etc.) and tool descriptions are written to induce the agent to
    observe/search voluntarily during work.
  - Curation (type promotion, merge confirmation, contradiction mediation, trust promotion) is
    designed not as separate work but as micro-decisions that surface naturally in query results.

---

## Chapter 5 - Governance (Governance)

Fix the procedure by which multiple subjects agree on a single canon.
Concrete design in [proposal-workflow.md](proposal-workflow.md).

### Principle 23. Gate to Canon (Gate to Canon)

**Ingest is free, and promotion to the canon happens only through a proposal.** The five intents that
affect the canon - tier promotion/demotion, entity merge/split, T-Box change, recall - go only through
the proposal workflow. And **a proposal is itself knowledge**: the creation/review/verdict of a
proposal are all observation events, and are governed by this principle just the same.

- **Rationale**: The pattern that code collaboration validated with the PR - placing a reviewable gate
  between an individual's work and the shared canon - holds for knowledge too. However, in knowledge
  the gate must be a **gate of tier**, not a gate of existence (since Principle 22 forbids an ingest
  gate). Wikidata's "open ingest + rank/patrol tier management" is a 20-year precedent for this
  structure. Moreover, making the proposal a separate subsystem would doubly implement
  provenance/convergence/preservation, so expressing the proposal as an observation is correct both
  logically and economically.
- **Enforcement**:
  - It is a violation if there is an API path by which a change affecting the canon bypasses the
    proposal. Conversely, it is also a violation if the proposal procedure blocks or delays observe (no
    bidirectional gating).
  - A proposal's state is a deterministic fold of the event stream (Principle 16). The passage of
    wall-clock time causes no transition, and auto-merge/expiry are also expressed only as explicit
    events.
  - Concurrent verdicts converge via a decision rule that treats a valid merge as an absorbing state.
    Beyond convergence (order-independent), it is **monotonic** - promotion is a monotonic function of
    the event set, so from the moment a valid merge is in the log, no event's arrival can retroactively
    cancel the promotion. A reversal (demotion) is always a new proposal.
  - Before merging, a belief diff (promotions/reversals/new contradictions/blast radius) and check
    results must already be computed - a merge without a diff is a merge without review.
  - A rejection is not a negation (Principle 5). No verdict deletes an assertion (Principle 3).
  - Self-approval is forbidden (by the delegation chain's principal). The single-person workspace
    exception attaches a self-attested marker so that it is distinguished in trust evaluation upon
    federation (Principle 18).
  - **The recall verdict is non-delegable**: because bulk retraction of a derived tree is the verdict
    with the largest destructive radius, it cannot hold as an agent's proxy verdict through the
    delegation chain ("under the human principal's authority") but must be the human's direct act (a
    direct signature or an elicitation response). This blocks the path by which a contaminated agent
    bulk-retracts without human eyes via proxy self-approval (this is exactly the scenario Principle 18
    guards against).

---

## Appendix A - Tensions Between Principles and Priorities

Principles can be in tension with each other. Guidance for judgment upon conflict:

| Tension | Judgment |
|------|------|
| Loose ingest (11, 22) vs schema clarity (8, 9) | **Prioritize ingest**, but leave a refinement path. Knowledge that never came in cannot be refined either. |
| Loose ingest (22) vs contamination defense (18) | **Do not block ingest; defend with trust.** Suspicious knowledge comes in too, but is quarantined at the lowest tier - not locking the door but attaching a label. |
| Preservation (3, 6) vs privacy (17) | **Privacy wins.** A destruction demand is the only exception to Principle 3 and leaves a tombstone. |
| Preservation (3) vs forgetting (7) | Not a conflict - **the log is eternal and recall is finite.** Forgetting is the allocation of attention, not of storage. |
| Aggressiveness of resolution (15) vs conservatism | **Conservatism is the default.** Auto-merge only in the confident band; when ambiguous, hold off with a candidate link. |
| Minimal tool surface (21) vs expressiveness | **Minimal is the default.** An expressiveness demand is first validated by composing existing tools, and promoted only when it is a repeated pattern. |
| Deterministic core (19) vs semantic search quality | Use the probabilistic element only **to widen recall**, and commit deterministically. |
| Gate (23) vs minimal friction (22) | **The gate is on promotion only, not on ingest.** Even when review is backlogged, knowledge already exists and circulates at a low tier. |
| Review rigor (23) vs reviewer's attention | **Most is automatic, only a few go to a human.** Only new contradictions/high-impact/structural changes/recall force human review. |

## Appendix B - Review Checklist

Questions to apply quickly during design/PR review:

- Does this change write a fact without going through an assertion? (Principle 1)
- Does ingest validation reject/transform an assertion's content beyond well-formedness? Conversely,
  is there a path by which a non-assertion (empty referent) gets placed in the permanent log?
  (Principle 1)
- Is there a path that stores without provenance? Does the delegation chain break? (Principle 2)
- Does it destructively overwrite something? Does a re-arrival of the same content address overwrite
  non-identity fields (attestation/lineage)? (Principle 3 merge norm)
- Is there a path by which the receiving node trusts a trust tier/confidence self-declared by another
  node without re-evaluation? (Principles 2/18)
- Is there logic that confuses valid time with transaction time? (Principle 4)
- Is there logic that interprets absence as negation? (Principle 5)
- Does forgetting/consolidation touch the observation log? (Principle 7)
- Is something modeled as a subtype actually a role? (Principle 13)
- Does adding a model field force compile-time review of the identity/total-order/merge functions?
  (Principle 14)
- Did nondeterminism enter the projection? (Principle 16, verified by property test)
- Does derived knowledge have `derived_from` lineage? Is there an ingest path with no trust tier?
  (Principle 18)
- Is a candidate induced from a hyperedge/co-occurrence reflected in the canon without a gate
  (deterministic resolution/proposal/human confirmation)? Is the induction output missing
  lineage/low-trust? (Principle 11 second-order structure, 15, 18, 23)
- Is there a path by which trust promotion happens implicitly? (Principle 18)
- Did an IO dependency creep into `core`? (Principle 20)
- Is the new MCP tool really a recurring intent? Does long-running work block? (Principle 21)
- Does this feature demand extra labor from the host? (Principle 22)
- Does a change affecting the canon bypass the proposal? Did wall-clock/nondeterminism enter the
  proposal state? (Principle 23)
- Is there a path that makes self-approval possible? (Principle 23)
- Do we measure recall quality? - search/recall changes are verified with a regression eval set in the
  style of a memory benchmark (LongMemEval-like).

## Appendix C - References

- T. R. Gruber, *Toward Principles for the Design of Ontologies Used for
  Knowledge Sharing* (1995) - Principles 8-12.
- N. Guarino & C. Welty, *OntoClean* - Principle 13.
- T. Berners-Lee, *Linked Data Design Issues* (2006) - Principle 14.
- Event Sourcing / CRDT / local-first literature - Principles 1, 3, 16.
- Bitemporal data modeling (valid/transaction time); its application in agent memory is
  Zep/Graphiti (*Zep: A Temporal Knowledge Graph Architecture for Agent Memory*,
  arXiv:2501.13956) - Principle 4.
- Sleep-time compute / memory consolidation / selective forgetting research (2025-2026) - Principle 7.
- OWASP Top 10 for Agentic Applications 2026, ASI06 "Memory & Context
  Poisoning"; memory-contamination attack/defense research - Principle 18.
- Agent identity/delegation standardization: A2A (Linux Foundation), IETF drafts (AIMS, WIMSE,
  Agentic JWT), the C2PA signed-provenance model - Principle 2.
- Surveys of LLM-based knowledge-graph/ontology induction (arXiv:2510.20345, etc.) - Principle 11.
- MCP spec 2026-07-28 (Tasks extension, multi round-trip input) - Principle 21.
- Code review/PR workflow; Wikidata's statement rank + patrol model; branch-style
  knowledge stores (TerminusDB, Dolt) - Principle 23.
- The multi-host principles of Chapters 1/3/4/5 are supragnosis's own extension.
