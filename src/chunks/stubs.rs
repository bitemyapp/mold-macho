//! __TEXT,__stubs: jump stubs for calls to imported functions.

use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::macho::*;
use crate::symbol::SymbolId;
use crate::target::Target;

/// __TEXT,__stubs: jump stubs for calls to imported functions.
#[derive(Debug)]
pub struct StubsSection {
    pub hdr: ChunkHeader,
    /// Symbols with a __stubs entry, in stub order.
    pub symbols: Vec<SymbolId>,
}

impl StubsSection {
    pub fn new() -> Self {
        let mut hdr = ChunkHeader::new("__TEXT", "__stubs");
        hdr.flags = S_SYMBOL_STUBS | S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS;
        hdr.p2align = 2;
        Self { hdr, symbols: Vec::new() }
    }
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    E::write_stubs(ctx, ctx.stubs.hdr.addr, buf);
}
