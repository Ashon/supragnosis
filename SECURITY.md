# Security Policy

## Reporting a vulnerability

Please report privately, not as a public issue.

Use GitHub's [private vulnerability reporting](https://github.com/Ashon/supragnosis/security/advisories/new)
on this repository. If that is unavailable to you, open a public issue containing only a
request for a private channel, with no details.

Expect an acknowledgement within a week. This is a small project, so please size your
expectations accordingly: there is no on-call rotation and no guaranteed patch window. A fix
lands in the next release, and the release notes name the issue.

## What is in scope

supragnosis is a local-first knowledge server. Its threat model has three surfaces.

**The MCP surface.** The HTTP daemon binds loopback only and has no authentication layer,
because the local machine is its trust surface. Anything that reaches it from off-host, or
that lets a remote origin drive it, is in scope. So is a tool that reads or writes outside
the workspace scope it was given.

**The viewer surface.** The viewer has no TCP port. It serves HTTP over a unix socket whose
0600 mode is the access control. The desktop shell proxies a webview onto that socket through
a `viz://` protocol handler. Anything that lets a third-party origin reach the socket, or that
launders the `/api/review` verdict-surface ceiling so an agent path can grant what only the
local principal may grant, is in scope.

**The federation surface.** Every attestation is ed25519-signed by its origin, and a receiver
recomputes the content id from (workspace, content, assertions) before verifying, so a forged
id cannot ride the wire. In scope: signature verification bypass, a relay forging or stripping
the signed lineage declaration undetected, a peer reading a workspace it was not authorized
for, a claimed trust tier that evaluates above what the transport can prove, and any path that
makes two nodes holding the same event set diverge.

Also in scope: a path that writes into the graph without passing through an assertion, a way
to make an ingest refusal publish the value it refused, and any bypass of the proposal gate for
a change that affects the canon.

## What is out of scope

**A non-loopback bind you configured yourself.** A federation server binding off-loopback
requires TLS and a non-empty allowlist and refuses to start without both. Overriding that in
your own deployment is your decision, not a vulnerability.

**Credential detection false negatives.** The ingest detector matches narrow, close to
self-identifying shapes. It is defence in depth for the sharing filter, not a replacement, and
false negatives are expected and documented. A *false negative report* is welcome as a normal
issue. A pattern that a generic entropy heuristic would catch is deliberately not added,
because a detector operators learn to override is worse than none.

**Content you asserted.** The system stores what you tell it and preserves notation verbatim.
That an observation contains something you regret is not a vulnerability in the store; see
[docs/excision.md](docs/excision.md) for where that lands.

**Denial of service by volume on a local socket.** The trust surface is the local machine.

## Known limitations

Stated here rather than discovered by a reporter:

- The MCP HTTP daemon has no authentication. It stays loopback-bound for exactly that reason.
  An authenticated non-loopback MCP tier is not built.
- Multi-principal governance is not implemented. A shared workspace today assumes one
  principal; the T-Box gate and the canon-policy artifact that would change this are deferred.
  See [docs/architecture.md](docs/architecture.md) Section 14.
- The tombstone for a regulation or privacy destruction demand is specified in
  [docs/excision.md](docs/excision.md) and not yet built. Today the control that exists is
  keeping the material out at ingest.

`unsafe_code` is forbidden workspace-wide.
