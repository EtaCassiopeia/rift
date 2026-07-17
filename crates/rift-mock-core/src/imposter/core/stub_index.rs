//! Stage-1 candidate prefilter for imposter stub matching (issues #292, #707).
//!
//! The imposter match loop (`core/matching.rs`) would otherwise be a linear scan running full
//! Mountebank predicate evaluation on every stub. This index narrows that to a candidate set, so
//! only stubs that *could* match are evaluated.
//!
//! # The dimension framework (issue #707)
//!
//! The index is a set of independent **dimensions**, one per request attribute (path, method, and
//! — via the sibling issues built on this seam — deepEquals bodies (#708), regexes (#709), and
//! literals (#710)). Each dimension answers one question for a request: *which stubs can this
//! attribute not rule out?* The answers are [`CandidateBits`] — dense bitsets over stub ids, where
//! a stub's id is its position in the snapshot's stub vector (declaration-ordered).
//!
//! [`StubIndex::candidates`] ANDs the per-dimension bitsets and walks the surviving bits ascending.
//! This is the Lucent Bit Vector technique from packet classification: each dimension prunes
//! independently, and the intersection is the candidate set. Ascending iteration *is* Mountebank's
//! first-match-wins order, so Stage-2 evaluation order is unchanged.
//!
//! ## The soundness rule — the one invariant every dimension must uphold
//!
//! A dimension's bitset for a request is `matched_bits | always_bits`, where:
//!
//! * `matched_bits` — stubs whose constraint on this attribute the request *satisfies*;
//! * `always_bits` — stubs that either do not constrain this attribute at all, or constrain it in
//!   a shape this dimension cannot index (precomputed once at build).
//!
//! **A dimension may only ever exclude a stub it can prove cannot match.** Everything else must be
//! in `always_bits`. The index is therefore a strict *over-approximation*: `candidates()` returns a
//! superset of the true matches, and full predicate verification (`stub_matches`, unchanged) stays
//! the single source of truth for semantics — including `and`/`or`/`not`, `except`, selectors,
//! case flags, and `inject`. Widening a dimension's eligibility later is a pure optimization, never
//! a semantics question.
//!
//! Eligibility is deliberately conservative and uniform across dimensions: a stub is indexed only
//! when a *top-level* (implicitly AND-ed) predicate is a plain `equals`/`startsWith`/`contains` on
//! the raw field — no `selector`, no `except`, not `caseSensitive`. Such a predicate is *required*
//! for the stub to match, so a request failing it can never match the stub → safe to exclude.
//!
//! ## Adding a dimension
//!
//! Implement [`Dimension`], add it as a field of [`StubIndex`], build its `always_bits` from the
//! stubs it cannot index, and AND it in [`StubIndex::candidates`]. Dimensions are concrete fields
//! rather than `Box<dyn Dimension>` so the match loop dispatches statically and allocates nothing
//! extra. The guardrail is `differential_index_matches_linear_oracle` below: a dimension that
//! under-approximates fails it immediately.

use super::StubState;
use super::bitset::CandidateBits;
use crate::imposter::types::Stub;
use crate::util::FastMap;
use rift_types::predicate::{Predicate, PredicateOperation};
use std::collections::HashMap;
use std::sync::Arc;

/// The request attributes the index prunes on. Extended as sibling dimensions land.
pub(crate) struct DimensionRequest<'a> {
    pub(crate) method: &'a str,
    pub(crate) path: &'a str,
}

