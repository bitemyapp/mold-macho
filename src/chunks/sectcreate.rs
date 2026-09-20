//! A section created from a file by -sectcreate, or an empty one for
//! -add_empty_section and for a section only a boundary symbol names.

use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::target::Target;

/// A section created from a file by -sectcreate, or an empty one for
/// -add_empty_section and for a section only a boundary symbol names.
#[derive(Debug)]
pub struct SectCreateSection {
    pub hdr: ChunkHeader,
    pub contents: &'static [u8],
}

impl SectCreateSection {
    pub fn new(segname: &'static str, sectname: &str, contents: &'static [u8]) -> Self {
        let mut hdr = ChunkHeader::new(segname, sectname);
        hdr.size = contents.len() as u64;
        Self { hdr, contents }
    }
}

pub fn copy_buf<E: Target>(ctx: &Context<E>, idx: u32, buf: &mut [u8]) {
    let data = ctx.sectcreate_sections[idx as usize].contents;
    buf[..data.len()].copy_from_slice(data);
}
