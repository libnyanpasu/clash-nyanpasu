//! Owned receiving values for the separate connection-detail stream.

/// One connection plus its derived rates, for IPC consumers that need the
/// per-connection detail, owned by the receiver.
#[derive(serde::Deserialize, Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClashConnection {
    #[serde(flatten, deserialize_with = "super::connection_receive::deserialize")]
    pub connection: clash_api::Connection,
    pub download_speed: u64,
    pub upload_speed: u64,
}

/// An owned connection-detail frame received from a demand subscription.
#[derive(serde::Deserialize, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClashConnectionDetails {
    /// Equal to the sequence of the `ConnectionsUpdated` event emitted for
    /// the same sample, so a subscriber can align the two.
    pub sequence: u64,
    pub connections: Vec<ClashConnection>,
}
