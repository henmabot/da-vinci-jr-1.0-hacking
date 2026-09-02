#!/bin/sh
set -eu

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)
output="$repo_root/build/firmata-tests"
mkdir -p "$output"

${CC:-cc} \
    -std=c11 -Wall -Wextra -Werror \
    -I"$repo_root/firmata/src" \
    -I"$repo_root/firmata/conf" \
    "$repo_root/firmata/tests/test_firmata.c" \
    "$repo_root/firmata/src/firmata.c" \
    -o "$output/test_firmata"

"$output/test_firmata"
