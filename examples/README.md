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

### Available examples

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

**Runtime: ~65-70 minutes**

```shell
$ cargo run --features=enterprise --example tombstone_purge_test
```

**Test scenario:**
1. Create document in accessible channel and replicate
2. Delete document (creating tombstone)
3. Purge tombstone from Sync Gateway
4. Configure CBS metadata purge interval to 1 hour
5. Wait 65 minutes
6. Compact CBS and SGW
7. Verify tombstone no longer exists
8. Re-create document with same ID and verify it's treated as new (flags=0, not flags=1)

### Utility functions

There are utility functions available in `examples/utils/` to interact with the Sync Gateway and Couchbase Server:
- **SGW admin operations**: user management, sessions, document operations, database lifecycle
- **CBS admin operations**: bucket compaction, document queries, tombstone management, metadata purge interval configuration

Feel free to add more if needed.
