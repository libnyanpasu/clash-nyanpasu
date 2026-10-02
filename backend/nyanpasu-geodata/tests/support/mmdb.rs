//! Minimal MaxMind DB writer (format 2.0, IPv6 tree, 24-bit records) for
//! building fixtures with exactly the networks and records a test needs.
use std::net::IpAddr;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Str(String),
    U32(u32),
    Map(Vec<(String, Value)>),
    Array(Vec<Value>),
}

pub fn s(value: &str) -> Value {
    Value::Str(value.to_owned())
}

pub fn map(entries: &[(&str, Value)]) -> Value {
    Value::Map(
        entries
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect(),
    )
}

pub fn array(items: &[&str]) -> Value {
    Value::Array(items.iter().map(|item| s(item)).collect())
}

#[derive(Clone, Copy)]
enum Child {
    Empty,
    Node(usize),
    Data(usize),
}

pub struct MmdbWriter {
    database_type: String,
    nodes: Vec<[Child; 2]>,
    values: Vec<Value>,
}

impl MmdbWriter {
    pub fn new(database_type: &str) -> Self {
        Self {
            database_type: database_type.to_owned(),
            nodes: vec![[Child::Empty; 2]],
            values: Vec::new(),
        }
    }

    /// `cidr` is `a.b.c.d/n` (stored under `::/96`) or an IPv6 network.
    pub fn insert(&mut self, cidr: &str, value: Value) -> &mut Self {
        let (ip, prefix) = cidr.split_once('/').expect("cidr");
        let prefix: usize = prefix.parse().expect("prefix");
        let (bits, depth) = match ip.parse::<IpAddr>().expect("ip") {
            IpAddr::V4(v4) => (u128::from(u32::from(v4)), prefix + 96),
            IpAddr::V6(v6) => (u128::from(v6), prefix),
        };
        let data = match self.values.iter().position(|v| *v == value) {
            Some(index) => index,
            None => {
                self.values.push(value);
                self.values.len() - 1
            }
        };
        let mut node = 0;
        for level in 0..depth {
            let bit = ((bits >> (127 - level)) & 1) as usize;
            if level + 1 == depth {
                self.nodes[node][bit] = Child::Data(data);
                break;
            }
            node = match self.nodes[node][bit] {
                Child::Node(next) => next,
                Child::Empty => {
                    self.nodes.push([Child::Empty; 2]);
                    let next = self.nodes.len() - 1;
                    self.nodes[node][bit] = Child::Node(next);
                    next
                }
                Child::Data(_) => panic!("{cidr} overlaps an inserted network"),
            };
        }
        self
    }

    pub fn build(&self) -> Vec<u8> {
        let mut data = Vec::new();
        let offsets: Vec<usize> = self
            .values
            .iter()
            .map(|value| {
                let offset = data.len();
                encode(&mut data, value);
                offset
            })
            .collect();
        let count = self.nodes.len();
        let record = |child: Child| -> u32 {
            match child {
                Child::Empty => count as u32,
                Child::Node(index) => index as u32,
                Child::Data(index) => (count + 16 + offsets[index]) as u32,
            }
        };
        let mut out = Vec::new();
        for [left, right] in &self.nodes {
            out.extend_from_slice(&record(*left).to_be_bytes()[1..]);
            out.extend_from_slice(&record(*right).to_be_bytes()[1..]);
        }
        out.extend_from_slice(&[0; 16]);
        out.extend_from_slice(&data);
        out.extend_from_slice(b"\xab\xcd\xefMaxMind.com");
        let mut metadata = Vec::new();
        control(&mut metadata, 7, 9);
        for (key, value) in [
            ("binary_format_major_version", Field::U16(2)),
            ("binary_format_minor_version", Field::U16(0)),
            ("build_epoch", Field::U64(1_700_000_000)),
            ("database_type", Field::Str(&self.database_type)),
            ("description", Field::Description),
            ("ip_version", Field::U16(6)),
            ("languages", Field::Languages),
            ("node_count", Field::U32(count as u32)),
            ("record_size", Field::U16(24)),
        ] {
            encode(&mut metadata, &s(key));
            match value {
                Field::U16(v) => uint(&mut metadata, 5, u64::from(v)),
                Field::U32(v) => uint(&mut metadata, 6, u64::from(v)),
                Field::U64(v) => uint(&mut metadata, 9, v),
                Field::Str(v) => encode(&mut metadata, &s(v)),
                Field::Description => encode(&mut metadata, &map(&[("en", s("fixture"))])),
                Field::Languages => encode(&mut metadata, &array(&["en"])),
            }
        }
        out.extend_from_slice(&metadata);
        out
    }
}

enum Field<'a> {
    U16(u16),
    U32(u32),
    U64(u64),
    Str(&'a str),
    Description,
    Languages,
}

fn encode(out: &mut Vec<u8>, value: &Value) {
    match value {
        Value::Str(text) => {
            control(out, 2, text.len());
            out.extend_from_slice(text.as_bytes());
        }
        Value::U32(number) => uint(out, 6, u64::from(*number)),
        Value::Map(entries) => {
            control(out, 7, entries.len());
            for (key, value) in entries {
                encode(out, &Value::Str(key.clone()));
                encode(out, value);
            }
        }
        Value::Array(items) => {
            control(out, 11, items.len());
            for item in items {
                encode(out, item);
            }
        }
    }
}

fn uint(out: &mut Vec<u8>, kind: u8, value: u64) {
    let bytes = value.to_be_bytes();
    let skip = bytes.iter().take_while(|b| **b == 0).count();
    control(out, kind, 8 - skip);
    out.extend_from_slice(&bytes[skip..]);
}

fn control(out: &mut Vec<u8>, kind: u8, size: usize) {
    let (size_bits, extra): (u8, Vec<u8>) = match size {
        0..29 => (size as u8, vec![]),
        29..285 => (29, vec![(size - 29) as u8]),
        285..65821 => (30, ((size - 285) as u16).to_be_bytes().to_vec()),
        _ => (31, ((size - 65821) as u32).to_be_bytes()[1..].to_vec()),
    };
    if kind <= 7 {
        out.push(kind << 5 | size_bits);
    } else {
        out.push(size_bits);
        out.push(kind - 7);
    }
    out.extend_from_slice(&extra);
}
