//! Search and traversal results, and the keyword rule every store adapter shares.

use serde::{Deserialize, Serialize};

/// A single search result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub kind: SearchHitKind,
    pub id: String,
    pub snippet: String,
    /// Ranking score - **for ranking comparison only**. The scale differs per search surface (keyword constant,
    /// cosine similarity, RRF fusion value). Interpreting the absolute value as confidence/certainty is a misreading.
    pub score: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchHitKind {
    Entity,
    Observation,
}

/// How a text matched a keyword query (architecture.md Section 4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeywordMatch {
    /// The text contains the whole query.
    Phrase,
    /// The text contains every term of the query, in any order and anywhere.
    AllTerms,
}

/// The keyword surface's matching rule, shared by every store adapter so two backends cannot
/// disagree about what matches (architecture.md Section 4.2). A text matches when it contains the
/// whole query, case-folded, or every whitespace-separated term of it. The rule only widens: a
/// phrase contains each of its own terms, so a single-term query behaves as a plain substring match.
#[derive(Debug, Clone)]
pub struct KeywordQuery {
    phrase: String,
    /// Distinct terms in query order. Empty for an all-whitespace query, which the phrase (the
    /// empty string, contained in everything) already decides.
    terms: Vec<String>,
}

impl KeywordQuery {
    pub fn new(query: &str) -> Self {
        let lowered = query.to_lowercase();
        // Runs of whitespace are one space, so `boot   crash` is still a phrase of `the boot crash`.
        let phrase = lowered.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut terms: Vec<String> = Vec::new();
        for t in lowered.split_whitespace() {
            if !terms.iter().any(|x| x == t) {
                terms.push(t.to_string());
            }
        }
        Self { phrase, terms }
    }

    /// How `text` matches, if it does. The phrase is checked first, so a text that holds the query
    /// verbatim is never reported as the weaker all-terms match.
    pub fn matches(&self, text: &str) -> Option<KeywordMatch> {
        let text = text.to_lowercase();
        if text.contains(&self.phrase) {
            Some(KeywordMatch::Phrase)
        } else if !self.terms.is_empty() && self.terms.iter().all(|t| text.contains(t.as_str())) {
            Some(KeywordMatch::AllTerms)
        } else {
            None
        }
    }

    /// The strongest match over several spellings of one thing - an entity's canonical name and
    /// its aliases. Each spelling is matched on its own, so terms never combine across two names.
    pub fn best_of<'a>(&self, texts: impl IntoIterator<Item = &'a str>) -> Option<KeywordMatch> {
        let mut best = None;
        for t in texts {
            match self.matches(t) {
                Some(KeywordMatch::Phrase) => return Some(KeywordMatch::Phrase),
                Some(KeywordMatch::AllTerms) => best = Some(KeywordMatch::AllTerms),
                None => {}
            }
        }
        best
    }

    /// The ranking score of a keyword hit: a phrase above an all-terms match of the same kind,
    /// entities above observations (architecture.md Section 4.2). For ordering only - see
    /// [`SearchHit::score`].
    pub fn score(kind: SearchHitKind, m: KeywordMatch) -> f32 {
        match (kind, m) {
            (SearchHitKind::Entity, KeywordMatch::Phrase) => 1.0,
            (SearchHitKind::Entity, KeywordMatch::AllTerms) => 0.9,
            (SearchHitKind::Observation, KeywordMatch::Phrase) => 0.7,
            (SearchHitKind::Observation, KeywordMatch::AllTerms) => 0.6,
        }
    }
}

/// A single graph-traversal result (an entity `depth` hops away from the start entity).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraverseHit {
    pub id: String,
    pub depth: usize,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The keyword rule only widens (architecture.md Section 4.2): what held the query verbatim
    /// still matches, and as a phrase; every term is required; a single term is a plain substring.
    #[test]
    fn keyword_query_widens_and_requires_every_term() {
        let q = KeywordQuery::new("  Boot   CRASH ");
        assert_eq!(q.matches("the boot crash again"), Some(KeywordMatch::Phrase));
        assert_eq!(q.matches("crash on every boot"), Some(KeywordMatch::AllTerms));
        assert_eq!(q.matches("boot only"), None, "every term is required");
        assert_eq!(KeywordQuery::new("crash").matches("crashes"), Some(KeywordMatch::Phrase));
        assert_eq!(
            KeywordQuery::new("crash crash").matches("a crash"),
            Some(KeywordMatch::AllTerms),
            "a repeated term is one term"
        );
        assert_eq!(
            KeywordQuery::new("alpha beta").best_of(["Alpha", "Beta"]),
            None,
            "terms never combine across spellings"
        );
        assert_eq!(
            KeywordQuery::new("beta").best_of(["Alpha", "Beta"]),
            Some(KeywordMatch::Phrase)
        );
        let s = |k, m| KeywordQuery::score(k, m);
        assert!(
            s(SearchHitKind::Entity, KeywordMatch::Phrase)
                > s(SearchHitKind::Entity, KeywordMatch::AllTerms)
        );
        assert!(
            s(SearchHitKind::Entity, KeywordMatch::AllTerms)
                > s(SearchHitKind::Observation, KeywordMatch::Phrase)
        );
        assert!(
            s(SearchHitKind::Observation, KeywordMatch::Phrase)
                > s(SearchHitKind::Observation, KeywordMatch::AllTerms)
        );
    }
}
