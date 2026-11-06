//! The public address the core's DIRECT outbound leaves from. DIRECT uses the
//! machine's own route, so the app asks over that route itself: STUN servers
//! first, then the HTTP lookup services the user accepted. The probe is an
//! adapter behind [`DirectEgressProbe`]; the STUN codec is pure.
mod adapters;
mod ports;
mod stun;

pub use adapters::NetworkDirectEgressProbe;
#[cfg(any(test, feature = "test-support"))]
pub use ports::MockDirectEgressProbe;
pub use ports::{
    DirectEgress, DirectEgressProbe, ProbePermission, ProbeSources, StunAttempt, StunServerTest,
};
