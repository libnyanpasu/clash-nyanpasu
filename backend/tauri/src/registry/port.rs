use parking_lot::RwLock;
use std::{borrow::Cow, collections::HashMap, sync::Arc};

#[derive(Clone)]
pub struct PortRegistry {
    map: Arc<RwLock<PortMap>>,
}

impl Default for PortRegistry {
    fn default() -> Self {
        Self {
            map: Arc::new(RwLock::new(PortMap::new())),
        }
    }
}

impl PortRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<L: Into<Label>>(&self, port: u16, label: L) {
        self.map.write().register(port, label);
    }

    pub fn unregister(&self, port: u16) {
        self.map.write().unregister(port);
    }

    pub fn get_label(&self, port: u16) -> Option<Label> {
        self.map.read().get_label(port)
    }

    pub fn get_ports_by_label(&self, label: Label) -> Vec<u16> {
        self.map.read().get_ports_by_label(label)
    }

    pub fn clear(&self) {
        self.map.write().clear();
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Label(Cow<'static, str>);

impl From<&'static str> for Label {
    fn from(s: &'static str) -> Self {
        Label(Cow::Borrowed(s))
    }
}

impl From<String> for Label {
    fn from(s: String) -> Self {
        Label(Cow::Owned(s))
    }
}

impl From<Cow<'static, str>> for Label {
    fn from(s: Cow<'static, str>) -> Self {
        Label(s)
    }
}

impl AsRef<str> for Label {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

struct PortMap {
    port: HashMap<u16, Label>,
    label: HashMap<Label, Vec<u16>>,
}

impl PortMap {
    fn new() -> Self {
        Self {
            port: HashMap::new(),
            label: HashMap::new(),
        }
    }

    pub fn register<L: Into<Label>>(&mut self, port: u16, label: L) {
        let label = label.into();
        self.port.insert(port, label.clone());
        self.label.entry(label).or_insert_with(Vec::new).push(port);
    }

    pub fn unregister(&mut self, port: u16) {
        let label = self.port.remove(&port);
        if let Some(label) = label
            && let Some(labels) = self.label.get_mut(&label)
        {
            labels.retain(|p| *p != port);
            if labels.is_empty() {
                self.label.remove(&label);
            }
        }
    }

    /// Get the label of a port
    pub fn get_label(&self, port: u16) -> Option<Label> {
        self.port.get(&port).cloned()
    }

    /// Get the ports by a label
    pub fn get_ports_by_label(&self, label: Label) -> Vec<u16> {
        self.label.get(&label).cloned().unwrap_or_default()
    }

    pub fn clear(&mut self) {
        self.port.clear();
        self.label.clear();
    }
}
