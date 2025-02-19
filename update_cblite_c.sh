#!/usr/bin/env bash

RED="\e[31m"
GREEN="\e[32m"
ENDCOLOR="\e[0m"

function echoGreen() {
    echo -e "${GREEN}$1${ENDCOLOR}"
}

function echoRed() {
    echo -e "${RED}$1${ENDCOLOR}"
}

scriptDir=$(dirname "$0")
echo "Script directory: $scriptDir"

# ####### #
# Options #
# ####### #

function help() {
    echo "Download & Setup new CBlite packages"
    echo
    echo "  -v  CBlite version (ie. 3.2.1)"
    echo "  -h  print this help"
}

while getopts ":v:h" option
do
  case $option in
    v)
      version="$OPTARG"
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

if [[ -z "$version" ]]
then
  >&2 echoRed "All required parameters are not set"
  help
  exit 1
else
  echoGreen "All good, let's start with CBlite $version :-)"
fi

declare -i errors=0
function checkErrors() {
    if [ ${errors} -ne 0 ]
    then
        >&2 echoRed "Failed at the following step: $1"
        exit 1
    fi
}

tmpFolder=$(mktemp -d)
echo "Temporary directory ${tmpFolder}"

# ################################## #
# Download couchbase-lite-C packages #
# ################################## #

echoGreen "Start downloading"

tmpDownloadFolder="${tmpFolder}/download"
mkdir $tmpDownloadFolder
echo "Temporary download directory ${tmpFolder}"

declare -A platforms=(
    [linux]=linux-x86_64.tar.gz 
    [windows]=windows-x86_64.zip
    [macos]=macos.zip
    [android]=android.zip
    [ios]=ios.zip
)

function download() {
    local suffix="$1"

    local url="https://packages.couchbase.com/releases/couchbase-lite-c/${version}/couchbase-lite-c-enterprise-${version}-${suffix}"
    local file="${tmpDownloadFolder}/${suffix}"

    if ! wget --quiet --show-progress --output-document "${file}" "${url}"
    then
        >&2 echo "Unable to download '${url}'."
        errors+=1
    fi
}

for platform in "${!platforms[@]}"
do
    echo "Downloading ${platform} package"

    fileName=${platforms[$platform]}
    download $fileName
done

checkErrors "Download packages"
echoGreen "Downloading successful"

# ############## #
# Unzip packages #
# ############## #

echoGreen "Start unzipping"

tmpUnzipFolder="${tmpFolder}/unzip"
mkdir $tmpUnzipFolder
echo "Temporary unzip directory ${tmpUnzipFolder}"

for platform in "${!platforms[@]}"
do
    echo "Unzipping ${platform} package"

    fileName=${platforms[$platform]}
    zippedPath="$tmpDownloadFolder/$fileName"

    unzipPlatformFolder="${tmpUnzipFolder}/$platform"
    mkdir $unzipPlatformFolder

    tar -x -f $zippedPath --directory $unzipPlatformFolder
done

echoGreen "Unzipping successful"

# ######################## #
# Package libcblite folder #
# ######################## #

echoGreen "Package libcblite"

tmpLibcbliteFolder="${tmpFolder}/libcblite"
mkdir $tmpLibcbliteFolder
echo "Temporary libcblite directory ${tmpLibcbliteFolder}"

libFolder="${tmpLibcbliteFolder}/lib"
mkdir $libFolder

for platform in "${!platforms[@]}"
do
    echo "Packaging ${platform}"

    unzipPlatformFolder="${tmpUnzipFolder}/$platform"

    case $platform in

        linux)
            platformFolder="${libFolder}/x86_64-unknown-linux-gnu"
            mkdir $platformFolder

            libFile="${unzipPlatformFolder}/libcblite-${version}/lib/x86_64-linux-gnu/libcblite.so.${version}"
            libDestinationFile="${platformFolder}/libcblite.so.3"
            cp $libFile $libDestinationFile

            # There are required ICU libs already present in the existing package
            cp $scriptDir/libcblite/lib/x86_64-unknown-linux-gnu/libicu* $platformFolder

            ;;

        windows)
            platformFolder="${libFolder}/x86_64-pc-windows-gnu"
            mkdir $platformFolder

            libFile="${unzipPlatformFolder}/libcblite-${version}/lib/cblite.lib"
            cp $libFile $platformFolder

            binFile="${unzipPlatformFolder}/libcblite-${version}/bin/cblite.dll"
            cp $binFile $platformFolder

            ;;

        macos)
            platformFolder="${libFolder}/macos"
            mkdir $platformFolder

            libFile="${unzipPlatformFolder}/libcblite-${version}/lib/libcblite.${version}.dylib"
            libDestinationFile="${platformFolder}/libcblite.3.dylib"
            cp $libFile $libDestinationFile

            ;;

        android)
            # aarch64
            platformFolder="${libFolder}/aarch64-linux-android"
            mkdir $platformFolder

            libFile="${unzipPlatformFolder}/libcblite-${version}/lib/aarch64-linux-android/libcblite.so"
            cp $libFile $platformFolder

            # arm
            platformFolder="${libFolder}/arm-linux-androideabi"
            mkdir $platformFolder

            libFile="${unzipPlatformFolder}/libcblite-${version}/lib/arm-linux-androideabi/libcblite.so"
            cp $libFile $platformFolder

            # i686
            platformFolder="${libFolder}/i686-linux-android"
            mkdir $platformFolder

            libFile="${unzipPlatformFolder}/libcblite-${version}/lib/i686-linux-android/libcblite.so"
            cp $libFile $platformFolder

            # x86_64
            platformFolder="${libFolder}/x86_64-linux-android"
            mkdir $platformFolder

            libFile="${unzipPlatformFolder}/libcblite-${version}/lib/x86_64-linux-android/libcblite.so"
            cp $libFile $platformFolder

            # Some files/directories must be moved only once for all platforms: include directory & LICENSE.txt
            cp -R "${unzipPlatformFolder}/libcblite-${version}/include" $tmpLibcbliteFolder

            cp "${unzipPlatformFolder}/libcblite-${version}/LICENSE.txt" $tmpLibcbliteFolder

            ;;

        ios)
            platformFolder="${libFolder}/ios"
            mkdir $platformFolder

            cp -R "${unzipPlatformFolder}/CouchbaseLite.xcframework" $platformFolder

            ;;
    esac
done

echoGreen "Packaging libcblite successful"

# ######################## #
# Replace libcblite folder #
# ######################## #

echoGreen "Replace libcblite directory by newly package one"

rm -rf $scriptDir/libcblite

cp -R $libFolder $scriptDir

echoGreen "Replacing libcblite successful"

# ################### #
# Strip the libraries #
# ################### #

DOCKER_BUILDKIT=1 docker build --file $scriptDir/Dockerfile -t strip --output $scriptDir/libcblite $scriptDir
