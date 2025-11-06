#!/bin/bash

set -e

NAME=wireguard-docker-plugin
REGISTRY=localhost:8000
REPOSITORY=wireguard
PLUGIN_NAME=$REGISTRY/$REPOSITORY
ALL_TARGETS="
    x86_64-unknown-linux-musl;amd64
    aarch64-unknown-linux-musl;arm64
"

VERSION=$(cargo metadata --frozen --format-version=1 | jq -r '.workspace_default_members[] as $pkgid | .packages[] | select(.id == $pkgid and .targets[].name == $name) | .version' --arg name $NAME)

if [ ! -r plugin/config.json ]; then
    echo "Config file not found. Aborting."
    exit 1
fi

for TARGET in $ALL_TARGETS; do
    TARGET_TRIPLE=${TARGET%;*}
    GOARCH=${TARGET#*;}
    echo "Building for $GOARCH ($TARGET_TRIPLE)"
    # Once with human-readable output
    cargo build --release --target $TARGET_TRIPLE
    # Once again only to parse the JSON output
    BIN_PATH=$(
        cargo build --message-format=json --release --target $TARGET_TRIPLE 2>/dev/null | \
        jq -r 'select(.reason == "compiler-artifact" and .target.name == $name) | last(.executable)' --arg name $NAME
    )
    [ -e "plugin/rootfs/$NAME" ] && rm "plugin/rootfs/$NAME"
    cp "$BIN_PATH" "plugin/rootfs/$NAME"
    TAG="$VERSION-$GOARCH"
    TAGGED_PLUGIN_NAME="$PLUGIN_NAME:$TAG"
    docker plugin rm $TAGGED_PLUGIN_NAME && echo "deleted previous version" || true
    docker plugin create $TAGGED_PLUGIN_NAME ./plugin
    docker plugin push $TAGGED_PLUGIN_NAME
    url=$REGISTRY/v2/$REPOSITORY/manifests/$TAG
    echo "url: $url"
    output=$(curl -sS -H 'Accept: application/vnd.docker.distribution.manifest.v2+json' -w '%header{Docker-Content-Digest} %{size_download}' -o /dev/null $url)
    manifest_digest=${output%% *}
    manifest_size=${output##* }
    echo "manifest_digest: $manifest_digest"
    echo "manifest_size: $manifest_size"
    MANIFESTS="$MANIFESTS $GOARCH;$manifest_digest;$manifest_size"
done

echo "----------------------"
echo "MANIFESTS: $MANIFESTS"
echo '{
  "schemaVersion": 2,
  "mediaType": "application/vnd.docker.distribution.manifest.list.v2+json",
  "manifests": [' > manifest-list.json

jsonprefix=""
for manifest in $MANIFESTS; do
    arch=${manifest%%;*}
    size=${manifest##*;}
    digest=${manifest#*;}
    digest=${digest%;*}

    echo "$jsonprefix  {
    \"mediaType\": \"application/vnd.docker.distribution.manifest.v2+json\",
    \"size\": $size,
    \"digest\": \"$digest\",
    \"platform\": {
      \"architecture\": \"$arch\",
      \"os\": \"linux\"
    }
    }" >> manifest-list.json
    jsonprefix=","
done

echo '  ]
}' >> manifest-list.json

echo "----------------------"
curl -sS -X PUT -H 'Content-Type: application/vnd.docker.distribution.manifest.list.v2+json' --data-binary @manifest-list.json $REGISTRY/v2/$REPOSITORY/manifests/$VERSION
