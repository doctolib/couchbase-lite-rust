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

## Automated Test Infrastructure

The long-running tests (`tombstone_purge_test` and `tombstone_purge_test_short`) now include:

- **Automatic Docker environment management**: Stops, rebuilds, and starts containers with correct configuration
- **Git validation**: Ensures no uncommitted changes before running
- **Structured reporting**: Generates comprehensive test reports in `test_results/` directory

### Test Reports

Each test run generates a timestamped report directory containing:
- `README.md`: Executive summary with test checkpoints and findings
- `metadata.json`: Test metadata, commit SHA, GitHub link
- `tombstone_states.json`: Full `_sync` xattr content at each checkpoint
- `test_output.log`: Complete console output
- `cbs_logs.log`: Couchbase Server container logs
- `sgw_logs.log`: Sync Gateway container logs

**Example report path**: `test_results/test_run_2025-11-01_08-00-00_8db78d6/`

## Running an example

### Available examples

#### `check_cbs_config`
Utility to verify Couchbase Server bucket configuration, especially metadata purge interval.

**Runtime: Instant**

```shell
$ cargo run --features=enterprise --example check_cbs_config
```

Expected output:
```
✓ CBS metadata purge interval (at purgeInterval): 0.04
  = 0.04 days (~1.0 hours, ~58 minutes)
```

#### `tombstone_quick_check`
Rapid validation test for tombstone detection via XATTRs. Verifies that tombstones are correctly identified in CBS without waiting for purge intervals.

**Runtime: ~30 seconds**
**Output**: Clean, no warnings

```shell
$ cargo run --features=enterprise --example tombstone_quick_check
```

#### `ticket_70596`
Demonstrates auto-purge behavior when documents are moved to inaccessible channels.

```shell
$ cargo run --features=enterprise --example ticket_70596
```

#### `tombstone_purge_test_short`
Tests tombstone purge with a short interval (~5 minutes). Useful for quick validation of the test logic, though CBS may not actually purge tombstones below the 1-hour minimum.

**Runtime: ~10 minutes**

```shell
$ cargo run --features=enterprise --example tombstone_purge_test_short
```

#### `tombstone_purge_test`
Complete tombstone purge test following Couchbase support recommendations (Thomas). Tests whether tombstones can be completely purged from CBS and SGW after the minimum 1-hour interval, such that re-creating a document with the same ID is treated as a new document.

**Runtime: ~65-70 minutes** (+ ~5 minutes for Docker rebuild)
**Features**: Automatic Docker management, structured reporting

```shell
$ cargo run --features=enterprise --example tombstone_purge_test
```

**What it does automatically:**
- ✅ Checks git status (fails if uncommitted changes)
- ✅ Rebuilds Docker environment (docker compose down -v && up)
- ✅ Verifies CBS purge interval configuration
- ✅ Runs complete test with checkpoints
- ✅ Generates structured report in `test_results/`
- ✅ Captures CBS and SGW logs

**Test scenario:**
1. Create document in accessible channel and replicate
2. Delete document (creating tombstone)
3. Purge tombstone from Sync Gateway
4. Verify CBS purge interval (configured at bucket creation)
5. Wait 65 minutes
6. Compact CBS and SGW
7. Verify tombstone state (purged or persisting)
8. Re-create document with same ID and verify it's treated as new (flags=0, not flags=1)

**Report location**: `test_results/test_run_<timestamp>_<commit_sha>/`

### Utility functions

There are utility functions available in `examples/utils/` to interact with the Sync Gateway and Couchbase Server:
- **SGW admin operations**: user management, sessions, document operations, database lifecycle
- **CBS admin operations**: bucket compaction, document queries, tombstone management, metadata purge interval configuration

Feel free to add more if needed.
