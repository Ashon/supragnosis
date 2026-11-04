//! The sync client: one round against a host - advertise, push the surplus, pull the deficit.

use super::*;

/// Result of one client sync round against a server.
#[derive(Debug, Default, Serialize)]
pub struct SyncSummary {
    pub pushed: usize,
    pub pulled: usize,
    pub rejected_by_server: usize,
    pub rejected_locally: usize,
}

/// The sync client (reqwest): advertise -> push my surplus -> pull my deficit -> apply locally.
pub struct SyncClient {
    base: String,
    token: String,
    http: reqwest::Client,
    /// Keys a hub vouched for at a join, by origin, and the last sequence each verifies in each
    /// workspace (docs/pairing.md Section 4.3.1).
    vouched: BTreeMap<String, BTreeMap<String, u64>>,
}

impl SyncClient {
    /// A client that verifies the host against the system's roots. Every client verifies: there is
    /// no setting that accepts any certificate (sync-correctness.md Section 10).
    pub fn new(
        base_url: impl Into<String>,
        token: impl Into<String>,
    ) -> Result<Self, TransportError> {
        Self::build(base_url, token, None)
    }

    /// The client for one configured host: its credential, and the certificate it trusts for it.
    pub fn for_link(link: &crate::ServerLink) -> Result<Self, TransportError> {
        Self::build(&link.url, &link.auth_token, link.ca_pem.as_deref())
    }

    fn build(
        base_url: impl Into<String>,
        token: impl Into<String>,
        ca_pem: Option<&[u8]>,
    ) -> Result<Self, TransportError> {
        // Same CryptoProvider pin as the server side (idempotent) - the client dials HTTPS hubs.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let mut builder = reqwest::Client::builder();
        // A named certificate is how a self-signed hub is trusted without trusting every hub.
        if let Some(pem) = ca_pem {
            builder = builder.use_preconfigured_tls(tls_trusting(pem)?);
        }
        let http = builder.timeout(CALL_TIMEOUT).build()?;
        Ok(Self {
            base: base_url.into().trim_end_matches('/').to_string(),
            token: token.into(),
            http,
            vouched: BTreeMap::new(),
        })
    }

    /// Verifies the vouched origins' events only up to their bound in each workspace, and none in
    /// a workspace without one (docs/pairing.md Section 4.3.1). Their keys must also be among the
    /// origin keys a sync is given; without this, those keys would verify unbounded.
    pub fn with_vouched(mut self, vouched: BTreeMap<String, BTreeMap<String, u64>>) -> Self {
        self.vouched = vouched;
        self
    }

