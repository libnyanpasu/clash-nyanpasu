use std::sync::Arc;

use jsonrpsee::RpcModule;
use jsonrpsee_types::ErrorObjectOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json, value::RawValue};
use tokio::sync::{Mutex, broadcast, mpsc};

const OPENRPC_DOCUMENT: &str = include_str!("../openrpc.json");
const MAX_WIRE_MESSAGE_SIZE: usize = 1024 * 1024;
const SUBSCRIPTION_BUFFER_SIZE: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileActivation {
    pub profile_id: String,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClashEvent {
    pub sequence: u64,
    pub update: String,
}

#[derive(Clone)]
pub struct FakeApplication {
    profiles: Arc<Mutex<Vec<Profile>>>,
    clash_events: broadcast::Sender<ClashEvent>,
    closed_subscriptions: broadcast::Sender<()>,
}

impl Default for FakeApplication {
    fn default() -> Self {
        let (clash_events, _) = broadcast::channel(16);
        let (closed_subscriptions, _) = broadcast::channel(4);

        Self {
            profiles: Arc::new(Mutex::new(vec![
                Profile {
                    id: "profile-1".into(),
                    name: "Primary".into(),
                    active: true,
                },
                Profile {
                    id: "profile-2".into(),
                    name: "Backup".into(),
                    active: false,
                },
            ])),
            clash_events,
            closed_subscriptions,
        }
    }
}

impl FakeApplication {
    async fn list_profiles(&self) -> Vec<Profile> {
        self.profiles.lock().await.clone()
    }

    async fn activate_profile(&self, profile_id: &str) -> Result<ProfileActivation, ()> {
        let mut profiles = self.profiles.lock().await;
        if !profiles.iter().any(|profile| profile.id == profile_id) {
            return Err(());
        }

        for profile in profiles.iter_mut() {
            profile.active = profile.id == profile_id;
        }

        Ok(ProfileActivation {
            profile_id: profile_id.to_owned(),
            active: true,
        })
    }

    #[cfg(test)]
    fn publish_clash_event(&self, event: ClashEvent) {
        let _ = self.clash_events.send(event);
    }

    #[cfg(test)]
    fn subscribe_closed(&self) -> broadcast::Receiver<()> {
        self.closed_subscriptions.subscribe()
    }
}

struct SubscriptionClosedSignal(broadcast::Sender<()>);

impl Drop for SubscriptionClosedSignal {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

pub fn application_rpc(app: FakeApplication) -> RpcModule<FakeApplication> {
    let mut module = RpcModule::new(app);

    module
        .register_async_method("nyanpasu.v1.profiles.list", |_, app, _| async move {
            app.list_profiles().await
        })
        .expect("profile list method name is unique");

    module
        .register_async_method(
            "nyanpasu.v1.profiles.activate",
            |params, app, _| async move {
                let profile_id = params.one::<String>().map_err(|error| {
                    ErrorObjectOwned::owned(-32602, "Invalid params", Some(error.to_string()))
                })?;

                app.activate_profile(&profile_id).await.map_err(|()| {
                    ErrorObjectOwned::owned(
                        -32004,
                        "Profile not found",
                        Some(json!({ "profileId": profile_id })),
                    )
                })
            },
        )
        .expect("profile activation method name is unique");

    module
        .register_method("rpc.discover", |_, _, _| {
            serde_json::from_str::<Value>(OPENRPC_DOCUMENT)
                .expect("the checked-in OpenRPC document is valid JSON")
        })
        .expect("OpenRPC discovery method name is unique");

    module
        .register_subscription(
            "nyanpasu.v1.clash.subscribe",
            "nyanpasu.v1.clash.event",
            "nyanpasu.v1.clash.unsubscribe",
            |_, pending, app, _| async move {
                let mut events = app.clash_events.subscribe();
                let _closed_signal = SubscriptionClosedSignal(app.closed_subscriptions.clone());
                let sink = pending.accept().await?;

                loop {
                    tokio::select! {
                        _ = sink.closed() => break,
                        event = events.recv() => match event {
                            Ok(event) => {
                                let message = jsonrpsee::core::to_json_raw_value(&event)
                                    .expect("probe event serializes to JSON");
                                tokio::select! {
                                    _ = sink.closed() => break,
                                    result = sink.send(message) => {
                                        if result.is_err() {
                                            break;
                                        }
                                    }
                                }
                            }
                            Err(broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(broadcast::error::RecvError::Closed) => break,
                        }
                    }
                }

                Ok::<(), jsonrpsee::core::SubscriptionError>(())
            },
        )
        .expect("Clash subscription methods are unique");

    module
}

/// Dispatches a JSON-RPC request without opening a transport listener.
/// The returned receiver carries subscription notifications, if any.
pub async fn dispatch_json_rpc_stream(
    module: &RpcModule<FakeApplication>,
    request: &str,
) -> Result<(String, mpsc::Receiver<Box<RawValue>>), String> {
    if request.len() > MAX_WIRE_MESSAGE_SIZE {
        return Err("JSON-RPC request exceeds the 1 MiB bridge limit".into());
    }

    let (response, notifications) = module
        .raw_json_request(request, SUBSCRIPTION_BUFFER_SIZE)
        .await
        .map_err(|error| format!("JSON-RPC dispatch failed: {error}"))?;
    let response = response.get();
    if response.len() > MAX_WIRE_MESSAGE_SIZE {
        return Err("JSON-RPC response exceeds the 1 MiB bridge limit".into());
    }
    Ok((response.to_owned(), notifications))
}

/// Adapts jsonrpsee's notification stream to the string payload expected by a
/// Tauri Channel. A Tokio channel stands in for the Tauri Channel in this probe.
pub async fn forward_notifications(
    mut notifications: mpsc::Receiver<Box<RawValue>>,
    channel: mpsc::Sender<String>,
) -> Result<(), String> {
    while let Some(notification) = notifications.recv().await {
        let notification = notification.get();
        if notification.len() > MAX_WIRE_MESSAGE_SIZE {
            return Err("JSON-RPC notification exceeds the 1 MiB bridge limit".into());
        }

        channel
            .send(notification.to_owned())
            .await
            .map_err(|_| "notification channel closed".to_owned())?;
    }

    Ok(())
}

/// Mirrors a Tauri `rpc_subscribe` command: return the JSON-RPC subscription
/// response immediately and forward subsequent messages through a Channel-like sink.
pub async fn rpc_subscribe(
    module: &RpcModule<FakeApplication>,
    request: &str,
    channel: mpsc::Sender<String>,
) -> Result<String, String> {
    let (response, notifications) = dispatch_json_rpc_stream(module, request).await?;
    tokio::spawn(async move {
        let _ = forward_notifications(notifications, channel).await;
    });
    Ok(response)
}

/// Dispatches one request and drops any subscription stream the caller ignores.
pub async fn dispatch_json_rpc(
    module: &RpcModule<FakeApplication>,
    request: &str,
) -> Result<String, String> {
    let (response, notifications) = dispatch_json_rpc_stream(module, request).await?;
    drop(notifications);
    Ok(response)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::time::Duration;

    use serde_json::{Value, json};
    use tokio::{sync::mpsc, time::timeout};

    use super::{
        ClashEvent, FakeApplication, MAX_WIRE_MESSAGE_SIZE, OPENRPC_DOCUMENT, application_rpc,
        dispatch_json_rpc, rpc_subscribe,
    };

    #[test]
    fn registered_methods_match_the_openrpc_document() {
        let module = application_rpc(FakeApplication::default());
        let document: Value = serde_json::from_str(OPENRPC_DOCUMENT).unwrap();
        let documented = document["methods"]
            .as_array()
            .unwrap()
            .iter()
            .map(|method| method["name"].as_str().unwrap().to_owned())
            .collect::<BTreeSet<_>>();
        let registered = module
            .method_names()
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();

        assert_eq!(registered, documented);
    }

    #[tokio::test]
    async fn jsonrpc_list_and_activation_use_the_documented_wire_shapes() {
        let module = application_rpc(FakeApplication::default());
        let list = dispatch_json_rpc(
            &module,
            r#"{"jsonrpc":"2.0","id":1,"method":"nyanpasu.v1.profiles.list"}"#,
        )
        .await
        .unwrap();
        let list: Value = serde_json::from_str(&list).unwrap();
        assert_eq!(list["result"][0]["id"], "profile-1");

        let activation = dispatch_json_rpc(
            &module,
            r#"{"jsonrpc":"2.0","id":2,"method":"nyanpasu.v1.profiles.activate","params":["profile-2"]}"#,
        )
        .await
        .unwrap();
        let activation: Value = serde_json::from_str(&activation).unwrap();
        assert_eq!(
            activation,
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "result": { "profileId": "profile-2", "active": true }
            })
        );
    }

    #[tokio::test]
    async fn application_errors_are_jsonrpc_errors_with_structured_data() {
        let module = application_rpc(FakeApplication::default());
        let response = dispatch_json_rpc(
            &module,
            r#"{"jsonrpc":"2.0","id":3,"method":"nyanpasu.v1.profiles.activate","params":["missing"]}"#,
        )
        .await
        .unwrap();
        let response: Value = serde_json::from_str(&response).unwrap();

        assert_eq!(response["error"]["code"], -32004);
        assert_eq!(response["error"]["data"]["profileId"], "missing");
    }

    #[tokio::test]
    async fn rpc_discover_returns_the_checked_in_openrpc_document() {
        let module = application_rpc(FakeApplication::default());
        let response = dispatch_json_rpc(
            &module,
            r#"{"jsonrpc":"2.0","id":4,"method":"rpc.discover"}"#,
        )
        .await
        .unwrap();
        let response: Value = serde_json::from_str(&response).unwrap();
        let expected: Value = serde_json::from_str(OPENRPC_DOCUMENT).unwrap();

        assert_eq!(response["result"], expected);
    }

    #[tokio::test]
    async fn bridge_rejects_requests_over_one_mebibyte() {
        let module = application_rpc(FakeApplication::default());
        let request = " ".repeat(MAX_WIRE_MESSAGE_SIZE + 1);

        assert!(
            dispatch_json_rpc(&module, &request)
                .await
                .unwrap_err()
                .contains("1 MiB bridge limit")
        );
    }

    #[tokio::test]
    async fn subscription_notifications_cross_the_channel_adapter_and_unsubscribe_closes_them() {
        let app = FakeApplication::default();
        let mut closed = app.subscribe_closed();
        let module = application_rpc(app.clone());
        let (channel, mut frontend) = mpsc::channel(4);
        let response = rpc_subscribe(
            &module,
            r#"{"jsonrpc":"2.0","id":10,"method":"nyanpasu.v1.clash.subscribe","params":[]}"#,
            channel,
        )
        .await
        .unwrap();
        let response: Value = serde_json::from_str(&response).unwrap();
        let subscription_id = response["result"].clone();

        app.publish_clash_event(ClashEvent {
            sequence: 1,
            update: "log_appended".into(),
        });

        let notification = timeout(Duration::from_secs(1), frontend.recv())
            .await
            .expect("the event should arrive through the channel adapter")
            .expect("the channel adapter should still be open");
        let notification: Value = serde_json::from_str(&notification).unwrap();
        assert_eq!(notification["method"], "nyanpasu.v1.clash.event");
        assert_eq!(notification["params"]["subscription"], subscription_id);
        assert_eq!(notification["params"]["result"]["sequence"], 1);
        assert_eq!(notification["params"]["result"]["update"], "log_appended");

        let unsubscribe = dispatch_json_rpc(
            &module,
            &json!({
                "jsonrpc": "2.0",
                "id": 11,
                "method": "nyanpasu.v1.clash.unsubscribe",
                "params": [subscription_id],
            })
            .to_string(),
        )
        .await
        .unwrap();
        let unsubscribe: Value = serde_json::from_str(&unsubscribe).unwrap();
        assert_eq!(unsubscribe["result"], true);

        timeout(Duration::from_secs(1), closed.recv())
            .await
            .expect("unsubscribe should stop the backend subscription task")
            .expect("the subscription close signal should be delivered");
        assert!(
            timeout(Duration::from_secs(1), frontend.recv())
                .await
                .expect("the channel forwarding task should stop after unsubscribe")
                .is_none()
        );
    }
}
