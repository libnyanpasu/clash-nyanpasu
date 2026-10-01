//! Read-only lookups over the geo databases the mihomo core consumes.
//!
//! Every source is compiled at load time into a compact, immutable index and the
//! source bytes are released: nothing keeps the core's files open or mapped, so
//! the core can rewrite them in place while an index is alive.
mod asn;
mod error;
mod files;
mod geoip_dat;
mod ip;
mod mmdb;
mod proto;
mod scratch;
mod site;
mod table;
mod tags;

pub use asn::{Asn, AsnIndex};
pub use error::{GeoError, GeoResult};
pub use files::{MihomoGeoFiles, Source, read_source};
pub use ip::IpIndex;
pub use site::{SiteIndex, SiteMatch};
pub use tags::Tags;
