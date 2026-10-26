# Usage - which tools the agents call, on which workspaces

> A local record of how this node is used: every MCP call the daemon answers, and every time the
> Claude Code hook gives a session its index or asks about a commit. Companion to
> [recall.md](recall.md) (what the hook gives a session; Section 1 measured it from transcripts) and
> [onboarding.md](onboarding.md) Section 5 (the first-contact log, which this extends in time).
>
> Status: **built**. Decisions D1-D6 were chosen by the author on 2026-10-26.

## 1. Why this exists

recall.md Section 1 measured how agents use this node by reading Claude Code's transcripts. That
answered the question for one app and only after the fact, and the daemon itself could not answer
it: a write is in the log with the app that made it, but a read is recorded nowhere. The viewer's
activity feed shows reads as they happen and forgets them, and the first-contact log keeps one
first and last time per app, in memory. The author asked to follow, over days, how many requests each
MCP function receives on each workspace, from every app, and to study usability from that.

## 2. What it is (D1, D2, D4)

One file, `usage.jsonl`, in the directory that holds the viewer's socket (`~/.supragnosis` by
default; `SUPRAGNOSIS_VIZ_SOCK` moves both). Each line is one JSON object:

| Field | What it holds |
|---|---|
| `t` | When, in milliseconds since the epoch |
| `kind` | `tool`, `resource`, `prompt` or `hook` |
| `name` | The MCP tool (`search_knowledge`), the resource's kind (`observation`, `entity`, `graph`, ...), the prompt (`brief`), or the hook's event (`session_start`, `subagent_start`, `commit_question`) |
| `workspace` | The workspace the call named, as given (`*` included); absent when it named none |
| `app` | The app's recorded name (`Claude Code`), absent for an app that gave none |
| `outcome` | `ok`, `error`, `not_found` (an id that names nothing) or `empty` (a read that found nothing) |
| `n` | How many it answered: hits, entities reached, rows, entities written; on a hook event, the index lines shown |
| `of` | On a hook index, how many records the workspace holds |
| `mode` | A search's mode, `hybrid` or `keyword` |
| `commit` | On `observe` with a `git:` source and on a commit question, the commit id |
| `ms` | How long the daemon took to answer |

- **The daemon writes** a line for every tool call, resource read and prompt it answers on its MCP
  surface (the one the bridge and every app reach), after the answer is built. **The hook writes** a
  line when it gives a session or a subagent its index and when it asks about a commit. Each line is
  one append to the file, so the two processes never interleave within a line.
- **The commit id joins the two.** A commit question and an `observe` whose source is that commit
  share it, so "was the question answered with a record" is counted exactly, where recall.md
  Section 1 could only approximate it by tool calls nearby.

## 3. What this is NOT (D3, D6)

- **Not knowledge.** The file is not in the store, is never synced and never leaves the machine,
  for the reason the first-contact log never does (onboarding.md O7). It is created 0600.
- **Not what was asked.** No query text, observation content, entity name or resource body is
  written: names of functions and workspaces, counts, outcomes and commit ids only. Recall quality
  is measured by recall.md Section 7's eval and by the transcripts, which hold the queries already.
- **Not a new MCP tool, and not telemetry.** No agent reads it through the MCP surface (Principle
  21) and nothing sends it anywhere.
- **Not trimmed.** A line is about 150 bytes; a heavy day of a few hundred calls adds tens of
  kilobytes. Nothing removes old lines now; Section 6 says when that is revisited.
- **Never in the way.** A line that cannot be written is said once in the daemon's log and the call
  is answered as if nothing happened (Principle 24, degrade loudly without refusing).

## 4. Reading it (D5)

`supragnosis usage [--since 7d] [--json]` reads the file and answers:

- calls by MCP function and workspace, with how many failed, found nothing or named nothing;
- calls by app;
- the hook: indexes given to sessions and subagents, commit questions, and how many of those were
  followed by an `observe` of the same commit.

`--since` takes a number with `d` or `h`, and defaults to 7 days; `--json` gives the same tables as
data. A page in the app or the viewer is owed until the command shows what such a page would need.

## 5. Invariants

| | Invariant |
|---|---|
| **UL1** | The usage file stays on the machine: it is not in the store, not synced, not sent, and it is created 0600. |
| **UL2** | No query text, observation content, entity name or resource body is written to it. |
| **UL3** | A line that cannot be written never fails or delays the call it records. |
| **UL4** | Every tool call, resource read and prompt the daemon's MCP surface answers is recorded with the workspace it named. |

## 6. Revisit when

- **Trimming:** when the file passes about 50 MB, or a month of lines slows `supragnosis usage`.
- **A page:** when the author wants the numbers where they already look (the app's This Mac pane,
  or the viewer's status bar), rather than in a terminal.
- **Query text:** only if a usability question needs what was asked and the transcripts cannot
  answer it (another app's queries, for instance), and then with its own decision about where it
  is kept.
