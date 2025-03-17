#!/usr/bin/env bash

export COUCHBASE_ADMINISTRATOR_USERNAME="cb_admin"
export COUCHBASE_ADMINISTRATOR_PASSWORD="cb_admin_pwd"

export COUCHBASE_BUCKET="my-bucket"

export COUCHBASE_SG_USERNAME="syncgw"
export COUCHBASE_SG_PASSWORD="syncgw-pwd"
export COUCHBASE_SG_NAME="sg-service-user"

function retry() {
    for i in $(seq 1 10); do
      $1
      if [[ $? == 0 ]]; then
          return 0
      fi
	    sleep 1
    done
    return 1
}

function clusterInit() {
    couchbase-cli cluster-init \
        -c 127.0.0.1:8091 \
        --cluster-username $COUCHBASE_ADMINISTRATOR_USERNAME \
        --cluster-password $COUCHBASE_ADMINISTRATOR_PASSWORD \
        --services data,index,query \
        --cluster-ramsize 256 \
        --cluster-index-ramsize 256 \
        --index-storage-setting default
    if [[ $? != 0 ]]; then
      return 1
    fi
}

function bucketCreate() {
    couchbase-cli bucket-create \
        -c 127.0.0.1:8091 \
        --username $COUCHBASE_ADMINISTRATOR_USERNAME \
        --password $COUCHBASE_ADMINISTRATOR_PASSWORD \
        --bucket-type=couchbase \
        --bucket-ramsize=100 \
        --bucket-replica=0 \
        --bucket $COUCHBASE_BUCKET \
        --wait
    if [[ $? != 0 ]]; then
        return 1
    fi
}

function userSgCreate() {
    couchbase-cli user-manage \
        -c 127.0.0.1:8091 \
        --username $COUCHBASE_ADMINISTRATOR_USERNAME \
        --password $COUCHBASE_ADMINISTRATOR_PASSWORD \
        --set \
        --rbac-username $COUCHBASE_SG_USERNAME \
        --rbac-password $COUCHBASE_SG_PASSWORD \
        --rbac-name $COUCHBASE_SG_NAME \
        --roles bucket_full_access[*],bucket_admin[*] \
        --auth-domain local
    if [[ $? != 0 ]]; then
        return 1
    fi
}

function main() {
    /entrypoint.sh couchbase-server &
    if [[ $? != 0 ]]; then
        echo "Couchbase startup failed. Exiting." >&2
        exit 1
    fi

	  # wait for service to come up
    until $(curl --output /dev/null --silent --head --fail http://localhost:8091); do
        sleep 5
    done

    if couchbase-cli server-list -c 127.0.0.1:8091 --username $COUCHBASE_ADMINISTRATOR_USERNAME --password $COUCHBASE_ADMINISTRATOR_PASSWORD ; then
      echo "Couchbase already initialized, skipping initialization"
    else
      echo "Couchbase is not configured."
      echo

      echo "Initializing the cluster...."
      retry clusterInit
      if [[ $? != 0 ]]; then
        echo "Cluster init failed. Exiting." >&2
        exit 1
      fi
      echo "Initializing the cluster [OK]"
      echo

      echo "Creating the bucket...."
      retry bucketCreate
      if [[ $? != 0 ]]; then
        echo "Bucket create failed. Exiting." >&2
        exit 1
      fi
      echo "Creating the bucket [OK]"
      echo

      echo "Creating Sync Gateway user...."
      retry userSgCreate
      if [[ $? != 0 ]]; then
        echo "User create failed. Exiting." >&2
        exit 1
      fi
      echo "Creating Sync Gateway user [OK]"
      echo

      sleep 10

    fi

    wait
}

main