/// One pruning dimension of the index. See the module docs for the soundness rule this contract
/// requires: `select` must set a bit for **every** stub the request cannot rule out.
trait Dimension {
    /// Write `matched_bits | always_bits` for `request` into `out`, overwriting it entirely.
    fn select(&self, request: &DimensionRequest<'_>, out: &mut CandidateBits);

    /// Whether any stub is indexed on this dimension at all.
    ///
    /// When none is, `always_bits` is all-ones, so `select` can only ever produce all-ones and
    /// intersecting it is a no-op — [`StubIndex::candidates`] skips the dimension entirely rather
    /// than pay a full-width copy and intersect to learn nothing. Constant per snapshot.
    fn prunes(&self) -> bool;
}

/// A required path constraint extracted from a stub's top-level predicates.
enum PathAnchor {
    Exact(String),
    Prefix(String),
    Contains(String),
}

/// The `path` value of a predicate's field map, folded for indexing, if present and a string.
fn field_path(fields: &HashMap<String, serde_json::Value>) -> Option<String> {
    match fields.get("path") {
        Some(serde_json::Value::String(s)) => Some(fold(s)),
        _ => None,
    }
}

/// The case fold the index compares under.
///
/// This MUST be the evaluator's fold, not merely *a* fold. The default (non-`caseSensitive`)
/// comparison in `predicates::mod` is `eq_ignore_ascii_case` / `starts_with_ignore_ascii_case` /
/// `contains_ignore_ascii_case` — **ASCII**. Folding both sides with `to_ascii_lowercase` is
/// exactly equivalent to those, so the path dimension neither over- nor under-approximates.
///
/// Unicode `to_lowercase` would be wrong here, and not merely conservative: it is length-changing
/// and context-sensitive, so it breaks the prefix/substring relation the dimension relies on. Stub
/// `startsWith "/ΟΣ"` vs request `/ΟΣΑ` is the counter-example — the evaluator matches (its ASCII
/// fold leaves Greek untouched), but Unicode-lowercasing the anchor yields a final sigma (`/ος`)
/// that `"/οσα"` does not start with, so the stub would be pruned and silently stop matching.
fn fold(s: &str) -> String {
    s.to_ascii_lowercase()
}

/// Whether a predicate's parameters leave its field values comparable as raw request values — the
/// soundness gate every dimension's eligibility rule shares.
///
/// Anything that transforms or re-scopes the compared value cannot be indexed against the raw
/// field: `except` rewrites the value before comparison, `selector` re-scopes it, and
/// `caseSensitive` opts out of the fold [`fold`] assumes. One home for the rule, because the
/// dimensions added on this seam (#708-#710) must not let their copies of it drift.
fn is_safely_indexable(p: &rift_types::predicate::PredicateParameters) -> bool {
    p.case_sensitive != Some(true) && p.except.is_empty() && p.selector.is_none()
}

/// A single predicate's path anchor, if it is a safely-indexable required path constraint.
fn path_anchor(pred: &Predicate) -> Option<PathAnchor> {
    if !is_safely_indexable(&pred.parameters) {
        return None;
    }
    match &pred.operation {
        PredicateOperation::Equals(fields) => field_path(fields).map(PathAnchor::Exact),
        PredicateOperation::StartsWith(fields) => field_path(fields).map(PathAnchor::Prefix),
        PredicateOperation::Contains(fields) => field_path(fields).map(PathAnchor::Contains),
        _ => None,
    }
}

/// The first required path anchor among a stub's top-level (AND-ed) predicates, or `None` if the
/// stub can't be safely path-indexed (→ `always_bits`).
fn classify(stub: &Stub) -> Option<PathAnchor> {
    stub.predicates.iter().find_map(path_anchor)
}

/// The path dimension (issue #292, ported onto the #707 bitset framework).
///
/// Buckets stay `Vec<usize>` rather than a bitset each: a bucket holds only the stubs sharing an
/// anchor, so materializing it costs O(matched) rather than O(stubs/64), and build memory stays
/// O(stubs) instead of O(stubs x buckets).
///
/// The prefix/contains buckets are walked linearly, exactly as pre-#707. Issue #710 replaces both
/// walks with an anchored/unanchored Aho-Corasick pass behind this same `Dimension` seam.
struct PathDimension {
    // Rebuilt on every stub-set replace/mutation (issue #704); its keys come from operator stub
    // config, not request traffic — see `crate::util::fastmap` doc for the HashDoS policy.
    exact: FastMap<String, Vec<usize>>,
    prefix: Vec<(String, Vec<usize>)>,
    contains: Vec<(String, Vec<usize>)>,
    /// Stubs with no indexable top-level path constraint — always candidates on this dimension.
    always: CandidateBits,
}

impl Dimension for PathDimension {
    fn select(&self, request: &DimensionRequest<'_>, out: &mut CandidateBits) {
        out.copy_from(&self.always);
        // Anchors were folded at build; fold the request the same way — see `fold`.
        let p = fold(request.path);
        if let Some(v) = self.exact.get(&p) {
            v.iter().for_each(|i| out.set(*i));
        }
        for (prefix, v) in &self.prefix {
            if p.starts_with(prefix.as_str()) {
                v.iter().for_each(|i| out.set(*i));
            }
        }
        for (sub, v) in &self.contains {
            if p.contains(sub.as_str()) {
                v.iter().for_each(|i| out.set(*i));
            }
        }
    }

    fn prunes(&self) -> bool {
        !self.exact.is_empty() || !self.prefix.is_empty() || !self.contains.is_empty()
    }
}

/// The fixed method slots. Any method outside the standard set (or a stub constraining an
/// unusual one) shares `Other` — a coarser bucket is still sound, it just prunes less.
const METHOD_SLOTS: usize = 8;
const SLOT_OTHER: usize = METHOD_SLOTS - 1;

/// The slot a method name belongs to, matched case-insensitively and without allocating.
fn method_slot(method: &str) -> usize {
    const NAMED: [&str; SLOT_OTHER] = ["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS"];
    NAMED
        .iter()
        .position(|m| method.eq_ignore_ascii_case(m))
        .unwrap_or(SLOT_OTHER)
}

/// The method required by a single predicate, if it is a safely-indexable required constraint.
fn method_anchor(pred: &Predicate) -> Option<&str> {
    if !is_safely_indexable(&pred.parameters) {
        return None;
    }
    match &pred.operation {
        PredicateOperation::Equals(fields) => match fields.get("method") {
            Some(serde_json::Value::String(s)) => Some(s.as_str()),
            _ => None,
        },
        _ => None,
    }
}

/// The method dimension (issue #707) — the cheapest possible dimension, and the proof of the
/// framework: a POST-only stub stops being a candidate for every GET.
struct MethodDimension {
    slots: [Vec<usize>; METHOD_SLOTS],
    /// Stubs with no indexable top-level method constraint — always candidates on this dimension.
    always: CandidateBits,
}

impl Dimension for MethodDimension {
    fn select(&self, request: &DimensionRequest<'_>, out: &mut CandidateBits) {
        out.copy_from(&self.always);
        self.slots[method_slot(request.method)]
            .iter()
            .for_each(|i| out.set(*i));
    }

