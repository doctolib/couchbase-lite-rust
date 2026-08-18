# Examples

## `index_benchmark` — offline index-migration benchmark (no Sync Gateway / Server)

`index_benchmark.rs` measures how a sequence of index changes affects query performance and
database size on a realistic, production-like data set, entirely locally.

What it does:
1. On first run, generates a persistent ~100 MB data set of billeo-style documents (1
   `UserSettingsModel`, many `FactureModel`, fewer `FactureLibreModel`, a lot of
   `EhrEncounterModel`, plus the supporting types). Subsequent runs reuse it.
2. Drops every index, recreates the current production index set, and compacts.
3. Runs every query (each shape/variation) `BENCH_RUNS` times and records the median/mean/min/max
   plus the index each query actually uses (from `EXPLAIN`, all `USING INDEX` occurrences). The two
   heavily-dynamic queries — `Facture.liste_factures` (`requete_liste_factures`) and
   `BillForUi.find_by` — are reproduced by faithful clause builders (including the createdAt/patient
   de-index pins and the injected all-statuses safety net) and expanded into the realistic frontend
   combinations the user can produce (period, statut, patient, care plan, num, mode, payment-state,
   the 11 `Billfilter` variants, etc.), so a single change of active filters is a distinct shape.
   That is ~100 query shapes in total, not one per named query. Set `BENCH_DUMP_SQL=1` to print every
   generated query and exit without running.
4. Walks the migration steps (the trains from the CBLite indexing plan). For each step it applies
   the index change, compacts, records on-disk size, **closes and reopens the database** — the
   close runs CBLite's automatic `Optimize` (partial `ANALYZE`), which is what actually re-elects
   query plans, so this reproduces production plan-flip behaviour rather than forcing deterministic
   stats — then re-runs every query and writes a per-step report + CSV.
5. Writes `report_OVERALL.md` answering: (a) did every query end up faster than baseline and by how
   much, (b) did the database shrink, (c) did any query get slower from one step to the next.

Run it (release strongly recommended; the first run generates the data set):

```shell
cargo run --release --example index_benchmark
```

Fast end-to-end smoke run:

```shell
BENCH_TARGET_MB=4 BENCH_RUNS=3 cargo run --example index_benchmark
```

Environment variables:
- `BENCH_DIR` (default `./bench_data`) — where the database and the `report_*.md` / `timings_*.csv`
  outputs are written.
- `BENCH_TARGET_MB` (default `100`) — target size of the generated **document** data (the baseline
  database is larger once all production indexes are built on top).
- `BENCH_RUNS` (default `10`) — measured executions per query. A per-query wall-clock budget stops
  pathological queries (e.g. an `ARRAY_CONTAINS` join no index can fix) early; the report notes how
  many runs completed.

To regenerate the data set from scratch, delete `bench_data/` (or your `BENCH_DIR`).

---

# Running examples with Couchbase Sync Gateway & Server

Couchbase Lite is often used with replication to a central server, so it can be useful to test the full stack.
The examples in this directory aim at covering these use cases.

## Setup the Couchbase Sync Gateway & Server

This process is handled through docker images, with as an entry point the file `docker-conf/docker-compose.yml`.

The configuration files that might interest you are:
- `docker-conf/couchbase-server-dev/configure-server.sh` -> sets up the cluster, bucket and SG user
- `docker-conf/db-config.json` -> contains the database configuration
- `docker-conf/sync-function.js` -> contains the sync function used by the Sync Gateway

To start both the Sync Gateway and Couchbase Server, move to `docker-conf` through a terminal and use:

```shell
$ docker-compose up
```

It's very long the first time...

You can then access the Couchbase Server web ui through [http://localhost:8091](http://localhost:8091) (Chrome might not work, Firefox has better support).
Make sure to not have another instance running.

## Update the config after startup

You can change a few things through the `curl` command.

#### Sync function

Update the file `docker-conf/sync-function.js` and run
```shell
$ curl -XPUT -v "http://localhost:4985/my-db/_config/sync" -H 'Content-Type: application/javascript' --data-binary @docker-conf/sync-function.js
```

#### Database config

Update the file `docker-conf/db-config.json` and run

```shell
$ curl -XPUT -v "http://localhost:4985/my-db/" -H 'Content-Type: application/json' --data-binary @docker-conf/db-config.json
```

## Running an example

As of now, there is only one example: `ticket_70596`.

It can be run with the following command:
```shell
$ cargo run --features=enterprise --example ticket_70596
```

There are utility functions available to interact with the Sync Gateway or Couchbase Server, feel free to add more if needed.
