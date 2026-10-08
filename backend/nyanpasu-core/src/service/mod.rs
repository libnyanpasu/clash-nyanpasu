pub mod actor;
mod compat;
pub mod control;
pub mod os;

pub use compat::{
    REQUIRED_SERVICE_MAJOR, REQUIRED_SERVICE_MIN, ServiceCompat, parse_service_version,
};
