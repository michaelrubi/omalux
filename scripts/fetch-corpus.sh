#!/bin/sh
# Fetch raw.pixls.us's CC0 ILCE-7M3 raws, the ones Lightcraft benchmarks
# on, into vendor/lightcraft/corpus/raw/ (gitignored), where the vendored
# crates' corpus tests and OmaLux's look for them. Checked by SHA-256, as
# Lightcraft's `cargo xtask corpus --download` checks them. Never commit
# them.
set -eu
dir="$(cd "$(dirname "$0")/.." && pwd)/vendor/lightcraft/corpus/raw"
mkdir -p "$dir"
fetch() {
    name=$1 url=$2 sum=$3
    if [ -f "$dir/$name" ] && echo "$sum  $dir/$name" | sha256sum -c --status; then
        echo "have $name"
        return
    fi
    curl -fL --retry 3 -o "$dir/$name.part" "$url"
    echo "$sum  $dir/$name.part" | sha256sum -c --status || { echo "$name: checksum mismatch" >&2; rm -f "$dir/$name.part"; exit 1; }
    mv "$dir/$name.part" "$dir/$name"
    echo "got $name"
}
fetch arw-sony-a7m3-compressed.arw \
    "https://raw.pixls.us/getfile.php/2414/nice/Sony%20-%20ILCE-7M3%20-%2014bit%2014bit%20compressed%20%283:2%29.ARW" \
    250784580ea527442c09004417bb0eead484f2bf3ee8f9121a776ac65bb50d0f
fetch arw-sony-a7m3-uncompressed.arw \
    "https://raw.pixls.us/getfile.php/2418/nice/Sony%20-%20ILCE-7M3%20-%2014bit%2014bit%20uncompressed%20%283:2%29.ARW" \
    ece80551abf64949dbe826985a80f1d9b265401ee4ea6b989e1829549616fdcd
