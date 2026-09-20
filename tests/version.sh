#!/bin/bash
source "$(dirname "$0")"/common.inc

$mold -v | grep '[ms]old'

cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>

int main() {
  printf("Hello world\n");
}
EOF

$CC --ld-path=$mold -Wl,-v -o $t/exe $t/a.o | grep '[ms]old'
$t/exe | grep 'Hello world'
