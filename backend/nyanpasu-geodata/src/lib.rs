//! Read-only lookups over the geo databases the mihomo core consumes.
//!
//! Every source is compiled at load time into a compact, immutable index and the
//! source bytes are released: nothing keeps the core's files open or mapped, so
//! the core can rewrite them in place while an index is alive.
mod collection;
mod error;
mod files;
mod index;
mod parser;

pub use collection::tags::Tags;
pub use error::{GeoError, GeoResult};
pub use files::{MihomoGeoFiles, Source, read_source};
pub use index::{
    asn::{Asn, AsnIndex},
    ip::IpIndex,
    site::{SiteIndex, SiteMatch},
};
