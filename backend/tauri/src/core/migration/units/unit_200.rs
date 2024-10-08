// use std::borrow::Cow;

// use once_cell::sync::Lazy;
// use semver::Version;
// use serde_yaml::Mapping;

// use crate::{
//     core::migration::{
//         DynMigration,
//         Migration
//     },
//     utils::dirs,
// };

// pub static UNITS: Lazy<Vec<DynMigration>> = Lazy::new(|| {
//     vec![
//     MigrateMultipleSubscriptionExtras.into()
//     ]
// });

// pub static VERSION: Lazy<semver::Version> = Lazy::new(|| semver::Version::parse("2.0.0").unwrap());

// #[derive(Debug, Clone)]
// pub struct MigrateMultipleSubscriptionExtras;

// impl Migration<'_> for MigrateMultipleSubscriptionExtras {
//     fn version(&self) -> &'static Version {
//         &VERSION
//     }

//     fn name(&self) -> Cow<'static, str> {
//         Cow::Borrowed("Migrate multiple subscription extras")
//     }

//     fn migrate(&self) -> std::io::Result<()> {
//         let profiles_path =
//             dirs::profiles_path().map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
//         let profiles = std::fs::read_to_string(profiles_path.clone())?;
//         let mut profiles: Mapping = serde_yaml::from_str(&profiles).map_err(|e| {
//             std::io::Error::new(
//                 std::io::ErrorKind::Other,
//                 format!("failed to parse profiles: {}", e),
//             )
//         })?;

//         let mut binding = serde_yaml::Value::Sequence(Vec::new());
//         let items = profiles
//             .get_mut("items")
//             .unwrap_or(&mut binding)
//             .as_sequence_mut()
//             .ok_or_else(|| {
//                 std::io::Error::new(
//                     std::io::ErrorKind::Other,
//                     "items is not a sequence in profiles",
//                 )
//             })?;
//         items
//             .iter_mut()
//             .filter_map(|item| item.as_mapping_mut())
//             .for_each(|item| {
//                 let extra = item.get("extra");
//                 // modify the extra field to be a mapping
//                 if let Some(extra) = extra
//                     && extra.is_mapping()
//                     && serde_yaml::from_value::<crate::config::profile::item::SubscriptionInfo>(
//                         extra.clone(),
//                     )
//                     .is_ok()
//                 {
//                     println!(
//                         "detected extra in item {:?} should be migrated:\n{:?}",
//                         item.get("uid").unwrap(),
//                         extra
//                     );
//                     let extra = extra.clone();
//                     let mut map = Mapping::new();
//                     let url = item.get("url").unwrap().clone();
//                     map.insert(url, extra);
//                     item.insert("extra".into(), serde_yaml::Value::Mapping(map));
//                 }

//                 // migrate urls
//                 let url = item.get("url");
//                 if let Some(url) = url
//                     && url.is_string()
//                 {
//                     println!(
//                         "detected url in item {:?} should be migrated:\n{:?}",
//                         item.get("uid").unwrap(),
//                         url
//                     );
//                     let url = url.clone();
//                     let seq = serde_yaml::Value::Sequence(vec![url]);
//                     item.insert("url".into(), seq);
//                 }
//             });
//         let file = std::fs::OpenOptions::new()
//             .write(true)
//             .truncate(true)
//             .open(profiles_path)?;
//         serde_yaml::to_writer(file, &profiles)
//             .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
//         Ok(())
//     }

//     fn discard(&self) -> std::io::Result<()> {
//         let profiles_path =
//             dirs::profiles_path().map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
//         let profiles = std::fs::read_to_string(profiles_path.clone())?;
//         let mut profiles: Mapping = serde_yaml::from_str(&profiles).map_err(|e| {
//             std::io::Error::new(
//                 std::io::ErrorKind::Other,
//                 format!("failed to parse profiles: {}", e),
//             )
//         })?;

//         let mut binding = serde_yaml::Value::Sequence(Vec::new());
//         let items = profiles
//             .get_mut("items")
//             .unwrap_or(&mut binding)
//             .as_sequence_mut()
//             .ok_or_else(|| {
//                 std::io::Error::new(
//                     std::io::ErrorKind::Other,
//                     "items is not a sequence in profiles",
//                 )
//             })?;
//         items
//             .iter_mut()
//             .filter_map(|item| item.as_mapping_mut())
//             .for_each(|item| {
//                 let extra = item.get("extra");
//                 // modify the extra field to be a mapping
//                 if let Some(extra) = extra
//                     && extra.is_mapping()
//                     && serde_yaml::from_value::<crate::config::profile::item::SubscriptionInfo>(
//                         extra.clone(),
//                     )
//                     .is_err()
//                 {
//                     println!(
//                         "detected extra in item {:?} should be discarded:\n{:?}",
//                         item.get("uid").unwrap(),
//                         extra
//                     );
//                     let extra = extra.as_mapping().unwrap();
//                     let extra = extra.values().next().unwrap().clone();
//                     item.insert("extra".into(), extra);
//                 }

//                 // migrate urls
//                 let url = item.get("url");
//                 if let Some(url) = url
//                     && url.is_sequence()
//                 {
//                     println!(
//                         "detected url in item {:?} should be discarded:\n{:?}",
//                         item.get("uid").unwrap(),
//                         url
//                     );
//                     let url = url.as_sequence().unwrap();
//                     let url = url.first().unwrap().clone();

//                     item.insert("url".into(), url);
//                 }
//             });

//         let file = std::fs::OpenOptions::new()
//             .write(true)
//             .truncate(true)
//             .open(profiles_path)?;
//         serde_yaml::to_writer(file, &profiles)
//             .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

//         Ok(())
//     }
// }
