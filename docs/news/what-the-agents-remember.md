---
title: What the agents remember, four days in
summary: Agents wrote to supragnosis far more often than they read from it. In 0.11.0 a session starts with its project's newest records. Four days of measurement on one machine suggest that reading rose; they do not yet show that the work improved.
date: 2026-10-28
version: 0.11.0
update: brew upgrade; supragnosis connect claude-code; supragnosis restart
---

supragnosis keeps what AI agents learn while they work, so that the next agent on the same project
can find it. Whether the next agent does find it is an empirical question, and for most of this
project's life it went unasked. This note reports what we measured, what we changed in 0.11.0 in
response, and what four days of use can and cannot say about that change. The numbers come from one
person's machine and one person's projects. They are offered as an account of method more than as a
result.

## What we measured first

On 2026-10-24 we read every Claude Code transcript kept on the author's machine for the two weeks
before: about 55 sessions and 900 subagents, some 92,000 tool calls. Agents had made 252 records
and read the store 44 times. We could find about two reads whose answer changed what the agent did
next.

The records themselves were not the problem. Most said why a decision was taken, what was ruled
out and what was measured. They were simply not read. What did reach agents was what arrived
without being asked for: Claude Code's own memory files, loaded at the start of every session, were
read about as often as every supragnosis tool was called. The difference did not appear
to be the agents' interest in memory. It was which memory was placed in front of them.

## What 0.11.0 changes

- **A session starts with an index.** At its start, and again after its context is compacted, the
  agent receives its project's 25 newest records: the date, the first sentence and a link that
  opens each one. They are marked as evidence written by agents and people, not as instructions.
  A subagent receives the newest ten.
- **A narrower question after a commit.** The agent is asked to record what the diff does not
  show: a choice the person made, an option ruled out, a number measured, a constraint.
- **Use recorded as provenance.** When a record shapes a new one, the agent is asked to name it in
  `derived_from`, a field `observe` has always had. Citation is the closest thing to "this was
  useful" that we could find which is also knowledge in its own right.
- **A usage log that stays on the machine.** The daemon counts which tools are called, on which
  workspaces, and with what outcome, without keeping any query or record text. `supragnosis usage`
  and the app's Settings > Usage read it.

## What four days show

We compared the seven days before the index with the four days after, counting each tool call once
across the transcripts. The windows and the counting are set out in
[docs/recall.md](../recall.md#72-the-first-days-in-use) Section 7.2.

| | Before (7 days) | After (4 days) |
|---|---|---|
| Reads of the store per day | 4.9 | 12.5 |
| Reads per 100 tool calls | 0.07 | 0.32 |
| Writes per day | 31 | 87 |
| Reads per write | 0.16 | 0.14 |

Reading rose by day and by tool call; it did not rise by write. Writing rose faster, mostly because
more questions after a commit were answered, so the store is read no more often relative to how much
is put into it. There were also fewer tool calls in the second period, which by itself raises the
rate per call. Of
the 16 records opened after the change, 14 were lines of the session's own index. None of the 23
reads recorded since the usage log began came back empty.

One finding was not anticipated. In the busiest project the 25 newest records reach back only
about six and a half hours, so a decision made a few days earlier is no longer in the index, and
the agent finds it only by searching. In quieter projects the same 25 lines cover three to six
days. An index chosen by recency alone serves a project less well the more that project records.

## What they do not show

Four days on one machine is a small sample. The mix of projects differed between the two periods,
overall activity was lower in the second, and about half of the reads came from two long sessions
in a single project. We have not measured whether a read changed a decision, still less whether it
improved one. A rise in reading is a precondition for recall being useful; it is not evidence that
it is.

## What we intend to measure next

These are plans, not results, and each will be reported whether or not it supports the change it
tests.

- **A labelled baseline.** A sample of requests marked by hand: whether past knowledge was needed,
  whether the store held it, whether the agent saw or searched for it, and whether it used it.
- **The index's selection.** Sessions split between the present index and one that also keeps the
  decisions still in force, to see whether recency is the right order.
- **Citations.** How often `derived_from` names the records an agent read, against a baseline of 17
  in 346 records before the guidance.
- **Requests in Korean.** Whether search can be run on the person's own words at each prompt. It
  remains deferred until an offline test shows it finds the right record about nine times in ten,
  and stays silent when nothing relevant is recorded.

## Looking at your own

`supragnosis usage --since 7d` prints your machine's side of these counts - which tools were
called, on which workspaces, how many records were opened from an index and how many writes cite
one - and Settings > Usage draws them by day. Nothing in that log leaves the machine, and no tool
serves it to an agent.

The design behind this note is in [docs/recall.md](../recall.md), and the usage log is described
in [docs/usage.md](../usage.md).
