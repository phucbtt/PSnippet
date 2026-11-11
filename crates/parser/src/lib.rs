//! Import/export for PSnippet: legacy AHK-like `::code::( ... )return` blocks and `.zsnip` JSON.
//! The parser is plain-text only: it never executes anything (spec §4.1).

pub mod json;
pub mod legacy;
pub mod seeds;

pub use json::{ExportError, export_json, import_json};
pub use legacy::{LegacySnippet, ParseError, export_legacy, parse_legacy};
pub use seeds::{DEFAULT_CATEGORIES, DEFAULT_SEEDS_TEXT, seed_snippets};