    async fn call<Req: Serialize, Resp: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        req: &Req,
    ) -> Result<Resp, TransportError> {
        self.call_for(path, req, None).await
    }

    /// A workspace request, saying no to serving that workspace to a 0.10 hub's principals
    /// ([`SERVE_HEADER`]).
    async fn call_for<Req: Serialize, Resp: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        req: &Req,
        workspace: Option<&str>,
    ) -> Result<Resp, TransportError> {
        let mut builder = self
            .http
            .post(format!("{}{}", self.base, path))
            .bearer_auth(&self.token)
            .header(PROTOCOL_HEADER, PROTOCOL)
            .header(RELEASE_HEADER, env!("CARGO_PKG_VERSION"))
            .json(req);
        if workspace.is_some() {
            builder = builder.header(SERVE_HEADER, "no");
        }
        let resp = builder.send().await?;
        let status = resp.status();
        if !status.is_success() {
            return Err(TransportError::Remote {
                status: status.as_u16(),
                body: resp.text().await.unwrap_or_default(),
            });
        }
        Ok(resp.json().await?)
    }

    pub async fn advertise(&self, workspace: &str) -> Result<AdvertiseResp, TransportError> {
        self.call_for(
            "/sync/advertise",
            &AdvertiseReq { workspace: workspace.into() },
            Some(workspace),
        )
        .await
    }

    pub async fn pull(
        &self,
        workspace: &str,
        since: &VersionVector,
        limit: usize,
    ) -> Result<PullResp, TransportError> {
        let req = PullReq { workspace: workspace.into(), since: since.clone(), limit: Some(limit) };
        self.call_for("/sync/pull", &req, Some(workspace)).await
    }

    /// Pulls page after page of what the host holds beyond `since`, handing each page to `apply`
    /// (sync-correctness.md Section 11). A stream with a rejection that holds (Section 16) in a page is
    /// held: the request claims it whole for the rest of this round, so its later events are neither applied past the
    /// rejected one nor sent again and again in place of the other streams. The next round asks from
    /// the store's own version vector, which is below the rejected event, and offers it again.
    pub async fn pull_all<F, Fut>(
        &self,
        workspace: &str,
        mut since: VersionVector,
        mut apply: F,
    ) -> Result<ApplyReport, TransportError>
    where
        F: FnMut(Vec<serde_json::Value>) -> Fut,
        Fut: std::future::Future<Output = Result<ApplyReport, TransportError>>,
    {
        let mut total = ApplyReport::default();
        loop {
            let page = self.pull(workspace, &since, PAGE_EVENTS).await?;
            if page.events.is_empty() {
                break;
            }
            let streams: Vec<(String, u64)> = page.events.iter().map(stream_of).collect();
            let report = apply(page.events).await?;
            let held: std::collections::HashSet<&str> = report
                .rejected
                .iter()
                .filter(|r| r.reason.holds())
                .map(|r| r.origin_node.as_str())
                .collect();
            let before = since.clone();
            for (origin, seq) in streams.iter().filter(|(o, _)| !o.is_empty()) {
                let to = if held.contains(origin.as_str()) { u64::MAX } else { *seq };
                since.advance(origin, workspace, to);
            }
            total.accepted += report.accepted;
            total.rejected.extend(report.rejected);
            // A host that says there is more but sent nothing new is not asked again this round.
            if !page.more || since == before {
                break;
            }
        }
        Ok(total)
    }

    /// Pushes `events` in batches (sync-correctness.md Section 11), in (origin, seq) order. A stream
    /// with a rejection that holds at the host in one batch is not sent further this round: the host's version vector stays
    /// below the rejected event, so it and everything behind it are offered again next round.
    pub async fn push(
        &self,
        workspace: &str,
        mut events: Vec<AttestationEvent>,
    ) -> Result<PushResp, TransportError> {
        events.sort_by_cached_key(|e| {
            e.attestation.sync.as_ref().map(|m| (m.origin_node.clone(), m.origin_seq))
        });
        let mut total = PushResp { accepted: 0, rejected: Vec::new() };
        let mut held: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut batch: Vec<serde_json::Value> = Vec::new();
        let mut bytes = 0usize;
        for ev in to_wire(events) {
            if held.contains(&stream_of(&ev).0) {
                continue;
            }
            let size = serde_json::to_vec(&ev).map(|v| v.len()).unwrap_or(0);
            if !batch.is_empty() && (batch.len() >= BATCH_EVENTS || bytes + size > BATCH_BYTES) {
                let sent = std::mem::take(&mut batch);
                self.push_batch(workspace, sent, &mut total, &mut held).await?;
                bytes = 0;
                if held.contains(&stream_of(&ev).0) {
                    continue;
                }
            }
            bytes += size;
            batch.push(ev);
        }
        if !batch.is_empty() {
            self.push_batch(workspace, batch, &mut total, &mut held).await?;
        }
        Ok(total)
    }

    async fn push_batch(
        &self,
        workspace: &str,
        events: Vec<serde_json::Value>,
        total: &mut PushResp,
        held: &mut std::collections::HashSet<String>,
    ) -> Result<(), TransportError> {
        let req = PushReq { workspace: workspace.into(), events };
        let resp: PushResp = self.call_for("/sync/push", &req, Some(workspace)).await?;
        held.extend(
            resp.rejected
                .iter()
                .filter(|r| holds_at_the_host(&r.2))
                .map(|r| r.0.clone())
                .filter(|o| !o.is_empty()),
        );
        total.accepted += resp.accepted;
        total.rejected.extend(resp.rejected);
        Ok(())
    }

    /// Health check: verifies connectivity, auth, and per-workspace authorization in one call.
    pub async fn ping(&self) -> Result<PingResp, TransportError> {
        self.call("/sync/ping", &serde_json::json!({})).await
    }

    /// Federated retrieval: search the SERVER's ontology (its retrieval surface, mode-labeled). Results
    /// are remote ids - pull the workspace to materialize them locally before traverse/get_entity.
    pub async fn search(
        &self,
        workspace: &str,
        query: &str,
        limit: usize,
    ) -> Result<SearchResp, TransportError> {
        self.call(
            "/sync/search",
            &SearchReq { workspace: workspace.into(), query: query.into(), limit },
        )
        .await
    }

    /// One full sync round for a workspace: learn what the server holds, backfill-stamp local
    /// knowledge, push what the server lacks (per its advertised VV), pull what we lack, and apply it
    /// (verify -> CAS -> VV). Re-materialization (engine reproject) is the caller's follow-up -
    /// transport moves the log.
    ///
    /// The server is asked first so its copy of this node's own stream floors the seq counter before
    /// anything is stamped (sync-correctness.md Section 5): a node whose store lost stamps it had
    /// already sent continues past them instead of issuing their seqs again.
    pub async fn sync_workspace(
        &self,
        store: &Arc<dyn AssertionStore>,
        node: &SyncNode,
        workspace: &str,
        share_workspaces: &[String],
        origin_keys: &BTreeMap<String, String>,
    ) -> Result<SyncSummary, TransportError> {
        let remote = self.advertise(workspace).await?;
        node.floor_seq(store.as_ref(), workspace, remote.vv.get(node.node_id(), workspace))?;
        node.backfill(store.as_ref(), workspace)?;
        let mine = version_vector(store.as_ref(), workspace)?;
        let surplus = export_delta(store.as_ref(), workspace, &remote.vv, share_workspaces)?;
        let mut summary = SyncSummary::default();
        if !surplus.is_empty() {
            let resp = self.push(workspace, surplus).await?;
            summary.pushed = resp.accepted;
            summary.rejected_by_server = resp.rejected.len();
        }
        let report = self
            .pull_all(workspace, mine, |events| {
                let mut vv = VersionVector::default();
                std::future::ready(
                    node.apply_wire_bounded(
                        store.as_ref(),
                        workspace,
                        events,
                        origin_keys,
                        &crate::bounds_in(&self.vouched, workspace),
                        None,
                        &mut vv,
                    )
                    .map_err(TransportError::from),
                )
            })
            .await?;
        summary.pulled = report.accepted;
        summary.rejected_locally = report.rejected.len();
        Ok(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// remote-server.md Section 13: a 0.11 node says no to serving on each workspace request, so a
    /// 0.10 hub withdraws the consent an earlier release gave it.
    #[tokio::test]
    async fn a_node_says_no_to_serving_on_every_workspace_request() {
        let said = Arc::new(std::sync::Mutex::new(Vec::<Option<String>>::new()));
        let log = said.clone();
        let app = Router::new().route(
            "/sync/advertise",
            axum::routing::post(move |headers: HeaderMap| {
                let log = log.clone();
                async move {
                    log.lock().unwrap().push(
                        headers.get(SERVE_HEADER).and_then(|v| v.to_str().ok()).map(str::to_string),
                    );
                    StatusCode::IM_A_TEAPOT
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await });
        let client = SyncClient::new(format!("http://{addr}"), "token").unwrap();
        let _ = client.advertise("team").await;
        let _ = client.advertise("private").await;
        assert_eq!(*said.lock().unwrap(), vec![Some("no".to_string()), Some("no".to_string())]);
    }
}
