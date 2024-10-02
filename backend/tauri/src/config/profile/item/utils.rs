use crate::{config::profile::item_type::ProfileItemType, utils::help};

pub fn generate_uid(kind: &ProfileItemType) -> String {
    match kind {
        ProfileItemType::Remote => help::get_uid("r"),
        ProfileItemType::Local => help::get_uid("l"),
        ProfileItemType::Script(_) => help::get_uid("s"),
        ProfileItemType::Merge => help::get_uid("m"),
    }
}
