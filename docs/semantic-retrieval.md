# Semantic retrieval in the prebuilt binary

> How the release binary comes to carry a local embedding model, which model, what it costs, and
> what happens when the model is not there. Companion to [architecture.md](architecture.md)
> Section 6 (hybrid retrieval) and [daemon-lifecycle.md](daemon-lifecycle.md) (who starts the
> daemon, and so who fetches the model).
>
> Status: **built** (Section 9 steps 1-4), with the measurements that decided it (Section 2).
> What building it changed is in Section 11.

## 1. Why this exists

Every release so far has been keyword only. `search_knowledge` matches every term of the query in
any order, with no morphology: a Korean particle glued to a noun is a different term, and a
question that says "worker failed to start" finds nothing about a worker that "crashed at boot".
Semantic retrieval exists, behind the `fastembed` feature, and the README said to build from source
for it. Nobody who installs with `brew` or the curl line builds from source, so the retrieval the
documents describe is the retrieval nobody has.

The gap is not theoretical. The author's own store is Korean and English mixed, and the memory of
2026-10-08 records a query (`pg-boss 부팅 크래시`) that found nothing until keyword matching was
loosened. A memory that cannot find what it holds when the question is phrased differently is
the first impression this project gives, and it is the wrong one.

## 2. What was measured

On an Apple silicon Mac, 2026-10-16, with the release profile and the real models. The A/B is
twelve observations and six queries per language, each query phrased to share no keyword with its
target; rank is the target's position among five hits in `hybrid` mode.

