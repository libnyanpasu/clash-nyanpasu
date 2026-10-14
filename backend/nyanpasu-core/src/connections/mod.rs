mod interrupt;
mod rates;

pub use interrupt::{ConnectionScope, interrupt_connections};
pub use rates::{
    ClashConnection, ClashConnectionsSummary, ConnectionCounters, ConnectionRates,
    DerivedConnections, TrafficRate,
};
