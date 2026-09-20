//! __TEXT,__stub_helper: with classic dyld info, the code a lazy pointer
//! initially points at, which enters dyld_stub_binder with the pointer's
//! lazy-bind record.

use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::macho::*;
use crate::symbol::SymbolId;
use crate::target::Target;

/// __TEXT,__stub_helper: with classic dyld info, the code a lazy
/// pointer initially points at, which enters dyld_stub_binder with the
/// pointer's lazy-bind record.
#[derive(Debug)]
pub struct StubHelperSection {
    pub hdr: ChunkHeader,
    /// dyld_stub_binder, resolved from the loaded dylibs when lazy
    /// binding is in use, and the __dyld_private word the stub helper
    /// hands it (a synthesized record in __DATA,__data).
    pub dyld_stub_binder: Option<SymbolId>,
    pub dyld_private_isec: u32,
}

impl StubHelperSection {
    pub fn new() -> Self {
        let mut hdr = ChunkHeader::new("__TEXT", "__stub_helper");
        hdr.flags = S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS;
        hdr.p2align = 2;
        Self { hdr, dyld_stub_binder: None, dyld_private_isec: u32::MAX }
    }
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    E::write_stub_helper(ctx, ctx.stub_helper.hdr.addr, buf);
}
