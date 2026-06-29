//! New-format wire round-trip coverage for the clean profile model.
use crate::profile::*;

fn parse(yaml: &str) -> Profiles {
    serde_yaml_ng::from_str::<Profiles>(yaml)
        .unwrap_or_else(|e| panic!("profiles must deserialize, got: {e}"))
}

#[test]
fn clean_document_round_trips() {
    let yaml = r#"current: all-subscriptions
global_transforms:
  - global-fix
valid:
  - dns
items:
  - uid: subscription-a
    name: Subscription A
    type: config
    config:
      type: file
      source:
        type: remote
        file: subscription-a.yaml
        updated_at: 1720954186
        url: https://example.com/a.yaml
        option:
          with_proxy: false
          self_proxy: true
          update_interval_minutes: 120
      transforms:
        - normalize-nodes
  - uid: subscription-b
    name: Subscription B
    type: config
    config:
      type: file
      source:
        type: remote
        file: subscription-b.yaml
        url: https://example.com/b.yaml
  - uid: all-subscriptions
    name: All Subscriptions
    type: config
    config:
      type: composition
      base: subscription-a
      extend_proxies_from:
        - subscription-b
      transforms:
        - finalize-all
  - uid: normalize-nodes
    name: Normalize Nodes
    type: transform
    transform:
      type: script
      runtime: javascript
      source:
        type: local
        binding:
          type: external
          file: normalize-nodes.js
          target: /home/user/clash-scripts/normalize.js
          mode: symlink
  - uid: finalize-all
    name: Finalize All
    type: transform
    transform:
      type: overlay
      source:
        type: local
        binding:
          type: managed
          file: finalize-all.yaml
  - uid: global-fix
    name: Global Fix
    type: transform
    transform:
      type: overlay
      source:
        type: local
        binding:
          type: managed
          file: global-fix.yaml
"#;
    let profiles = parse(yaml);
    assert_eq!(
        profiles.current,
        Some(ProfileId("all-subscriptions".into()))
    );
    assert_eq!(
        profiles.global_transforms,
        vec![ProfileId("global-fix".into())]
    );
    assert_eq!(profiles.items.len(), 6);

    let a = profiles
        .get_item(&ProfileId("subscription-a".into()))
        .unwrap();
    assert!(a.definition.is_direct_file_config());

    let comp = profiles
        .get_item(&ProfileId("all-subscriptions".into()))
        .unwrap();
    match &comp.definition {
        ProfileDefinition::Config {
            config: ConfigDefinition::Composition(c),
        } => {
            assert_eq!(c.base, Some(ProfileId("subscription-a".into())));
            assert_eq!(
                c.extend_proxies_from,
                vec![ProfileId("subscription-b".into())]
            );
        }
        other => panic!("expected composition, got {other:?}"),
    }

    profiles.validate().expect("document must validate");

    let dumped = serde_yaml_ng::to_string(&profiles).expect("serialize");
    let reparsed = parse(&dumped);
    assert_eq!(reparsed.current, profiles.current);
    assert_eq!(reparsed.items.len(), profiles.items.len());
    reparsed.validate().expect("reparsed must validate");
}
