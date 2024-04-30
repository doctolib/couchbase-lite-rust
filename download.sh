#!/usr/bin/env bash

function help() {
    echo "Download CBlite files"
    echo
    echo "  -v  CBlite version (ie. 3.0.3)"
    echo "  -d  destination for files"
    echo "  -h  print this help"
}

while getopts ":v:d:h" option
do
  case $option in
    v)
      version="$OPTARG"
      ;;
    d)
      dest="$OPTARG"
      ;;
    h)
      help
      exit
      ;;
    \?)
      >&2 echo "Invalid option."
      help
      exit 1
      ;;
  esac
done

if [[ -z "$version" \
  || -z "$dest" \
]]
then
  >&2 echo "All required parameters are not set."
  help
  exit 1
else
  echo "Downloading CBlite $version files…"
fi

mkdir -p "${dest}"

declare -i errors=0
function download() {

    suffix="$1"

    url="https://packages.couchbase.com/releases/couchbase-lite-c/${version}/couchbase-lite-c-enterprise-${version}-${suffix}"
    file="${dest}/couchbase-lite-c-enterprise-${version}-${suffix}"

    if ! wget --quiet --show-progress --output-document "${file}" "${url}"
    then
        >&2 echo "Unable to download '${url}'."
        errors+=1
    fi
}

download ubuntu20.04-x86_64.tar.gz
download windows-x86_64.zip
download macos.zip
download android.zip
download ios.zip

if [ ${errors} -ne 0 ]
then
    >&2 echo "Failed to download all required CBlite packages."
    exit 1
fi
