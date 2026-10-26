# Usage - which tools the agents call, on which workspaces

> A local record of how this node is used: every MCP call the daemon answers, and every time the
> Claude Code hook gives a session its index or asks about a commit. Companion to
> [recall.md](recall.md) (what the hook gives a session; Section 1 measured it from transcripts) and
> [onboarding.md](onboarding.md) Section 5 (the first-contact log, which this extends in time).
>
> Status: **built**. Decisions D1-D6 were chosen by the author on 2026-10-26, and D7 (the Usage pane,
> Section 4.1) on 2026-10-27.

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
| `name` | The MCP tool (`search_knowledge`), the resource's kind (`observation`, `entity`, `graph`, ...), the prompt (`brief`), or the hook's event (`session_start`, `subagent_start`, `commit_question`); `unknown` for a tool or prompt the surface does not have, since that name is the caller's text |
| `workspace` | The workspace the call named, as given (`*` included); absent when it named none |
| `app` | The app's recorded name (`Claude Code`), absent for an app that gave none |
| `outcome` | `ok`, `error`, `not_found` (an id `get_entity`, `get_proposal` or a resource read cannot find) or `empty` (a read that found nothing; `traverse` from an unknown id reads this way) |
| `n` | How many it answered: hits, entities reached, rows, clusters, entities written; on a hook event, the index lines shown |
| `of` | On a hook index, how many records the workspace holds |
| `mode` | A search's mode, `hybrid` or `keyword` |
| `commit` | On `observe` whose source is `git:` and 7 to 64 hex digits, and on a commit question, the commit id; any other source text is not kept |
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
- the hook: indexes given to sessions and subagents, commit questions counted by commit (a second
  commit command within the minute asks about the same commit again), and how many of those
  commits an `observe` names.

`--since` takes a number with `d` or `h`, and defaults to 7 days. `--json` gives the same tables as
data, with `"schema": 1`, since the app reads it (compatibility.md Section 6), and an empty summary
when nothing is recorded yet.

### 4.1 Trends, and where a person sees them (D7)

Settings > Usage in the desktop app (settings-page.md Section 3.6) shows the tables and how they
moved day by day. It reads `supragnosis usage --json --since <7d|30d|90d>`, so the command stays
the one reader of the file and the page counts nothing itself.

- **`by_hour`.** The JSON carries one entry per UTC hour that holds a line: the hour's start `t`;
  its calls by function, by workspace and by app, as `[name, count]` pairs; how many were writes,
  found nothing (`empty`), named nothing (`not_found`) or failed (`errors`); and the hook's session
  and subagent indexes and commit questions. A question counts in the hour it was first asked, and
  as answered when an `observe` in the window names its commit.
- **Writes** are the calls that change what this node holds: `observe`, `define_type`, `propose`,
  `review` and `sync_pull`. Every other call reads. Reads against writes is the question recall.md
  Section 1 asked of transcripts, now asked of every app.
- **Local days.** The page adds the hours into the person's local days. A time zone that is not a
  whole number of hours from UTC puts half an hour at each day's edge into the neighbouring day.
- **What the pane shows**, for the window chosen: four figures (calls with reads and writes, the
  share that found nothing or failed, the indexes the hook gave, the commit questions answered);
  calls per day split by function, workspace, app or read and write, the five largest and the rest
  as Other; per day, what found nothing or failed, the indexes given and the commit questions
  answered; and the two tables of Section 4. Every chart has its numbers in a table beside it.

## 5. Invariants

| | Invariant |
|---|---|
| **UL1** | The usage file stays on the machine: it is not in the store, not synced, not sent, and it is created 0600. |
| **UL2** | No query text, observation content, entity name or resource body is written to it. |
| **UL3** | A line that cannot be written never fails or delays the call it records. |
| **UL4** | Every tool call, resource read and prompt the daemon's MCP surface answers is recorded with the workspace it named. |

## 6. Revisit when

- **Trimming:** when the file passes about 50 MB, or a month of lines slows `supragnosis usage`.
- **Longer windows:** when 90 days is not enough to see a change, or reading 90 days of hours makes
  the pane slow to open.
- **Query text:** only if a usability question needs what was asked and the transcripts cannot
  answer it (another app's queries, for instance), and then with its own decision about where it
  is kept.
