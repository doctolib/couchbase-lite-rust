# Running examples with Couchbase Sync Gateway & Server

Couchbase Lite is often used with replication to a central server, so it can be useful to test the full stack.
The examples in this directory aim at covering these use cases.

## Setup the Couchbase Sync Gateway & Server

This process is handled through docker images, with as an entry point the file `docker-conf/docker-compose.yml`.

The configuration files that might interest you are:
- `docker-conf/couchbase-server-dev/configure-server.sh` -> sets up the cluster, bucket and SG user
- `docker-conf/db-config.json` -> contains the database configuration
- `docker-conf/sync-function.js` -> contains the sync function used by the Sync Gateway

To start both the Sync Gatewawy and Couchbase Server, move to `docker-conf` through a terminal and use:

```shell
$ docker-compose up
```

It's very long the first time...

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

As of now, there is only one example: `sgw_1_cblite`.

It can be run with the following command:
```shell
$ cargo run --features=enterprise --example sgw_1_cblite
```

What it does:
- Create a cblite database `test1`
- Add a user `great_name` to the Sync Gateway
- Retrieve a session token for the user `great_name` from the Sync Gateway
- Start a continuous push & pull replicator
- Create a document, then wait for 5 seconds for the replication to finish
