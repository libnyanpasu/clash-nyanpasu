//! Compiled independently with `--no-default-features --features local-client`.
#![cfg(feature = "local-client")]

use nyanpasu_rpc_protocol::{
    client::{LocalClient, TypedCallError, operation_domain_error},
    operations::{CreateProfile, CreateProfileRequest, GetCoreStatus},
};

#[test]
fn protocol_only_operation_consumer_compiles() {
    async fn consumer(
        client: &LocalClient,
        request: CreateProfileRequest,
    ) -> Result<(), TypedCallError> {
        let _status = client.call_operation::<GetCoreStatus>(()).await?;
        match client.call_operation::<CreateProfile>(request).await {
            Ok(_outcome) => Ok(()),
            Err(error) => {
                let _domain = operation_domain_error::<CreateProfile>(&error).unwrap();
                Err(error)
            }
        }
    }
    let _ = consumer;
}