    fn prunes(&self) -> bool {
        self.slots.iter().any(|s| !s.is_empty())
    }
}

/// The multi-dimensional candidate prefilter over a stub snapshot. See the module docs.
pub(crate) struct StubIndex {
    len: usize,
    path: PathDimension,
    method: MethodDimension,
}

impl StubIndex {
    /// Build every dimension in one pass over the stubs, preserving ascending stub id within each
    /// bucket (so iteration stays declaration-ordered).
    fn build(stubs: &[Arc<StubState>]) -> Self {
        let len = stubs.len();
        let mut exact: FastMap<String, Vec<usize>> = FastMap::default();
        let mut prefix: FastMap<String, Vec<usize>> = FastMap::default();
        let mut contains: FastMap<String, Vec<usize>> = FastMap::default();
        let mut path_always = CandidateBits::zeros(len);

        let mut slots: [Vec<usize>; METHOD_SLOTS] = Default::default();
        let mut method_always = CandidateBits::zeros(len);

        for (i, state) in stubs.iter().enumerate() {
            match classify(&state.stub) {
                Some(PathAnchor::Exact(k)) => exact.entry(k).or_default().push(i),
                Some(PathAnchor::Prefix(k)) => prefix.entry(k).or_default().push(i),
                Some(PathAnchor::Contains(k)) => contains.entry(k).or_default().push(i),
                None => path_always.set(i),
            }
            match state.stub.predicates.iter().find_map(method_anchor) {
                Some(m) => slots[method_slot(m)].push(i),
                None => method_always.set(i),
            }
        }

        StubIndex {
            len,
            path: PathDimension {
                exact,
                prefix: prefix.into_iter().collect(),
                contains: contains.into_iter().collect(),
                always: path_always,
            },
            method: MethodDimension {
                slots,
                always: method_always,
            },
        }
    }

    /// The number of stubs this index spans.
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// Candidate stub ids for a request: the intersection of every dimension's bitset. A superset
    /// of the stubs that could match — Stage-2 does the real Mountebank evaluation on these, in the
    /// ascending (declaration) order [`CandidateBits::iter`] yields.
    /// Dimensions run cheapest-first, and only when they can actually prune:
    ///
    /// * a dimension no stub is indexed on is skipped ([`Dimension::prunes`]) — otherwise a corpus
    ///   that never constrains the method would pay the method dimension a full-width copy and
    ///   intersect to produce all-ones;
    /// * the first dimension that does run seeds the accumulator directly, since `select`
    ///   overwrites — no `all()` fill to intersect against, and no scratch buffer;
    /// * an empty accumulator short-circuits the rest. Method runs first because it is both the
    ///   cheapest (a slot lookup, no allocation) and, on method-partitioned corpora, the most
    ///   selective — so the exit can skip the path dimension's fold allocation entirely.
    ///
    /// Sibling dimensions (#708-#710) slot in as further blocks in the same shape.
    pub(crate) fn candidates(&self, request: &DimensionRequest<'_>) -> CandidateBits {
        let mut acc = CandidateBits::zeros(self.len);
        let mut seeded = false;

        if self.method.prunes() {
            self.method.select(request, &mut acc);
            seeded = true;
            if acc.is_empty() {
                return acc;
            }
        }

        if self.path.prunes() {
            if seeded {
                let mut scratch = CandidateBits::zeros(self.len);
                self.path.select(request, &mut scratch);
                acc.intersect_with(&scratch);
            } else {
                self.path.select(request, &mut acc);
                seeded = true;
            }
        }

        // No dimension indexes anything (e.g. every stub is a body regex): everyone is a candidate.
        if seeded {
            acc
        } else {
            CandidateBits::all(self.len)
        }
    }
}

/// Does this predicate tree contain an `inject` predicate anywhere?
fn predicate_contains_inject(pred: &Predicate) -> bool {
    match &pred.operation {
        PredicateOperation::Inject(_) => true,
        PredicateOperation::Not(inner) => predicate_contains_inject(inner),
        PredicateOperation::And(children) | PredicateOperation::Or(children) => {
            children.iter().any(predicate_contains_inject)
        }
        _ => false,
    }
}

/// The unit of stub state the match hot path reads: the stubs, the index over *those exact* stubs,
/// and the snapshot-wide precomputed gates (issue #707).
///
/// Held behind a single `ArcSwap` in [`Imposter`](super::Imposter), so one wait-free `load()` per
/// request yields all of it. Before #707 the stubs and the index lived in two `ArcSwap`s kept in
/// sync only by convention inside `mutate_stubs`; bundling them makes that invariant type-enforced
/// — a reader cannot observe an index built for a different stub vector — and costs one atomic
/// instead of two.
pub(crate) struct StubSnapshot {
    stubs: Vec<Arc<StubState>>,
    index: StubIndex,
    /// Whether any stub's predicate tree contains an `inject` predicate, anywhere (including
    /// nested under `and`/`or`/`not`). Computed once per snapshot so the request hot path can
    /// gate the bounded (spawn_blocking) matching route on it for free (issue #476).
    has_inject: bool,
    /// Whether any stub is scenario-gated (`requiredScenarioState`). The eligibility gate reads
    /// flow state during matching; on a blocking backend (Redis) that read must run off the tokio
    /// worker, so the bounded matcher offloads only when this is set — a scenario-free snapshot
    /// keeps the inline fast path even on a blocking backend (issue #475).
    has_scenario_gate: bool,
}

impl StubSnapshot {
    /// Build the index and the snapshot-wide gates for `stubs`.
    pub(crate) fn build(stubs: Vec<Arc<StubState>>) -> Self {
        let index = StubIndex::build(&stubs);
        let has_inject = stubs
            .iter()
            .any(|s| s.stub.predicates.iter().any(predicate_contains_inject));
        let has_scenario_gate = stubs
            .iter()
            .any(|s| s.stub.required_scenario_state.is_some());
        StubSnapshot {
            stubs,
            index,
            has_inject,
            has_scenario_gate,
        }
    }

