//! Small helpers shared across the linker.

pub mod demangle;
pub mod glob;
pub mod perf;

/// Rounds `value` up to a multiple of `align`, which must be zero or a power
/// of two. Zero means "no alignment".
#[inline]
pub fn align_to(value: u64, align: u64) -> u64 {
    if align == 0 {
        return value;
    }
    debug_assert!(align.is_power_of_two());
    (value + align - 1) & !(align - 1)
}

/// Rounds `val` up to the next value congruent to `modulus` modulo
/// `align`: the smallest x >= val with x % align == modulus. ld64
/// places every atom this way, keeping the offset it had within its
/// section modulo the section's alignment, not merely rounding up to
/// the section's alignment.
pub fn align_to_mod(val: u64, align: u64, modulus: u64) -> u64 {
    debug_assert!(align.is_power_of_two() && modulus < align);
    if val <= modulus { modulus } else { align_to(val - modulus, align) + modulus }
}

// Returns [hi:lo] bits of val.
#[inline]
pub fn bits(value: u64, hi: u32, lo: u32) -> u64 {
    (value >> lo) & ((1u64 << (hi - lo + 1)) - 1)
}

// Cast val to a signed N bit integer.
// For example, sign_extend(x, 32) == (i32)x for any integer x.
pub fn sign_extend(value: u64, n: u32) -> i64 {
    ((value << (64 - n)) as i64) >> (64 - n)
}

/// A sort key that orders strings like the strings themselves but
/// settles most comparisons on one integer: the first eight bytes,
/// big-endian, zero-padded. Symbol names cannot contain NULs, so
/// (prefix, name) order equals plain name order. Mach-O sorts its
/// global symbols and export-trie input by name (ELF mold never
/// name-sorts), and mangled names share long prefixes, which makes
/// plain str comparison the sort's bottleneck.
pub fn name_sort_key(name: &str) -> (u64, &str) {
    let b = name.as_bytes();
    let mut p = [0u8; 8];
    let n = b.len().min(8);
    p[..n].copy_from_slice(&b[..n]);
    (u64::from_be_bytes(p), name)
}

/// Appends `value` in unsigned LEB128 encoding.
pub fn encode_uleb(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// Appends `value` in signed LEB128 encoding.
pub fn encode_sleb(out: &mut Vec<u8>, mut value: i64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        let negative = byte & 0x40 != 0;
        if (value == 0 && !negative) || (value == -1 && negative) {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// Computes the SHA-256 hash of `data` into `out`.
///
/// libSystem, which every macOS process links against, exports the
/// CommonCrypto implementation, so we use it rather than pulling in a
/// Rust crypto crate.
pub fn sha256(data: &[u8], out: &mut [u8; 32]) {
    unsafe extern "C" {
        fn CC_SHA256(data: *const u8, len: u32, md: *mut u8) -> *mut u8;
    }
    // SAFETY: CC_SHA256 reads `len` bytes and writes exactly 32 bytes.
    unsafe {
        CC_SHA256(data.as_ptr(), data.len() as u32, out.as_mut_ptr());
    }
}
