#!/bin/sh -x

apk add curl

export DB_NAME="my-db"

echo 'START SG Update'
echo

# Setting up database API: https://docs.couchbase.com/sync-gateway/current/rest_api_admin.html#tag/Database-Management/operation/put_db-
echo 'Setting up the database...'
curl -XPUT -v "http://sg:4985/${DB_NAME}/" -H 'Content-Type: application/json' --data-binary @db-config.json
echo

# Updating sync function API: https://docs.couchbase.com/sync-gateway/current/rest_api_admin.html#tag/Database-Configuration/operation/put_keyspace-_config-sync
# Sync function doc: https://docs.couchbase.com/sync-gateway/current/sync-function.html
echo 'Updating sync function...'
curl -XPUT -v "http://sg:4985/${DB_NAME}/_config/sync" -H 'Content-Type: application/javascript' --data-binary @sync-function.js

echo 'END SG Update'