    /// The stubs this snapshot describes, in declaration order.
    pub(crate) fn stubs(&self) -> &[Arc<StubState>] {
        &self.stubs
    }

    /// Whether any stub in this snapshot uses an `inject` predicate (issue #476).
    pub(crate) fn has_inject(&self) -> bool {
        self.has_inject
    }

    /// Whether any stub in this snapshot is scenario-gated (`requiredScenarioState`, issue #475).
    pub(crate) fn has_scenario_gate(&self) -> bool {
        self.has_scenario_gate
    }

    /// Candidate stub ids for a request — see [`StubIndex::candidates`].
    pub(crate) fn candidates(&self, method: &str, path: &str) -> CandidateBits {
        self.index.candidates(&DimensionRequest { method, path })
    }

    /// The index over these stubs (tests assert dimension-level behaviour through it).
    #[cfg(test)]
    pub(crate) fn index(&self) -> &StubIndex {
        &self.index
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::imposter::core::Imposter;
    use crate::imposter::types::ImposterConfig;
    use rand::rngs::StdRng;
    use rand::{Rng, SeedableRng};
    use serde_json::{Value, json};
    use std::collections::HashMap;

    fn stub_states(preds: &[Value]) -> Vec<Arc<StubState>> {
        preds
            .iter()
            .map(|p| {
                let stub = serde_json::from_value(
                    json!({ "predicates": p, "responses": [{ "is": { "statusCode": 200 } }] }),
                )
                .expect("valid stub");
                Arc::new(StubState::new(stub))
            })
            .collect()
    }

    /// A snapshot over `preds`, for dimension-level assertions.
    fn snapshot(preds: &[Value]) -> StubSnapshot {
        StubSnapshot::build(stub_states(preds))
    }

    /// The candidate ids for a request, ascending — the pre-#707 `candidates()` shape, so the
    /// existing coverage/ordering assertions read unchanged.
    fn candidate_ids(snap: &StubSnapshot, method: &str, path: &str) -> Vec<usize> {
        snap.candidates(method, path).iter().collect()
    }

    fn imposter(preds: &[Value]) -> Imposter {
        let stubs: Vec<Value> = preds
            .iter()
            .map(|p| json!({ "predicates": p, "responses": [{ "is": { "statusCode": 200 } }] }))
            .collect();
        let config: ImposterConfig =
            serde_json::from_value(json!({ "port": 9999, "protocol": "http", "stubs": stubs }))
                .expect("valid imposter config");
        Imposter::new(config).expect("test imposter")
    }

    /// A diverse corpus exercising every anchor category AND every fallback category, in an order
    /// that makes first-match-wins meaningful (the `not`/empty stubs at the end catch anything).
    fn corpus() -> Vec<Value> {
        vec![
            json!([{"equals": {"path": "/exact"}}]),       // 0 exact
            json!([{"equals": {"path": "/EXACT"}}]),       // 1 exact, other case
            json!([{"startsWith": {"path": "/pre"}}]),     // 2 prefix
            json!([{"contains": {"path": "mid"}}]),        // 3 contains
            json!([{"matches": {"path": "^/re[0-9]+$"}}]), // 4 regex -> fallback
            json!([{"exists": {"query": true}}]),          // 5 exists -> fallback
            json!([{"equals": {"method": "POST"}}]),       // 6 method-only -> fallback
            json!([{"equals": {"body": "ping"}}]),         // 7 body -> fallback
            json!([{"or": [{"equals": {"path": "/o1"}}, {"equals": {"path": "/o2"}}]}]), // 8 or -> fallback
            json!([{"not": {"equals": {"path": "/nope"}}}]), // 9 not -> fallback
            json!([{"equals": {"path": "/cs"}, "caseSensitive": true}]), // 10 caseSensitive -> fallback
            json!([{"equals": {"method": "GET", "path": "/mp"}}]),       // 11 method+path exact
            json!([]),                                                   // 12 match-all -> fallback
        ]
    }

    fn idx(r: anyhow::Result<Option<(Arc<StubState>, usize)>>) -> Option<usize> {
        r.expect("no backend error").map(|(_, i)| i)
    }

    // AC2: the indexed path returns the SAME matched stub as the linear scan for every request —
    // the correctness guardrail. Covers case-insensitivity, prefix/contains, all fallback
    // categories, method+path, and first-match-wins ordering (the trailing not/empty stubs).
    #[test]
    fn indexed_matching_equals_linear() {
        let imp = imposter(&corpus());
        let no_headers: HashMap<String, String> = HashMap::new();

        // (method, path, query, body)
        let requests: &[(&str, &str, Option<&str>, Option<&str>)] = &[
            ("GET", "/exact", None, None),
            ("GET", "/EXACT", None, None),
            ("GET", "/eXaCt", None, None), // case-insensitive collides on both 0 and 1 -> 0 wins
            ("GET", "/prefixed/deep", None, None),
            ("GET", "/pre", None, None),
            ("GET", "/x-mid-y", None, None),
            ("GET", "/re12", None, None),
            ("GET", "/re", None, None), // regex requires digits -> no 4; falls to not(9)
            ("GET", "/mp", None, None),
            ("POST", "/mp", None, None), // method+path requires GET -> not 11; POST hits 6
            ("GET", "/nope", None, None), // not(/nope) excludes -> empty(12)
            ("GET", "/cs", None, None),  // caseSensitive lives in fallback
            ("GET", "/CS", None, None),
            ("GET", "/o1", None, None),
            ("GET", "/o2", None, None),
            ("GET", "/anything", Some("a=1"), None), // exists{query} -> 5
            ("GET", "/anything", None, Some("ping")), // body -> 7 (9 not also matches, order)
            ("GET", "/zzz", None, None),             // nothing anchored -> not(9)
            ("POST", "/zzz", None, None),
            ("GET", "/pre-mid-exact", None, None), // matches prefix(2) AND contains(3): first wins
        ];

        for (m, p, q, b) in requests {
            let linear = idx(imp.find_matching_stub_linear(m, p, &no_headers, *q, *b, None, None));
            let indexed =
                idx(imp.find_matching_stub_with_client(m, p, &no_headers, *q, *b, None, None));
            assert_eq!(
                indexed, linear,
                "index diverged from linear for {m} {p} q={q:?} b={b:?}"
            );
        }
    }

    // AC2 edge cases: the fold/normalization boundary where the index (Unicode `to_lowercase`) and
    // the `equals` evaluator (ASCII `eq_ignore_ascii_case`) differ, plus a path predicate nested in
    // `and` (must be fallback), multiple path predicates, and a trailing slash. No greedy `not` stub
    // here, so anchored stubs are actually reached and the boundary is exercised, not shadowed.
    #[test]
    fn indexed_matching_equals_linear_edge_cases() {
        let imp = imposter(&[
            json!([{"equals": {"path": "/café"}}]),  // 0 unicode exact
            json!([{"startsWith": {"path": "/A"}}]), // 1 prefix, uppercase anchor
            json!([{"and": [{"equals": {"method": "GET"}}, {"equals": {"path": "/andp"}}]}]), // 2 and -> fallback
            json!([{"equals": {"path": "/exact"}}, {"startsWith": {"path": "/exa"}}]), // 3 two path preds
            json!([{"contains": {"path": "/seg"}}]),                                   // 4 contains
            json!([{"equals": {"path": "/pm2"}}, {"equals": {"method": "GET"}}]), // 5 path anchor + separate method predicate
        ]);
        let no_headers: HashMap<String, String> = HashMap::new();
        let requests: &[(&str, &str)] = &[
            ("GET", "/café"),
            ("GET", "/CAFÉ"), // ASCII fold: É != é so equals rejects; index over-includes harmlessly
            ("GET", "/caFé"),
            ("GET", "/a1"),   // startsWith /A, case-insensitive
            ("GET", "/andp"), // and-nested path lives in fallback (stub 2)
            ("POST", "/andp"),
            ("GET", "/exact"), // stub 3: both path preds hold
            ("GET", "/exa"),   // startsWith /exa holds but equals /exact fails -> not stub 3
            ("GET", "/x/seg/y"),
            ("GET", "/exact/"), // trailing slash is not equal to /exact
            ("GET", "/andp/extra"),
            ("GET", "/pm2"), // stub 5: path anchor indexes it, separate method predicate holds
            ("POST", "/pm2"), // path-anchored candidate, but Stage-2 method predicate rejects -> None
        ];
        for (m, p) in requests {
            let linear =
                idx(imp.find_matching_stub_linear(m, p, &no_headers, None, None, None, None));
            let indexed =
                idx(imp.find_matching_stub_with_client(m, p, &no_headers, None, None, None, None));
            assert_eq!(indexed, linear, "index diverged from linear for {m} {p}");
        }
    }

    // The path dimension genuinely narrows (excludes non-matching anchored stubs) yet never drops a
    // stub the linear scan would consider (always-bits + matching anchors are all present).
    // Stub 6 is method-only (`equals {method: POST}`), so a GET request now prunes it on the method
    // dimension — the #707 pruning the path dimension alone could never do.
    #[test]
    fn stub_index_narrows_and_covers() {
        let snap = snapshot(&corpus());
        let cands = candidate_ids(&snap, "GET", "/exact");

        // Narrowing: the prefix (2) and method+path-/mp (11) anchored stubs cannot match /exact,
        // so they are excluded.
        assert!(!cands.contains(&2), "prefix /pre stub excluded for /exact");
        assert!(
            !cands.contains(&11),
            "method+path /mp stub excluded for /exact"
        );
        assert!(
            !cands.contains(&6),
            "POST-only stub excluded for a GET request (method dimension, #707)"
        );

        // Coverage: both exact stubs (case-insensitive collision) and every stub no dimension can
        // index remain candidates.
        assert!(
            cands.contains(&0) && cands.contains(&1),
            "exact stubs present"
        );
        for fb in [4, 5, 7, 8, 9, 10, 12] {
            assert!(
                cands.contains(&fb),
                "un-indexable stub {fb} must always be a candidate"
            );
        }
        // Ascending + deduped so Stage-2 preserves declaration order.
        let mut sorted = cands.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(cands, sorted, "candidates must be ascending and deduped");
    }

    // AC4: the method dimension collapses the candidate set — a request's method prunes every stub
    // anchored to a different one. This is the framework's proof: before #707 all four stubs were
    // candidates for every request, because the path dimension cannot see the method.
    #[test]
    fn method_dimension_collapses_candidates() {
        let snap = snapshot(&[
            json!([{"equals": {"method": "GET"}}]),
            json!([{"equals": {"method": "POST"}}]),
            json!([{"equals": {"method": "PUT"}}]),
            json!([{"equals": {"method": "DELETE"}}]),
        ]);
        for (i, m) in ["GET", "POST", "PUT", "DELETE"].iter().enumerate() {
            let c = snap.candidates(m, "/anything");
            assert_eq!(c.count(), 1, "{m}: exactly one candidate survives");
            assert!(c.contains(i), "{m}: the {m}-anchored stub is the survivor");
        }
        // A method no stub anchors prunes everything — and the early-exit path returns empty.
        assert_eq!(snap.candidates("PATCH", "/anything").count(), 0);
        // Case-insensitive, per Mountebank's default comparison.
        assert!(snap.candidates("get", "/anything").contains(0));
    }

    // AC6 (the soundness rule): a stub whose method constraint the dimension cannot index must stay
    // a candidate for EVERY method. A dimension may only exclude what it can prove cannot match —
    // anything else belongs in always_bits.
    #[test]
    fn ineligible_method_shapes_are_always_candidates() {
        let snap = snapshot(&[
            json!([{"equals": {"method": "GET"}}]), // 0 indexable
            json!([{"or": [{"equals": {"method": "GET"}}, {"equals": {"method": "POST"}}]}]), // 1 or
            json!([{"not": {"equals": {"method": "GET"}}}]), // 2 not
            json!([{"and": [{"equals": {"method": "GET"}}, {"equals": {"path": "/x"}}]}]), // 3 and
            json!([{"equals": {"method": "GET"}, "except": "X"}]), // 4 except rewrites the value
            json!([{"equals": {"method": "GET"}, "caseSensitive": true}]), // 5 caseSensitive
            json!([{"matches": {"method": "^G"}}]),          // 6 regex
            // 7: the evaluator stringifies a non-string value and compares it, so it IS a real
            // constraint — just not one this dimension indexes.
            json!([{"equals": {"method": 7}}]),
            json!([{"equals": {"method": "GET"}, "jsonpath": {"selector": "$.id"}}]), // 8 selector
            json!([]),                                                                // 9 match-all
        ]);
        // PATCH matches no indexable anchor, so only always-bits stubs may survive.
        let c = snap.candidates("PATCH", "/x");
        assert!(!c.contains(0), "the indexable GET stub must be pruned");
        for i in [1, 2, 3, 4, 5, 6, 7, 8, 9] {
            assert!(
                c.contains(i),
                "stub {i} is un-indexable → always a candidate"
            );
        }
    }

    // The `selector` arm of the shared `is_safely_indexable` gate, for the path dimension. Without
    // a test, inverting or dropping that check would be an *under*-approximation — a silently
    // pruned stub — which is the one failure mode the index must never have.
    #[test]
    fn selector_scoped_path_predicates_are_always_candidates() {
        let snap = snapshot(&[
            json!([{"equals": {"path": "/a"}, "jsonpath": {"selector": "$.id"}}]), // 0 selector
            json!([{"equals": {"path": "/a"}, "except": "X"}]),                    // 1 except
            json!([{"equals": {"path": "/a"}, "caseSensitive": true}]), // 2 caseSensitive
            json!([{"equals": {"path": "/a"}}]),                        // 3 indexable
        ]);
        // A path no anchor matches: only stubs the dimension cannot index may survive.
        let c = snap.candidates("GET", "/other");
        for i in [0, 1, 2] {
            assert!(
                c.contains(i),
                "stub {i} is un-indexable → always a candidate"
            );
        }
        assert!(!c.contains(3), "the indexable /a stub must be pruned");
    }

    // The index must fold case EXACTLY as the evaluator does (ASCII), not merely conservatively.
    // Unicode `to_lowercase` is length-changing and context-sensitive, so it breaks the prefix and
    // substring relations the path dimension relies on: the evaluator matches `startsWith "/ΟΣ"`
    // against `/ΟΣΑ` (its ASCII fold leaves Greek untouched), but a Unicode fold maps the anchor's
    // trailing Σ to a final sigma (`/ος`) that `"/οσα"` does not start with — pruning a stub that
    // does match. Regression test for that class of silent no-match.
    #[test]
    fn non_ascii_case_folding_matches_the_evaluator() {
        let imp = imposter(&[
            json!([{"startsWith": {"path": "/ΟΣ"}}]), // 0 Greek sigma: Unicode fold breaks the prefix
            json!([{"contains": {"path": "ΑΣ"}}]),    // 1 the same trap via contains
            json!([{"equals": {"path": "/İ"}}]),      // 2 dotted capital I lowercases to two chars
        ]);
        let no_headers: HashMap<String, String> = HashMap::new();
        for (m, p) in [
            ("GET", "/ΟΣΑ"),
            ("GET", "/ΟΣ"),
            ("GET", "/οσα"),
            ("GET", "/xΑΣy"),
            ("GET", "/İ"),
            ("GET", "/i̇"),
        ] {
            let linear =
                idx(imp.find_matching_stub_linear(m, p, &no_headers, None, None, None, None));
            let indexed =
                idx(imp.find_matching_stub_with_client(m, p, &no_headers, None, None, None, None));
            assert_eq!(indexed, linear, "index diverged from linear for {m} {p}");
        }
    }

    // An unusual method shares the `Other` slot with every other unusual method. That is coarser,
    // not wrong: the dimension over-includes and verification decides. Guards against a slot scheme
    // that silently drops methods outside the named set.
    #[test]
    fn unnamed_methods_share_the_other_slot_soundly() {
        let snap = snapshot(&[
            json!([{"equals": {"method": "TRACE"}}]),
            json!([{"equals": {"method": "CONNECT"}}]),
            json!([{"equals": {"method": "GET"}}]),
        ]);
        // Both unusual stubs are candidates for either unusual method (over-approximation)...
        for m in ["TRACE", "CONNECT"] {
            let c = snap.candidates(m, "/x");
            assert!(c.contains(0) && c.contains(1), "{m}: Other-slot stubs kept");
            assert!(!c.contains(2), "{m}: the GET stub is still pruned");
        }
        // ...but full verification still returns only the truly matching one.
        let imp = imposter(&[
            json!([{"equals": {"method": "TRACE"}}]),
            json!([{"equals": {"method": "CONNECT"}}]),
        ]);
        let no_headers: HashMap<String, String> = HashMap::new();
        assert_eq!(
            idx(imp.find_matching_stub_with_client(
                "CONNECT",
                "/x",
                &no_headers,
                None,
                None,
                None,
                None
            )),
            Some(1)
        );
    }

    // AC1: one load yields the stubs and an index built over those exact stubs. The two cannot
    // diverge — there is no second ArcSwap to tear against — and that must survive mutation.
    #[test]
    fn snapshot_stubs_and_index_are_one_unit() {
        let imp = imposter(&[json!([{"equals": {"path": "/a"}}])]);
        for n in 1..6usize {
            let snap = imp.snapshot();
            assert_eq!(
                snap.stubs().len(),
                snap.index().len(),
                "index spans exactly the stubs it was loaded with"
            );
            // Every candidate id must be a valid index into the same load's stub vector.
            let c = snap.candidates("GET", "/a");
            assert!(c.iter().all(|i| i < snap.stubs().len()));
            drop(snap);

            let stub = serde_json::from_value(json!({
                "predicates": [{"equals": {"path": format!("/p{n}")}}],
                "responses": [{ "is": { "statusCode": 200 } }]
            }))
            .expect("valid stub");
            imp.add_stub(stub, None);
        }
        let snap = imp.snapshot();
        assert_eq!(snap.stubs().len(), 6);
        assert_eq!(snap.index().len(), 6);
    }

    // AC3: rebuilding on stub reload keeps the index consistent with the new stubs.
    #[test]
    fn index_rebuilt_on_replace_stubs() {
        let imp = imposter(&[json!([{"equals": {"path": "/old"}}])]);
        let no_headers: HashMap<String, String> = HashMap::new();
        assert_eq!(
            idx(imp.find_matching_stub_with_client(
                "GET",
                "/old",
                &no_headers,
                None,
                None,
                None,
                None
            )),
            Some(0)
        );

        let new_stub =
            serde_json::from_value(json!({ "predicates": [{"equals": {"path": "/new"}}], "responses": [{ "is": { "statusCode": 200 } }] }))
                .expect("valid stub");
        imp.replace_stubs(vec![new_stub]);

        // Old path no longer matches; new path does — proves the index was rebuilt, not stale.
        assert_eq!(
            idx(imp.find_matching_stub_with_client(
                "GET",
                "/old",
                &no_headers,
                None,
                None,
                None,
                None
            )),
            None
        );
        assert_eq!(
            idx(imp.find_matching_stub_with_client(
                "GET",
                "/new",
                &no_headers,
                None,
                None,
                None,
                None
            )),
            Some(0)
        );
    }

    // AC2: a match-all (empty-predicate) stub declared BEFORE an anchored stub must still win —
    // the index (fallback, low index) can never let a higher-index anchor jump declaration order.
    #[test]
    fn match_all_before_anchor_wins() {
        let imp = imposter(&[
            json!([]),                           // 0 match-all (fallback)
            json!([{"equals": {"path": "/a"}}]), // 1 exact anchor
        ]);
        let no_headers: HashMap<String, String> = HashMap::new();
        // /a matches both; the earlier match-all (stub 0) wins in both the indexed and linear paths.
        assert_eq!(
            idx(imp.find_matching_stub_linear("GET", "/a", &no_headers, None, None, None, None)),
            Some(0)
        );
        assert_eq!(
            idx(imp.find_matching_stub_with_client(
                "GET",
                "/a",
                &no_headers,
                None,
                None,
                None,
                None
            )),
            Some(0),
            "the earlier match-all stub must win over the anchored stub"
        );
    }

    /// A randomized stub corpus spanning every dimension the index prunes on (method, path) and
    /// every shape it must *not* prune on (regex, body, exists, or/not/and, caseSensitive, except,
    /// empty). Seeded, so a differential failure is reproducible from the seed alone.
    fn random_corpus(rng: &mut StdRng, n: usize) -> Vec<Value> {
        const METHODS: &[&str] = &["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "TRACE"];
        const SEGS: &[&str] = &["/a", "/b", "/api/v1", "/api/v2", "/x/y", "/mid"];
        (0..n)
            .map(|_| {
                let seg = SEGS[rng.gen_range(0..SEGS.len())];
                let m = METHODS[rng.gen_range(0..METHODS.len())];
                match rng.gen_range(0..12) {
                    // Indexable on both dimensions (one predicate, two fields).
                    0 => json!([{"equals": {"method": m, "path": seg}}]),
                    // Indexable on both dimensions (two separate top-level predicates).
                    1 => json!([{"equals": {"method": m}}, {"equals": {"path": seg}}]),
                    // Method-only: prunable by method, always-candidate on path.
                    2 => json!([{"equals": {"method": m}}]),
                    // Path-only: prunable by path, always-candidate on method.
                    3 => json!([{"equals": {"path": seg}}]),
                    4 => json!([{"startsWith": {"path": seg}}]),
                    5 => json!([{"contains": {"path": seg}}]),
                    // Not indexable on either dimension — must be always-candidate.
                    6 => json!([{"matches": {"path": format!("^{seg}[0-9]*$")}}]),
                    7 => json!([{"or": [{"equals": {"method": m}}, {"equals": {"path": seg}}]}]),
                    8 => json!([{"not": {"equals": {"path": seg}}}]),
                    9 => json!([{"equals": {"method": m}, "caseSensitive": true}]),
                    10 => json!([{"equals": {"method": m}, "except": "X"}]),
                    _ => json!([]),
                }
            })
            .collect()
    }

    // AC2 (the load-bearing correctness gate): over a randomized corpus, the indexed path must
    // return exactly the stub the linear oracle returns — same index, same first-match-wins order —
    // for every request. This is a *characterization* gate: it holds for the pre-#707 index too, and
    // must keep holding through the snapshot/bitset refactor and every dimension added on top of it
    // (#708/#709/#710). Any dimension that under-approximates (prunes a stub that could match)
    // fails here.
    #[test]
    fn differential_index_matches_linear_oracle() {
        const METHODS: &[&str] = &[
            "GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "TRACE", "get",
        ];
        const PATHS: &[&str] = &[
            "/a",
            "/b",
            "/A",
            "/api/v1",
            "/api/v2",
            "/api/v1/deep",
            "/x/y",
            "/x-mid-y",
            "/mid",
            "/zzz",
            "/a0",
            "/GETX",
        ];
        let mut rng = StdRng::seed_from_u64(0x0707_5EED);
        let no_headers: HashMap<String, String> = HashMap::new();

        // Several independent corpora, so the assertion isn't hostage to one lucky stub layout.
        for corpus_n in 0..8 {
            let imp = imposter(&random_corpus(&mut rng, 40));
            for _ in 0..1250 {
                let m = METHODS[rng.gen_range(0..METHODS.len())];
                let p = PATHS[rng.gen_range(0..PATHS.len())];
                let body = if rng.gen_bool(0.25) {
                    Some("ping")
                } else {
                    None
                };
                let query = if rng.gen_bool(0.25) {
                    Some("a=1")
                } else {
                    None
                };

                let linear =
                    idx(imp.find_matching_stub_linear(m, p, &no_headers, query, body, None, None));
                let indexed = idx(imp.find_matching_stub_with_client(
                    m,
                    p,
                    &no_headers,
                    query,
                    body,
                    None,
                    None,
                ));
                assert_eq!(
                    indexed, linear,
                    "index diverged from linear oracle: corpus {corpus_n}, {m} {p} q={query:?} b={body:?}"
                );
            }
        }
    }

    // Issue #475: the has_scenario_gate flag — computed once at index build — detects a
    // `requiredScenarioState` stub so the bounded matcher offloads the gate's flow-store read to
    // spawn_blocking on a blocking backend, while a scenario-free set keeps the inline fast path.
    #[test]
    fn has_scenario_gate_detects_required_scenario_state() {
        let build = |v: Value| {
            let states: Vec<Arc<StubState>> = v
                .as_array()
                .expect("array")
                .iter()
                .map(|s| {
                    Arc::new(StubState::new(
                        serde_json::from_value(s.clone()).expect("stub"),
                    ))
                })
                .collect();
            StubSnapshot::build(states)
        };
        let ungated = build(json!([
            { "predicates": [{"equals": {"path": "/a"}}], "responses": [{"is": {"statusCode": 200}}] }
        ]));
        assert!(!ungated.has_scenario_gate());

        let gated = build(json!([
            {
                "predicates": [{"equals": {"path": "/a"}}],
                "scenarioName": "sc",
                "requiredScenarioState": "started",
                "responses": [{"is": {"statusCode": 200}}]
            }
        ]));
        assert!(gated.has_scenario_gate());
    }

    // Issue #476: the has_inject gate — computed once at index build — detects an inject
    // predicate anywhere in a stub's predicate tree, including nested under and/or/not, and
    // stays false for scriptless stub sets so they keep the inline matching fast path.
    #[test]
    fn has_inject_detects_top_level_and_nested() {
        let scriptless = StubSnapshot::build(stub_states(&[
            json!([{"equals": {"path": "/a"}}]),
            json!([{"and": [{"equals": {"path": "/b"}}, {"exists": {"query": {"q": true}}}]}]),
        ]));
        assert!(!scriptless.has_inject());

        let top_level = StubSnapshot::build(stub_states(&[
            json!([{"equals": {"path": "/a"}}]),
            json!([{"inject": "function (config) { return true; }"}]),
        ]));
        assert!(top_level.has_inject());

        let under_and = StubSnapshot::build(stub_states(&[json!([
            {"and": [{"equals": {"path": "/a"}}, {"inject": "function (config) { return true; }"}]}
        ])]));
        assert!(under_and.has_inject());

        let under_not_in_or = StubSnapshot::build(stub_states(&[json!([
            {"or": [{"equals": {"path": "/a"}}, {"not": {"inject": "function (config) { return true; }"}}]}
        ])]));
        assert!(under_not_in_or.has_inject());
    }
}
