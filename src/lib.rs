//! A high-performance Mach-O linker.

pub mod archive_file;
pub mod chunks;
pub mod cmdline;
pub mod context;
pub mod dead_strip;
pub mod driver;
pub mod dwarf;
pub mod error;
pub mod filetype;
pub mod icf;
pub mod input_files;
pub mod input_sections;
pub mod lto;
pub mod macho;
mod macho_consts;
pub mod mapfile;
pub mod mapped_file;
pub mod output_file;
pub mod passes;
pub mod relocatable;
pub mod subprocess;
pub mod symbol;
pub mod tapi;
pub mod target;
pub mod thunks;
pub mod util;
