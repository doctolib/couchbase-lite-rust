#!/usr/bin/env bash

echo "Launch SG"
SG_CONFIG_PATH=/etc/sync_gateway/config.json

COUCHBASE_SERVER_URL="http://couchbase-server:8091"
SG_AUTH_ARG="syncgw:syncgw-pwd"

while ! { curl -X GET -u $SG_AUTH_ARG $COUCHBASE_SERVER_URL/pools/default/buckets -H "accept: application/json" -s | grep -q '"status":"healthy"'; }; do
  echo "Wait 🕑"
  sleep 1
done
echo "CB ready, starting SG"

sleep 5

/entrypoint.sh -bootstrap.use_tls_server=false  $SG_CONFIG_PATH


