//! __DATA,__thread_ptrs: pointers to thread-local variable descriptors, what
//! a TLVP-relocated instruction sequence loads from.

use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::macho::*;
use crate::symbol::SymbolId;
use crate::target::Target;

/// __DATA,__thread_ptrs: pointers to thread-local variable
/// descriptors, what a TLVP-relocated instruction sequence loads from.
#[derive(Debug)]
pub struct ThreadPtrsSection {
    pub hdr: ChunkHeader,
    /// Thread-local symbols with a __thread_ptrs slot, in slot order.
    pub symbols: Vec<SymbolId>,
}

impl ThreadPtrsSection {
    pub fn new() -> Self {
        let mut hdr = ChunkHeader::new("__DATA", "__thread_ptrs");
        hdr.flags = S_THREAD_LOCAL_VARIABLE_POINTERS;
        hdr.p2align = 3;
        Self { hdr, symbols: Vec::new() }
    }
}

impl Default for ThreadPtrsSection {
    fn default() -> Self {
        Self::new()
    }
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    for (i, &id) in ctx.thread_ptrs.symbols.iter().enumerate() {
        if !ctx.symbols[id].is_imported() {
            buf[i * 8..i * 8 + 8].copy_from_slice(&ctx.sym_addr(id).to_le_bytes());
        }
    }
}
