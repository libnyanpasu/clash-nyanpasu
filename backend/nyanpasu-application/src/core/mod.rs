//! Platform-neutral runtime ownership router and endpoint contract.

mod actor;
pub mod api;
pub mod endpoint;

pub use actor::{
    ControllerGeneration, CoreClient, CoreObserver, CoreStatusInfo, CoreStatusProjection,
    EndpointConnectivity, HandoffReport, ShutdownReport, SubmitFailure, SubmitTicket,
};
pub use endpoint::{
    ApiChanges, CheckSubmission, CheckSupport, ControlEndpoint, CoreStatusSnapshot, CoreSubmission,
    EndpointHandle, ExecutionHost, wire_core_type_to_kind,
};
