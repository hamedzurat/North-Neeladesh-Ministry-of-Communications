# Rust persistence for the Run journal

_Researched 2026-08-10 against official SQLite documentation and the published crate documentation/source._

## Decision

Use **one SQLite database per Run, in WAL mode, through SQLx 0.9 and one owned `SqliteConnection`**. This is the simplest good fit for the required shape: one writer, one concurrent read-only observer, atomic Recovery Points, indexed inspection, and a single ordered timeline containing both authoritative replay entries and diagnostic evidence.

SQLite is a better fit than a hand-written JSON-lines log here. A flat log would still need crash-tail detection, atomic multi-record commits, indexing, schema migration, concurrent-reader rules, and backup discipline. SQLite already supplies those properties. WAL specifically permits readers and the one writer to operate concurrently; SQLite still permits only one writer, which matches the proposed ownership model exactly ([SQLite WAL concurrency](https://www.sqlite.org/wal.html#concurrency)).

This is not a recommendation to send arbitrary application logs to a database or to make every subsystem a database writer. The Rust core remains the only authority, and a single adapter serializes all Run-correlated journal entries.

## Rust library choice

| Choice | Fit | Decision implication |
| --- | --- | --- |
| `sqlx` with SQLite | Its SQLite driver puts each connection's blocking SQLite API behind a background worker thread, and its options directly expose WAL, synchronous mode, busy timeout, and read-only access ([`SqliteConnection`](https://docs.rs/sqlx/latest/sqlx/struct.SqliteConnection.html), [`SqliteConnectOptions`](https://docs.rs/sqlx/latest/sqlx/sqlite/struct.SqliteConnectOptions.html)). It also has embedded, versioned migrations ([`migrate!`](https://docs.rs/sqlx/latest/sqlx/macro.migrate.html)). | **Choose this for the already-asynchronous core.** Own one connection in the journal task. Do not use a pool: it provides no benefit for an ordered writer and makes additional writable connections easier to introduce accidentally. |
| `rusqlite` | Direct, synchronous SQLite API. A `Connection` is `Send` but not `Sync`, which naturally fits one owned writer thread; separate threads/processes should open separate connections ([`Connection`](https://docs.rs/rusqlite/latest/rusqlite/struct.Connection.html), [`OpenFlags`](https://docs.rs/rusqlite/latest/rusqlite/struct.OpenFlags.html)). It exposes busy timeouts, explicit transaction behavior, online backup, JSON conversion, and SQLite tracing hooks. | A sound second choice if persistence is deliberately a synchronous actor thread. That design would have to supply the async bridge and use a separate migration crate; SQLx already supplies both pieces for this core. |

Use SQLx's bundled/static SQLite build so the application ships a known SQLite rather than depending on the target laptop's system library ([SQLx SQLite driver notes](https://docs.rs/crate/sqlx-sqlite/latest)). A minimal dependency set at the time of this report is:

```toml
sqlx = { version = "0.9", default-features = false, features = [
    "runtime-tokio", "sqlite", "json", "macros", "migrate"
] }
libsqlite3-sys = "=0.37.0"
```

Pin the exact resolved versions in `Cargo.lock`. SQLx 0.9 permits a range of `libsqlite3-sys` releases and explicitly documents pinning that dependency when a precise native version matters ([SQLx SQLite driver notes](https://docs.rs/crate/sqlx-sqlite/latest)). Version 0.37.0 bundles SQLite 3.51.3 ([libsqlite3-sys 0.37.0](https://docs.rs/crate/libsqlite3-sys/0.37.0)), which contains the WAL-reset corruption fix discussed below. The core should also query and log `sqlite_version()` during startup.

## Minimal architecture

```text
authoritative core + workers
          |
          | typed JournalEntry / append_batch
          v
bounded MPSC channel -> async journal task -> one SQLx SqliteConnection
          ^                  |                 | (SQLx worker thread)
          | commit ack       |                 v
   core applies/continues    +----------> Run database in WAL mode
                                      ^
                                      |
                         separate read-only observer connection
```

The journal task is the only code with a write-capable connection. It accepts typed `append` and `append_batch` requests. An authoritative transition is not a completed Recovery Point until its transaction commits and the task acknowledges it. A batch can atomically contain the normalized authoritative event and the related diagnostic evidence (for example, raw model result, parsed proposal, and validation decision). SQLx keeps SQLite's blocking calls off the async executor through its internal worker thread.

On startup, rebuild the Save by reading only authoritative entries in `seq` order. Diagnostic entries remain visible in that same sequence to `just observe` and debugging tools but are never reducer input. Do not add snapshots in version one. If measured replay time later becomes material, add a rebuildable snapshot keyed by the last authoritative `seq`; it must remain a cache, not a second source of truth.

### Minimal schema

```sql
CREATE TABLE journal_entry (
    seq                 INTEGER PRIMARY KEY,
    run_id              TEXT    NOT NULL,
    recorded_at_utc     TEXT    NOT NULL,
    class               TEXT    NOT NULL
                                CHECK (class IN ('authoritative', 'diagnostic')),
    kind                TEXT    NOT NULL,
    event_version       INTEGER NOT NULL,
    correlation_id      TEXT,
    causation_seq       INTEGER REFERENCES journal_entry(seq),
    payload_json        TEXT    NOT NULL CHECK (json_valid(payload_json)),
    payload_blob        BLOB,
    blob_media_type     TEXT,
    blob_encoding       TEXT,
    redaction_class     TEXT    NOT NULL DEFAULT 'internal'
) STRICT;

CREATE INDEX journal_entry_class_seq
    ON journal_entry(class, seq);

CREATE INDEX journal_entry_correlation_seq
    ON journal_entry(correlation_id, seq);

CREATE TRIGGER journal_entry_no_update
BEFORE UPDATE ON journal_entry
BEGIN
    SELECT RAISE(ABORT, 'journal_entry is append-only');
END;

CREATE TRIGGER journal_entry_no_delete
BEFORE DELETE ON journal_entry
BEGIN
    SELECT RAISE(ABORT, 'journal_entry is append-only');
END;
```

`seq`, not a wall clock, defines total order. The authoritative payload must contain any explicit game clock or seeded-random input required by Replay. `recorded_at_utc` is only diagnostic. `kind` plus `event_version` selects a versioned Rust decoder, so a later code build does not silently reinterpret an old Run.

Use canonical JSON text for structured payloads. SQLx maps `Json<T>` to SQLite `TEXT`; rusqlite also supports JSON text and ordinary BLOBs ([SQLx SQLite type mappings](https://docs.rs/sqlx/latest/sqlx/sqlite/types/), [rusqlite JSON conversion](https://docs.rs/rusqlite/latest/src/rusqlite/types/serde_json.rs.html)). Keep BLOB nullable and use it only for opaque bytes or explicitly compressed large evidence. Normal transcripts, Response Contexts, prompts, settings, raw LLM output, proposals, validation reports, and worker errors should remain JSON text so that the observer and `sqlite3` can inspect them.

One database per Run keeps growth, export, backup, and corruption isolated. The first authoritative entry should freeze the Run Manifest, Scenario/content identity, seed, game build, journal schema, model identities, and gameplay-affecting settings. The database file name is convenience, not identity; every row still carries `run_id` so exported rows remain attributable.

## Exact connection policy

The writer should open a local-filesystem path read/write/create, run migrations before accepting a Run, and explicitly configure and verify:

```sql
PRAGMA journal_mode = WAL;      -- verify returned value is "wal"
PRAGMA synchronous = FULL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 2000;
PRAGMA wal_autocheckpoint = 1000;
```

Use SQLx's custom transaction start with `BEGIN IMMEDIATE` for each journal batch. It either acquires the write transaction up front or returns `SQLITE_BUSY`; after it succeeds SQLite guarantees no later operation through `COMMIT` fails with `SQLITE_BUSY` ([SQLx `Connection`](https://docs.rs/sqlx/latest/sqlx/trait.Connection.html), [SQLite result-code guidance](https://www.sqlite.org/rescode.html#busy)). With exactly one writer this should be uneventful; a persistent busy error is an invariant or storage-health failure and should pause the Run rather than drop an entry.

`synchronous=FULL` is intentional. In WAL mode it syncs the WAL after every commit and is ACID across operating-system crash or power loss. `NORMAL` remains consistent but can lose a recently committed transaction after power loss ([SQLite synchronous matrix](https://sqlite.org/pragma.html#pragma_synchronous)). The entry rate here is low enough to start with the stronger guarantee. Benchmark on the presentation laptop before considering any relaxation.

Keep SQLite's default 1000-page automatic checkpointing initially. The default usually keeps the WAL around 4 MiB, but a long reader transaction can prevent reset and allow it to grow without bound ([WAL size and checkpoint starvation](https://www.sqlite.org/wal.html#avoiding_excessively_large_wal_files)). A passive checkpoint at a Shift boundary is reasonable, but no gameplay correctness should depend on it.

The observer should open a **separate** SQLx connection with `.read_only(true)`, set a short busy timeout, and never set `immutable=true`: an immutable connection is permitted to skip locking/change detection and is wrong for a live file ([SQLx connection options](https://docs.rs/sqlx/latest/sqlx/sqlite/struct.SqliteConnectOptions.html)). The writer must initialize WAL before the observer starts.

`just observe` can poll `PRAGMA data_version` every 100–250 ms and, when it changes, fully consume:

```sql
SELECT seq, recorded_at_utc, class, kind, correlation_id, payload_json
FROM journal_entry
WHERE seq > ?1
ORDER BY seq
LIMIT 256;
```

`data_version` is explicitly intended for interactive displays that need to notice another connection's commits ([SQLite `data_version`](https://sqlite.org/pragma.html#pragma_data_version)). Do not keep a read transaction open while sleeping: long readers prevent checkpoints from completing. The default observer view should summarize metadata and payload size; a flag can show the complete JSON/BLOB metadata on demand.

WAL requires all processes to be on the same machine and does not work over network filesystems. A live WAL database also has `-wal` and `-shm` sidecars; the WAL is part of persistent state and must remain paired with the database ([WAL limitations and file handling](https://www.sqlite.org/wal.html)). Store Runs on the laptop's local disk, not a network share, USB-sync folder, or cloud-synchronized directory.

## What belongs in the timeline

Authoritative entries are the minimal deterministic inputs needed to reconstruct game state:

- the Run Manifest and compatibility identities;
- accepted Operator commands and Cabinet-derived commands;
- explicit game-clock and seeded-random inputs;
- normalized accepted external completions, including validated AI results;
- authoritative pause/resume, Story Event, Action Record, and ending transitions.

Diagnostic-only entries include frozen Response Contexts, exact model request/settings, raw LLM output, parsing and proposal details, rejected validation outcomes, transcripts, worker stdout/stderr summaries and failures, retry/cancellation evidence, Cabinet Link health, printer evidence, and persistence/checkpoint faults. The accepted normalized value may therefore appear authoritatively while its raw provider response appears diagnostically, tied by `correlation_id`.

Record semantically complete AI boundaries, not every streamed token callback: request sent, final raw response, parse/proposal, validation, and accepted/rejected result. This preserves what is useful for diagnosis without multiplying transactions or making scheduling noise part of Replay. Raw microphone audio is not needed for resume or deterministic Replay; only add it as a BLOB if a separate debugging/privacy decision requires it.

## Structured tracing integration

Do **not** make `tracing` or `tracing-subscriber` the authoritative write path. `tracing-subscriber` layers are composable presentation/collection policies and may be filtered, while `tracing-appender`'s default non-blocking writer is lossy when its bounded queue fills ([`tracing-subscriber::Layer`](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/layer/trait.Layer.html), [`tracing-appender::non_blocking`](https://docs.rs/tracing-appender/latest/tracing_appender/non_blocking/)). Those are acceptable properties for operational logs, not for a Recovery Point.

Use a typed `JournalSink`/`JournalWriter` adapter directly for every authoritative entry and every diagnostic artifact promised to be retained. Emit ordinary `tracing` events in parallel for console/file observability. A custom `Layer` may later forward selected best-effort operational events into the journal, but it must enqueue through the same adapter, never perform SQLite I/O inside the callback, and never be required for Replay or complete LLM evidence. SQLx's query logging can provide SQL timing, but it does not replace the semantic LLM payload entry.

## Backup, corruption, and recovery

Normal process/OS crash recovery is SQLite's job: an interrupted transaction is rolled back automatically when the database is next opened. The application should open the writable connection first so it can perform any required WAL recovery, then run `PRAGMA quick_check` and `PRAGMA foreign_key_check` before offering Resume. Use full `integrity_check` in preflight or backup verification; `quick_check` is faster but omits some checks, and neither integrity check includes foreign-key validation ([SQLite integrity-check guidance](https://sqlite.org/pragma.html#pragma_integrity_check)).

Create consistent backups at a safe boundary such as a completed Shift or clean Run close with `VACUUM INTO`, which SQLite documents as a consistent live snapshot and alternative to the online backup API ([SQLite `VACUUM INTO`](https://www.sqlite.org/lang_vacuum.html#vacuuminto), [SQLite Online Backup API](https://www.sqlite.org/backup.html)). Do not `cp` a live database file: copying it without its live journal can lose committed transactions or produce a corrupt copy. Likewise, do not move, rename, or delete `-wal`/`-shm` files while a connection is open ([SQLite corruption hazards](https://www.sqlite.org/howtocorrupt.html)).

If checks report corruption, pause and preserve the damaged file plus sidecars for diagnosis, then restore the newest verified backup. SQLite's `.recover`/recovery API is salvage, not an exact recovery guarantee; perfect restoration is the exception for some corruption patterns ([SQLite recovery limitations](https://www.sqlite.org/recovery.html)). The latest committed transaction guarantee therefore depends on the primary database and storage behaving correctly; backups bound loss after actual media/filesystem corruption.

## Risks and guardrails

- **Unbounded evidence:** complete LLM payloads and transcripts can grow without limit. Track database and per-entry byte counts, preflight free disk space, warn/pause before the volume becomes unsafe, and archive completed Run databases. Never silently truncate an authoritative entry. Do not prune individual rows from the append-only journal; archive or delete a whole Run only under an explicit retention policy.
- **WAL starvation:** `just observe` must finish each query before sleeping. Keep automatic checkpoints enabled and expose WAL/database sizes in diagnostics.
- **SQLite WAL-reset bug:** require SQLite 3.51.3 or a fixed backport such as 3.50.7/3.44.6. SQLite says older WAL versions can rarely corrupt under multi-connection checkpoint/write timing; pin and verify the bundled version ([SQLite WAL-reset bug](https://www.sqlite.org/wal.html#walreset)).
- **Sidecar mishandling:** use the SQLite backup API for live copies. Treat the database plus a live `-wal` as one persistence unit.
- **Secrets and privacy:** redact API keys, authorization headers, environment variables, and unrelated host paths **before** an entry reaches the journal queue. Prompts, dialogue, and transcripts are intentionally retained and should be marked with a redaction class for controlled export. A redaction label is not encryption; use restrictive filesystem permissions and add SQLCipher only if a real threat model justifies its extra deployment complexity.
- **Version drift:** record game/content/model identities in the first authoritative entry and version every event. A newer binary must either understand those versions or refuse Resume clearly; SQLite schema migration alone cannot make old domain events semantically compatible.
- **Disk-full/I/O failure:** journal append failure is not a recoverable warning. Do not apply or acknowledge the authoritative transition; pause the Run and surface the persistence fault.

## Bottom line

SQLite is the best simple choice for this specific one-writer/one-observer Rust design. Select SQLx with one owned `SqliteConnection`, serialize all relevant entries through one journal task, use WAL + `synchronous=FULL`, and keep one `seq` timeline with an explicit authoritative/diagnostic class. This gives live observation, forensic debugging, deterministic reconstruction, and Resume without maintaining separate save and logging systems.
