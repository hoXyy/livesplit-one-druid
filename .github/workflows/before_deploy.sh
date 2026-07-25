set -ex

main() {
    local tag=$(git tag --points-at HEAD)
    local src=$(pwd)
    local stage=$(mktemp -d)

    cp "target/$TARGET/max-opt/livesplit-one" "$stage/LiveSplitOne"
    tar -C "$stage" -czf "$src/livesplit-one-$tag-$RELEASE_TARGET.tar.gz" LiveSplitOne
    rm -rf "$stage"
}

main
