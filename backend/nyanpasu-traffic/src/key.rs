//! Display keys borrowed from the session or a snapshot. Formatting a rule or chain is
//! deferred until a key is returned; hashing and comparisons stream their string parts.
use std::{
    cmp::Ordering,
    hash::{Hash, Hasher},
};

use crate::{Dimension, Dimensions, RuleKey};

#[derive(Clone, Copy, Debug)]
pub(crate) enum Key<'a> {
    Text(&'a str),
    Rule(&'a RuleKey),
    Chain(&'a [String]),
}

impl<'a> Key<'a> {
    pub(crate) fn of(d: &'a Dimensions, dimension: Dimension) -> Self {
        match dimension {
            Dimension::Rule if !d.rule.payload.is_empty() => Self::Rule(&d.rule),
            Dimension::Chain => Self::Chain(d.chains.get(1..).unwrap_or_default()),
            _ => Self::Text(match dimension {
                Dimension::Origin => d.origin(),
                Dimension::Process => &d.process,
                Dimension::Source => &d.source,
                Dimension::Inbound => &d.inbound,
                Dimension::Target => &d.target,
                Dimension::Protocol => &d.protocol,
                Dimension::Rule => &d.rule.kind,
                Dimension::Exit => d.exit(),
                Dimension::Profile => d.profile.as_deref().unwrap_or_default(),
                Dimension::SourceRegion => &d.source_region,
                Dimension::DestinationRegion => &d.destination_region,
                Dimension::DestinationBasis => d.destination_basis.map_or("", |b| b.as_str()),
                Dimension::Chain => unreachable!(),
            }),
        }
    }

    fn parts(self) -> Parts<'a> {
        match self {
            Self::Text(text) => Parts::Text(Some(text)),
            Self::Rule(rule) => {
                Parts::Rule([rule.kind.as_str(), ",", rule.payload.as_str()].into_iter())
            }
            Self::Chain(groups) => Parts::Chain {
                groups: groups.iter().rev(),
                separator: false,
            },
        }
    }

    pub(crate) fn owned(self) -> String {
        let mut text = String::with_capacity(self.parts().map(str::len).sum());
        for part in self.parts() {
            text.push_str(part);
        }
        text
    }

    pub(crate) fn is_empty(self) -> bool {
        self.parts().all(str::is_empty)
    }

    pub(crate) fn bytes(self) -> impl Iterator<Item = u8> + 'a {
        self.parts().flat_map(str::bytes)
    }
}

enum Parts<'a> {
    Text(Option<&'a str>),
    Rule(std::array::IntoIter<&'a str, 3>),
    Chain {
        groups: std::iter::Rev<std::slice::Iter<'a, String>>,
        separator: bool,
    },
}

impl<'a> Iterator for Parts<'a> {
    type Item = &'a str;
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Text(text) => text.take(),
            Self::Rule(parts) => parts.next(),
            Self::Chain { groups, separator } => {
                if *separator && groups.len() > 0 {
                    *separator = false;
                    Some(" → ")
                } else {
                    *separator = true;
                    groups.next().map(String::as_str)
                }
            }
        }
    }
}

impl PartialEq for Key<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.bytes().eq(other.bytes())
    }
}
impl Eq for Key<'_> {}
impl PartialOrd for Key<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Key<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.bytes().cmp(other.bytes())
    }
}
impl Hash for Key<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        for byte in self.bytes() {
            state.write_u8(byte);
        }
        state.write_usize(self.parts().map(str::len).sum());
    }
}

pub(crate) fn matches(d: &Dimensions, filters: &[crate::TrafficFilter]) -> bool {
    filters
        .iter()
        .all(|f| Key::of(d, f.dimension) == Key::Text(&f.value))
}
