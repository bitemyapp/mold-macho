//! LC_DATA_IN_CODE: ranges inside __text that hold data (jump tables, inline
//! constants), so disassemblers and the signature verifier can treat them
//! as bytes.

use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::target::Target;

/// LC_DATA_IN_CODE: ranges inside __text that hold data (jump tables,
/// inline constants), so disassemblers and the signature verifier can
/// treat them as bytes.
#[derive(Debug)]
pub struct DataInCodeSection {
    pub hdr: ChunkHeader,
    /// The entries (fileoff, length, kind), built once when layout
    /// reaches __LINKEDIT.
    pub entries: Vec<(u32, u16, u16)>,
}

impl DataInCodeSection {
    pub fn new() -> Self {
        Self { hdr: ChunkHeader::linkedit(), entries: Vec::new() }
    }
}

impl Default for DataInCodeSection {
    fn default() -> Self {
        Self::new()
    }
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    let mut p = 0;
    for &(off, len, kind) in &ctx.data_in_code.entries {
        buf[p..p + 4].copy_from_slice(&off.to_le_bytes());
        buf[p + 4..p + 6].copy_from_slice(&len.to_le_bytes());
        buf[p + 6..p + 8].copy_from_slice(&kind.to_le_bytes());
        p += 8;
    }
}

/// Live data-in-code entries as (subsection, offset within it,
/// length, kind). In an object, an entry's offset is an address in
/// the object's own address space (sections there are laid out from
/// zero), which find_subsec maps to the owning subsection - entries
/// whose subsection was dead-stripped vanish with it.
/// Builds the LC_DATA_IN_CODE entries. Runs when layout reaches
/// __LINKEDIT: the __text file offsets the entries record are final by
/// then, so the table is built exactly once (sold builds its contents
/// in compute_size the same way) and copied out verbatim.
pub fn build<E: Target>(ctx: &Context<E>) -> Vec<(u32, u16, u16)> {
    let mut out: Vec<(u32, u16, u16)> = Vec::new();
    for obj in &ctx.objs {
        if !obj.is_alive {
            continue;
        }
        for &(off, len, kind) in &obj.dice {
            let Some((isec, off_in)) =
                crate::input_files::find_subsec(&ctx.isecs, &obj.subsecs, off as u64)
            else {
                continue;
            };
            let isec = &ctx.isecs[ctx.resolve_isec(isec)];
            if isec.is_alive() {
                let fileoff = ctx.chunk_header(isec.output_section().unwrap()).fileoff
                    + isec.offset as u64
                    + off_in;
                out.push((fileoff as u32, len, kind));
            }
        }
    }
    out.sort_unstable();
    out
}
