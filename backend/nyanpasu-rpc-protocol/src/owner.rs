//! Caller identity values; transport adapters assign and parse the labels.

/// An opaque caller label assigned by the transport that owns the resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpcOwner(String);

impl RpcOwner {
    /// Construct an owner from a label assigned by a transport adapter.
    pub fn new(label: String) -> Self {
        Self(label)
    }

    /// Retain the invoking desktop window label as its resource owner.
    pub fn desktop(label: &str) -> Self {
        Self(label.to_owned())
    }
    /// Borrow the resource owner label without changing its transport identity.
    pub fn label(&self) -> &str {
        &self.0
    }
}
