use std::{sync::Arc, time::Duration};

use mockall::Sequence;
use nyanpasu_geodata::IpIndex;
use tokio::sync::watch;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::{
    GeoIndexArgs, GeoIndexClient, MockCountryIndexSource,
    actor::Message,
    fixtures::geoip_dat,
    ports::{GeoIndexError, GeodataMode, IndexKey, Loaded},
};

fn test_index() -> Arc<IpIndex> {
    Arc::new(IpIndex::from_geoip_dat(&geoip_dat(&[("US", &["8.0.0.0/8"])])).unwrap())
}

fn key(byte: u8) -> IndexKey {
    IndexKey {
        mode: GeodataMode::Dat,
        sha256: [byte; 32],
    }
}

/// A source whose watch succeeds; the core never answers, so tests report the mode themselves.
fn source() -> MockCountryIndexSource {
    let mut source = MockCountryIndexSource::new();
    source.expect_watch().returning(|_| Ok(Box::new(())));
    source
}

async fn spawn(source: MockCountryIndexSource) -> GeoIndexClient {
    let (core, _service) = crate::client::tests::test_v2_clients();
    GeoIndexClient::spawn(
        GeoIndexArgs {
            source: Arc::new(source),
            core,
        },
        CancellationToken::new(),
        &TaskTracker::new(),
    )
    .await
    .unwrap()
}

async fn next(published: &mut watch::Receiver<Option<Arc<IpIndex>>>) -> Option<Arc<IpIndex>> {
    tokio::time::timeout(Duration::from_secs(5), published.changed())
        .await
        .expect("an index is published")
        .unwrap();
    published.borrow_and_update().clone()
}

#[tokio::test]
async fn a_reported_mode_loads_and_publishes_its_index() {
    let index = test_index();
    let mut source = source();
    let published_index = index.clone();
    source
        .expect_load()
        .withf(|mode, current| *mode == GeodataMode::Dat && current.is_none())
        .times(1)
        .returning(move |_, _| {
            Ok(Loaded::Index {
                key: key(1),
                index: published_index.clone(),
            })
        });
    let client = spawn(source).await;
    let mut published = client.subscribe();

    client.send(Message::Mode(GeodataMode::Dat));

    assert!(Arc::ptr_eq(&next(&mut published).await.unwrap(), &index));
}

#[tokio::test]
async fn a_written_database_waits_for_the_core_s_mode() {
    let mut source = source();
    // Only the mode's message loads; the earlier change has no database to read yet.
    source
        .expect_load()
        .withf(|mode, _| *mode == GeodataMode::Mmdb)
        .times(1)
        .returning(|_, _| Ok(Loaded::Missing));
    let client = spawn(source).await;
    let mut published = client.subscribe();

    client.send(Message::FilesChanged);
    client.send(Message::Mode(GeodataMode::Mmdb));

    assert!(next(&mut published).await.is_none());
}

#[tokio::test]
async fn a_failed_reload_keeps_the_index_and_its_key() {
    let index = test_index();
    let mut sequence = Sequence::new();
    let mut source = source();
    let first = index.clone();
    source
        .expect_load()
        .times(1)
        .in_sequence(&mut sequence)
        .returning(move |_, _| {
            Ok(Loaded::Index {
                key: key(1),
                index: first.clone(),
            })
        });
    source
        .expect_load()
        .withf(|_, current| *current == Some(key(1)))
        .times(1)
        .in_sequence(&mut sequence)
        .returning(|_, _| {
            Err(GeoIndexError::ListHome {
                source: std::io::Error::other("denied"),
            })
        });
    // Still handed the first key: the failure did not forget what is published.
    source
        .expect_load()
        .withf(|_, current| *current == Some(key(1)))
        .times(1)
        .in_sequence(&mut sequence)
        .returning(|_, _| Ok(Loaded::Missing));
    let client = spawn(source).await;
    let mut published = client.subscribe();

    client.send(Message::Mode(GeodataMode::Dat));
    assert!(Arc::ptr_eq(&next(&mut published).await.unwrap(), &index));
    client.send(Message::FilesChanged);
    client.send(Message::FilesChanged);

    assert!(next(&mut published).await.is_none());
}
