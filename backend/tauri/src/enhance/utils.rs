use crate::config::profile::{item_type::ProfileUid, profiles::IProfiles};

use super::ChainItem;

pub fn convert_uids_to_scripts(profiles: &IProfiles, uids: &[ProfileUid]) -> Vec<ChainItem> {
    uids.iter()
        .filter_map(|uid| profiles.get_item(uid).ok())
        .filter_map(<Option<ChainItem>>::from)
        .collect::<Vec<ChainItem>>()
}
