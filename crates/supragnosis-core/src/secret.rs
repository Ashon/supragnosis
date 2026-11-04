//! The secret detector at ingest (Principle 17): credential-shaped text is found and named,
//! never quoted.

/// A credential-shaped run of text found by [`detect_secret`]: which pattern matched, and where.
///
/// It deliberately does NOT carry the matched text. An error naming what it found would put the
/// secret into the caller's logs, the agent's transcript and any transport in between - re-leaking
/// the thing the check exists to keep out. Same rule the excision tombstone follows for its reason
/// field (excision.md E2): name the shape and the place, never the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecretHit {
    /// A short, stable name for the shape that matched - safe to log and to show an agent.
    pub pattern: &'static str,
    /// Byte offset of the match, so a caller can point at it without quoting it.
    pub at: usize,
}

/// Detects credential-shaped text (Principle 17: the secret-redaction hook at ingest).
///
/// **Refuse, never rewrite.** P1 forbids transforming an assertion before it reaches the log -
/// notation is preserved verbatim and normalization is the projection's job - so this returns a
/// finding and the caller refuses the write. Redacting in place would also change the content and
/// therefore the content address (P14), which is a second reason it cannot be done here.
///
/// Refusing does not conflict with P1's ban on content-grounded rejection either: what that clause
/// protects is an assertion's *claim* - its notation, its truth, its schema conformance. A leaked
/// credential is none of those. It is material whose presence is the harm, and P1 itself argues for
/// catching it here, since the append-only log (P3) makes "before ingest" the only moment that works.
///
/// It is also NOT the case that Appendix A's "do not block ingest; defend with trust" governs this.
/// That row is about suspicious *knowledge*, where quarantining at the lowest tier reduces the harm
/// because the harm is believing a falsehood. A low-tier secret is still a readable secret and still
/// replicates, so a tier does nothing here.
///
/// **Patterns are deliberately narrow.** Only shapes that are close to self-identifying: a generic
/// entropy heuristic would fire on hashes, ids and base64 payloads, and a detector the operator learns
/// to override is worse than none. False negatives are expected and acceptable - this is defence in
/// depth (P17), an aid to the sharing filter rather than a replacement for it.
pub fn detect_secret(text: &str) -> Option<SecretHit> {
    let b = text.as_bytes();
    let hit = |pattern, at| Some(SecretHit { pattern, at });

    // A PEM private key block. The two halves are checked in order so "PRIVATE KEY" in prose does not
    // match on its own.
    if let Some(i) = find(b, b"-----BEGIN") {
        if find(&b[i..], b"PRIVATE KEY-----").is_some() {
            return hit("pem-private-key", i);
        }
    }
    for (prefix, len, set) in [
        // AWS access key id: AKIA + 16 uppercase alphanumerics.
        (&b"AKIA"[..], 16usize, CharSet::UpperNum),
        // Google API key: AIza + 35 url-safe characters.
        (&b"AIza"[..], 35, CharSet::UrlSafe),
    ] {
        if let Some(i) = find_run(b, prefix, len, set) {
            return hit(if prefix == b"AKIA" { "aws-access-key-id" } else { "google-api-key" }, i);
        }
    }
    for prefix in [&b"ghp_"[..], b"gho_", b"ghs_", b"ghu_", b"ghr_"] {
        if let Some(i) = find_run(b, prefix, 36, CharSet::UrlSafe) {
            return hit("github-token", i);
        }
    }
    for prefix in [&b"xoxb-"[..], b"xoxp-", b"xoxa-", b"xoxr-"] {
        if let Some(i) = find_run(b, prefix, 10, CharSet::UrlSafe) {
            return hit("slack-token", i);
        }
    }
    for prefix in [&b"sk_live_"[..], b"rk_live_"] {
        if let Some(i) = find_run(b, prefix, 16, CharSet::UrlSafe) {
            return hit("stripe-live-key", i);
        }
    }
    // A JWT's first segment is the base64 of `{"alg":"...`, which starts with this fixed run.
    if let Some(i) = find(b, b"eyJhbGciOi") {
        return hit("jwt", i);
    }
    if let Some(i) = find_url_credentials(b) {
        return hit("url-inline-credentials", i);
    }
    None
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CharSet {
    UpperNum,
    UrlSafe,
}

impl CharSet {
    fn has(self, c: u8) -> bool {
        match self {
            CharSet::UpperNum => c.is_ascii_uppercase() || c.is_ascii_digit(),
            CharSet::UrlSafe => c.is_ascii_alphanumeric() || c == b'_' || c == b'-',
        }
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// A prefix followed by at least `len` characters from `set` - the shape almost every issued token
/// takes, and specific enough that prose does not reach it.
fn find_run(hay: &[u8], prefix: &[u8], len: usize, set: CharSet) -> Option<usize> {
    let mut from = 0;
    while let Some(rel) = find(&hay[from..], prefix) {
        let i = from + rel;
        let run = hay[i + prefix.len()..].iter().take_while(|c| set.has(**c)).count();
        if run >= len {
            return Some(i);
        }
        from = i + 1;
    }
    None
}

/// `scheme://user:password@host` - a connection string with the password inlined. The userinfo must
/// contain a colon and a non-empty password, so `ssh://git@host` and a bare `a:b@c` in prose do not
/// match.
fn find_url_credentials(hay: &[u8]) -> Option<usize> {
    let mut from = 0;
    while let Some(rel) = find(&hay[from..], b"://") {
        let start = from + rel + 3;
        let rest = &hay[start..];
        let end = rest
            .iter()
            .position(|c| matches!(c, b'/' | b' ' | b'\n' | b'"'))
            .unwrap_or(rest.len());
        let userinfo_end = rest[..end].iter().position(|c| *c == b'@');
        if let Some(at) = userinfo_end {
            if let Some(colon) = rest[..at].iter().position(|c| *c == b':') {
                if at > colon + 1 {
                    return Some(start);
                }
            }
        }
        from = start;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The detector fires on credential shapes and stays quiet on the text a knowledge base is made
    /// of. The negative half is the important one: a detector that cries wolf gets switched off, and
    /// a switched-off detector is worse than none because it is believed to be running.
    #[test]
    fn detect_secret_finds_credentials_without_firing_on_prose() {
        let fires = [
            ("-----BEGIN RSA PRIVATE KEY-----\nMIIEow...", "pem-private-key"),
            ("aws key AKIAIOSFODNN7EXAMPLE in the config", "aws-access-key-id"),
            ("token ghp_1234567890abcdefghijklmnopqrstuvwxyz here", "github-token"),
            ("xoxb-2411-2411-abcdefghij", "slack-token"),
            // Split for the same reason as the Stripe key below. This one got through push
            // protection and was then reported by secret scanning after the fact, which is the
            // same collision arriving one step later: a scanner reads the shape, and a detector's
            // test suite is made of shapes.
            (concat!("AIza", "SyA1234567890abcdefghijklmnopqrstuv"), "google-api-key"),
            // Split from its prefix and rejoined by concat!. This is Stripe's own published
            // example key, but GitHub push protection scans the diff rather than the intent, so a
            // contiguous `sk_live_...` here blocks every push that touches this file - one detector
            // refusing another detector's test suite. detect_secret still sees the whole string.
            (concat!("sk_live", "_4eC39HqLyjWDarjtT1zdp7dc"), "stripe-live-key"),
            ("Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.x.y", "jwt"),
            (
                "DATABASE_URL=postgres://admin:hunter2@db.internal:5432/app",
                "url-inline-credentials",
            ),
        ];
        for (text, want) in fires {
            let hit = detect_secret(text).unwrap_or_else(|| panic!("missed a {want}: {text}"));
            assert_eq!(hit.pattern, want, "wrong pattern for: {text}");
        }

        let quiet = [
            // Ordinary knowledge, including the words and shapes that a naive detector trips on.
            "the deploy reads DATABASE_URL from the environment",
            "rotate the PRIVATE KEY quarterly - see the runbook",
            "https://github.com/ashon/supragnosis",
            "ssh://git@github.com/ashon/supragnosis.git",
            "blake3 id d20747da56a857fbcfb6c76429e895d5a160fa95a117c21e0c9105001ad27ce2",
            "AKIA is the prefix AWS uses for an access key id",
            "the ratio is 3:1@peak",
            "base64 payload eyJ3b3Jrc3BhY2UiOiJ3cyJ9 in the proposal",
        ];
        for text in quiet {
            assert_eq!(detect_secret(text), None, "false positive on: {text}");
        }
    }

    /// A finding names the shape and the place, never the value - so that refusing a write cannot
    /// itself publish the secret into a log, a transcript or an error channel. The failure message
    /// leaves the finding out for the same reason: a finding that failed here holds the secret.
    #[test]
    fn a_finding_never_carries_the_secret() {
        let text = "key AKIAIOSFODNN7EXAMPLE";
        let hit = detect_secret(text).expect("hit");
        assert!(
            !format!("{hit:?}").contains("AKIAIOSFODNN7EXAMPLE"),
            "the finding quoted the secret"
        );
        assert_eq!(
            &text[hit.at..hit.at + 4],
            "AKIA",
            "but it does point at where the match starts"
        );
    }
}