| | keyword only (every release so far) | bge-small-en-v1.5 (the feature's default until now) | paraphrase-multilingual-MiniLM-L12-v2, quantized |
|---|---|---|---|
| binary | 21 MB | 52 MB, every library linked statically | 52 MB |
| model, downloaded once | - | 129 MB, 12.9 s | 224 MB (257 MB on disk), 23.4 s |
| one session once cached: observe x2, search x2 | 0.03 s | 0.17 s | 0.58 s |
| peak resident memory | 16 MB | 287 MB | 982 MB |
| Korean paraphrase A/B, top-1 / top-3 of 6 | 0 / 0 | 0 / 0 | 3 / 5 |
| English paraphrase A/B, top-1 / top-3 of 6 | 0 / 0 | 2 / 2 | 4 / 6 |

Three things follow. Keyword retrieval answers none of the rephrased questions in either language.
The English model is no use on Korean, and on short English questions it is beaten by the
multilingual one. The multilingual model is the only one of the three that works on the store this
project actually has, and its price is memory.

Other facts the design rests on:

- `ort` ships prebuilt static ONNX Runtime libraries for all three release targets
  (`aarch64-apple-darwin`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`). Its build
  script fetches each from `cdn.pyke.io` and verifies it against a hash pinned in the crate, so the
  build gains one network dependency, and not a trust one.
- fastembed's default features pull native TLS for the model download. On Linux that links OpenSSL
  into a binary that today links none (reqwest and the sync listener use rustls).
- The store already records the embedder's identity and refuses to reopen under another
  (`redb_refuses_a_reopen_under_a_different_embedder`): vectors from two models share no space.
- `reproject` re-embeds every live entity, so a store written without vectors can be given them
  once, with the daemon stopped (`merge_band_reports_whether_it_could_run_and_over_how_much`,
  part 3).
- No release has ever recorded an embedder in a store, so changing the feature's default model
  breaks nothing shipped.

## 3. What this is NOT

- **Not a change to retrieval.** The hybrid fold, the `mode` label on every answer, the keyword
  path and the degrade from one to the other are as they are. This document puts a model where
  the fold expected one.
- **Not a model in the artifact.** The tarball, the bottle and the cask stay the binary. The model
  is fetched once into `~/.supragnosis/models`, and the container image bakes it in at build
  time (Section 6), which is the one place a first-use fetch has nowhere to go.
- **Not a remote embedder.** Nothing leaves the machine (Principle 17). The one network act is
  the download of the model files from Hugging Face, by the person's command, never by the daemon
  (SR1).
- **Not automatic backfill.** Knowledge written before this release has no vectors; `reproject`
  gives it them, once, and the release note says so. A daemon that embeds its backlog on start is
  a later decision, recorded in Section 10.

## 4. The build

- The release binaries are built with `--features fastembed` for all three targets. The formula
  and the casks copy the binary and change nothing. The bottle job builds from the same tarball.
- `supragnosis-embed` takes fastembed with `default-features = false` and
  `ort-download-binaries-rustls-tls` and `hf-hub-rustls-tls`: the download of the runtime at build
  time and of the model at run time both go over rustls, as every other connection the binary
  makes, and the image-model code is left out. A release step lists the binary's dynamic
  dependencies and fails on anything outside the system (SR5).
- The container image builds with the feature and runs `supragnosis embed fetch` in the build
  stage into `/usr/share/supragnosis/models`, which `SUPRAGNOSIS_MODEL_DIR` names in the image.
  The state volume holds the knowledge and the node key as before; the model is part of the image,
  so a container never downloads.
- CI keeps the keyword path tested: `cargo test` without the feature stays as it is. The
  `fastembed feature builds` job becomes what the release does, and runs clippy under the feature.

## 5. The model

- **Default: `paraphrase-multilingual-MiniLM-L12-v2`, quantized, 384 dimensions**, from the A/B.
  The embedder's identity is `paraphrase-multilingual-minilm-l12-v2-q-384`.
- **`SUPRAGNOSIS_EMBED_MODEL`** picks between `multilingual` (the default) and `bge-small-en`
  (`bge-small-en-v1.5-384`, 129 MB, 287 MB resident) for a store that will only ever hold
  English. Any other value is refused at start with the two names (Principle 24: an unknown
  configuration value is a refusal, as an unknown key is).
- A model's dimensions and identity are known without loading it, so the engine can be built
  before the model is, and the store's guard runs at open as it does today.
- **`SUPRAGNOSIS_EMBED=none`** is the opt-out: no model is loaded, memory stays where it is today,
  retrieval is keyword only and `status` says so. `hashing` stays what it was, a development
  stand-in.
- The cache directory is `~/.supragnosis/models` (`SUPRAGNOSIS_MODEL_DIR` overrides it), inside
  the state directory, so `rm -rf ~/.supragnosis` still removes everything.

## 6. Loading, fetching, degrading

- **The model loads on first use, from the cache only.** The daemon starts as fast as it does
  today; the first `observe` or `search_knowledge` pays the load once, 0.1 to 0.5 s from the
  measurements, inside that call. Every later call finds it loaded.
- **The daemon never downloads.** When the cache has no model, the first embed fails, the process
  degrades to keyword retrieval for its lifetime, the answer's `mode` says `keyword` as it does
  today, one warning names the command to run, and `status` says the same (SR1, SR3). A write
  never waits on the network and never fails for want of a model (Principle 19).
- **`supragnosis embed fetch`** downloads the chosen model into the cache, with progress on the
  terminal, and says what it got and how large. It is idempotent. `supragnosis service install`
  runs it after installing the job, and reports a failure without failing the install: the daemon
  is useful without the model. The app's Start at Login does the same through the CLI it already
  calls, and its status line carries the result.
- After a fetch, a running daemon that had degraded loads the model on its next embed: the
  failure it remembers is "no files", and the files are now there. So the sequence for a person
  who started by hand is `supragnosis embed fetch`, and nothing else.
- **`status`** gains a `semantic` object: `{"state": "on" | "loading" | "off", "model": <identity>,
  "note": <why off, and what to run>}`. The text form is one line, `semantic: on
  (paraphrase-multilingual-minilm-l12-v2-q-384)` or `semantic: off - model not fetched; run
  supragnosis embed fetch`. An added field, so the JSON schema number stays.

## 7. Existing stores

A store written by an earlier release has no embedder recorded and no vectors. Opening it under
this release records the embedder and embeds what is written from then on; what was written
before is found by keyword only until `supragnosis reproject` runs once (daemon stopped). The
release note opens with that, and with the two numbers a person should know before upgrading: the
first fetch is 224 MB, and the daemon holds up to about 1 GB once the model is loaded.

## 8. Invariants

| | Invariant |
|---|---|
| **SR1** | The daemon never downloads a model. A missing model degrades the process to keyword retrieval, labelled on every answer, and never delays or fails a write. |
| **SR2** | The model is loaded at most once per process, on first use, from the cache; a load that found no files is not retried until files exist. |
| **SR3** | `status` says whether semantic retrieval is on, and when it is off, why and what to run. |
| **SR4** | Vectors from two models never share a store: the embedder's identity is model-specific and the store refuses a reopen under another. |
| **SR5** | The release binary links no library the operating system does not ship: no OpenSSL, no ONNX Runtime shared object. |

## 9. Ordering

1. **The embed crate.** Model selection, model-specific identity, the lazy loader with its
   one-shot failure memory, the fetch, and fastembed's features.
2. **The CLI.** `SUPRAGNOSIS_EMBED_MODEL`, `embed fetch`, the `semantic` line and field in
   `status`, `service install` fetching, the app's Start at Login following.
3. **The build.** `release.yml` with the feature and the dynamic-dependency check, the Dockerfile
   with the baked model, `rust.yml`'s feature job.
4. **Documents.** README, the landing page, `deploy/README.md`, `architecture.md`'s retrieval
   section, and the release note of the release that ships it.

## 10. Closure map

| Principle | Where this closes it |
|---|---|
| P19 - the probabilistic edge degrades and never blocks a write | SR1, SR2, Section 6 |
| P24 - degrade loudly; an unknown value is a refusal | SR3; `SUPRAGNOSIS_EMBED_MODEL` in Section 5 |
| P17 - nothing leaves the machine | Section 3: the fetch is the person's act, the daemon's connections are unchanged |
| P19 - an adapter swap must not harm core correctness | SR4 |
| compatibility.md - a store is never misread | Section 7: an unrecorded embedder is recorded, never assumed |

Guarded by:

- `a_missing_model_degrades_to_keyword_and_says_why` - the loader is called once, the embed
  answers the degrade note, a second embed does not call the loader again, `readiness` carries
  the note, and after the files appear the next embed loads (SR1, SR2).
- `an_empty_cache_is_missing_and_never_fetched` - the real adapter over an empty directory: the
  note names the fetch and its size, nothing is created on disk, and only a file over a megabyte
  counts as the weights (SR1).
- `the_model_loads_once_on_first_use` - not at construction, once after two embeds; a load that
  failed with the files present is not retried (SR2).
- `the_model_choice_is_parsed_and_identified` - the two names map to their identities and
  dimensions; any other value is refused naming both (Section 5).
- `status_names_the_semantic_state` - on with the model, off with the daemon's reason, off for a
  node with no embedder; `status_json_matches_its_example` carries the field (SR3).
- SR4 is `redb_refuses_a_reopen_under_a_different_embedder`.
- SR5 is a release step, not a cargo test: `Structural` in the registry, by the features Section
  4 names and the step that lists the binary's dependencies on every release and in CI's feature
  job.

Deferred, recorded here so it is not forgotten: a daemon that embeds its own backlog after a model
arrives (Section 3's last point). The need is real for the first upgrade; `reproject` meets it at
the cost of a stop.

## 11. What building it changed

- **fastembed has no fetch-only call.** `TextEmbedding::try_new` is what downloads, and it loads
  the model it fetched. So `embed fetch` loads the model once in the fetching process, and
  `service install` pays about a gigabyte of memory for the seconds the fetch takes. The
  provider's own path is unchanged: it never calls `try_new` while the files are absent.
- **hf-hub keeps each file once and links to it.** The cache holds `blobs` and a `snapshots`
  directory of symlinks into them; a size that followed the links reported 480 MB for 240. The
  size and the presence check skip symlinks.
- **The health report carries `semantic`, and `status --json` keeps it beside `store`, not in
  it.** The daemon's `/api/health` is what the CLI already read for the store, so the model's
  state rides on it; the document gives it its own field and strips it from `store`, which the app
  reads by shape.
- **The degrade is said at start as well as on use.** `readiness` on an unloaded provider asks
  the loader once, without keeping the answer, so the daemon's log names the missing model when
  it starts and `status` names it from then on; the first embed decides for real.
- **Checked end to end on the author's machine** with the release build: `embed fetch` into an
  empty directory took 25 s and reported 240 MB, a second run found it; `SUPRAGNOSIS_EMBED_MODEL=gpt`
  was refused naming both models; a stdio server over an empty cache logged the note at start and
  answered a search in `keyword` mode; the same server over the fetched cache answered a Korean
  paraphrase in `hybrid` mode with the right observation first; `status` against the running
  v0.4.12 daemon, which has no model, read `semantic off - keyword only`.
