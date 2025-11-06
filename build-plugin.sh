#!/bin/sh

set -e

NAME=wireguard-docker-plugin
REGISTRY=localhost:8000
REPOSITORY=sorcio/wireguard
PLUGIN_NAME=$REGISTRY/$REPOSITORY
ALL_TARGETS="
    x86_64-unknown-linux-musl;amd64
    aarch64-unknown-linux-musl;arm64
"

version=$(cargo metadata --frozen --format-version=1 | jq -r '.workspace_default_members[] as $pkgid | .packages[] | select(.id == $pkgid and .targets[].name == $name) | .version' --arg name $NAME)

if [ ! -r plugin/config.json ]; then
    echo "Config file not found. Aborting."
    exit 1
fi

for target in $ALL_TARGETS; do
    target_triple=${target%;*}
    goarch=${target#*;}
    echo "Building for $goarch ($target_triple)"
    # Once with human-readable output
    cargo build --release --target $target_triple
    # Once again only to parse the JSON output
    bin_path=$(
        cargo build --message-format=json --release --target $target_triple 2>/dev/null | \
        jq -r 'select(.reason == "compiler-artifact" and .target.name == $name) | last(.executable)' --arg name $NAME
    )
    [ -e "plugin/rootfs/$NAME" ] && rm "plugin/rootfs/$NAME"
    cp "$bin_path" "plugin/rootfs/$NAME"
    tag="$version-$goarch"
    tagged_plugin_name="$PLUGIN_NAME:$tag"
    docker plugin rm $tagged_plugin_name 2> /dev/null && echo "deleted previous version" || true
    docker plugin create $tagged_plugin_name ./plugin
    docker plugin push $tagged_plugin_name
done
