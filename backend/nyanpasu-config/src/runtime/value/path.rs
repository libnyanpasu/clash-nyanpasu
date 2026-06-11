use std::sync::Arc;

use indexmap::IndexMap;
use thiserror::Error;

use super::{ConfigValue, ConfigValue::*};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PathSegment {
    Key(Arc<str>),
    Index(usize),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ValuePathError {
    #[error("expected object at path segment {segment}")]
    ExpectedObject { segment: usize },
    #[error("expected array at path segment {segment}")]
    ExpectedArray { segment: usize },
    #[error("missing object key `{key}` at path segment {segment}")]
    MissingKey { segment: usize, key: Arc<str> },
    #[error("array index {index} is out of bounds for length {len} at path segment {segment}")]
    IndexOutOfBounds {
        segment: usize,
        index: usize,
        len: usize,
    },
    #[error("cannot remove the root config value")]
    RemoveRoot,
}

impl ConfigValue {
    /// Returns a copy with `value` set at `path`, cloning only the containers
    /// along the path spine and sharing every untouched subtree.
    pub fn set_path(
        &self,
        path: &[PathSegment],
        value: ConfigValue,
    ) -> Result<ConfigValue, ValuePathError> {
        self.set_path_at(path, value, 0)
    }

    /// Returns a copy with the node at `path` removed, sharing untouched subtrees.
    pub fn remove_path(&self, path: &[PathSegment]) -> Result<ConfigValue, ValuePathError> {
        if path.is_empty() {
            return Err(ValuePathError::RemoveRoot);
        }

        self.remove_path_at(path, 0)
    }

    fn set_path_at(
        &self,
        path: &[PathSegment],
        value: ConfigValue,
        depth: usize,
    ) -> Result<ConfigValue, ValuePathError> {
        let Some((segment, rest)) = path.split_first() else {
            return Ok(value);
        };

        match (segment, self) {
            (PathSegment::Key(key), Object(object)) => {
                let mut next = object.as_ref().clone();
                let updated = if rest.is_empty() {
                    value
                } else {
                    let child = next.get(key).ok_or_else(|| ValuePathError::MissingKey {
                        segment: depth,
                        key: key.clone(),
                    })?;
                    child.set_path_at(rest, value, depth + 1)?
                };
                next.insert(key.clone(), updated);
                Ok(Object(Arc::new(next)))
            }
            (PathSegment::Key(_), _) => Err(ValuePathError::ExpectedObject { segment: depth }),
            (PathSegment::Index(index), Array(array)) => {
                if *index >= array.len() {
                    return Err(ValuePathError::IndexOutOfBounds {
                        segment: depth,
                        index: *index,
                        len: array.len(),
                    });
                }

                let mut next = array.iter().cloned().collect::<Vec<_>>();
                next[*index] = if rest.is_empty() {
                    value
                } else {
                    next[*index].set_path_at(rest, value, depth + 1)?
                };
                Ok(Array(Arc::from(next)))
            }
            (PathSegment::Index(_), _) => Err(ValuePathError::ExpectedArray { segment: depth }),
        }
    }

    fn remove_path_at(
        &self,
        path: &[PathSegment],
        depth: usize,
    ) -> Result<ConfigValue, ValuePathError> {
        let Some((segment, rest)) = path.split_first() else {
            return Err(ValuePathError::RemoveRoot);
        };

        match (segment, self) {
            (PathSegment::Key(key), Object(object)) => {
                let mut next: IndexMap<Arc<str>, ConfigValue> = object.as_ref().clone();
                if rest.is_empty() {
                    next.shift_remove(key)
                        .ok_or_else(|| ValuePathError::MissingKey {
                            segment: depth,
                            key: key.clone(),
                        })?;
                } else {
                    let child = next.get(key).ok_or_else(|| ValuePathError::MissingKey {
                        segment: depth,
                        key: key.clone(),
                    })?;
                    let updated = child.remove_path_at(rest, depth + 1)?;
                    next.insert(key.clone(), updated);
                }
                Ok(Object(Arc::new(next)))
            }
            (PathSegment::Key(_), _) => Err(ValuePathError::ExpectedObject { segment: depth }),
            (PathSegment::Index(index), Array(array)) => {
                if *index >= array.len() {
                    return Err(ValuePathError::IndexOutOfBounds {
                        segment: depth,
                        index: *index,
                        len: array.len(),
                    });
                }

                let mut next = array.iter().cloned().collect::<Vec<_>>();
                if rest.is_empty() {
                    next.remove(*index);
                } else {
                    next[*index] = next[*index].remove_path_at(rest, depth + 1)?;
                }
                Ok(Array(Arc::from(next)))
            }
            (PathSegment::Index(_), _) => Err(ValuePathError::ExpectedArray { segment: depth }),
        }
    }
}
