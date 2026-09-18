#!/usr/bin/env bash
. $(dirname $0)/common.inc

[ "$ARCH" = arm64 ] || skip

# ARM64 ldr q PAGEOFF12 is scaled by 16. A 16-byte literal that
# lands at 8 mod 16 cannot be encoded; dropping the low bits loads
# the neighboring slot. rustc libtest hits this when splitn's
# (1, 0) initializer is placed after an 8-byte atom and --list is
# parsed as "ist".
#
# The first object starts __literal16 on an 8-mod-16 address
# (p2align 3, after an 8-byte __const), so the old layout kept the
# SIMD constant at 8 mod 16. The second object has the same bytes,
# so literal merge keeps one copy.

cat <<EOF2 | $CC -o $t/a.o -c -xassembler -
.section __TEXT,__const
.p2align 3
.quad 0x1111111111111111

.section __TEXT,__literal16,16byte_literals
.p2align 3
.globl _lCPI
_lCPI:
  .quad 1, 0
EOF2

cat <<EOF2 | $CC -o $t/b.o -c -xassembler -
.section __TEXT,__literal16,16byte_literals
.p2align 4
.quad 1, 0
EOF2

cat <<EOF2 | $CC -o $t/load.o -c -xassembler -
.globl _load_cpi
_load_cpi:
  adrp x8, _lCPI@PAGE
  ldr q0, [x8, _lCPI@PAGEOFF]
  mov x0, v0.d[0]
  mov x1, v0.d[1]
  ret
EOF2

cat <<EOF2 | $CC -o $t/main.o -c -xc -
#include <stdint.h>
#include <stdio.h>

typedef struct { unsigned long lo, hi; } Pair;
Pair load_cpi(void);
extern unsigned long lCPI[2];

int main(void) {
  Pair p = load_cpi();
  uintptr_t addr = (uintptr_t)lCPI;
  printf("%lu %lu %d\n", p.lo, p.hi, (int)(addr % 16));
  return (p.lo == 1 && p.hi == 0 && addr % 16 == 0) ? 0 : 1;
}
EOF2

$CC --ld-path=$mold -o $t/exe $t/main.o $t/load.o $t/a.o $t/b.o
$t/exe | grep -q '^1 0 0$'
