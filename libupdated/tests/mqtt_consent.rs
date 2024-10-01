// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

use std::collections::HashMap;
use std::thread;
use std::time::Duration;

use config::{File, FileFormat};
use libupdated::mqtt_consent::mqtt_consent_mock::MqttConsentMock;
use libupdated::progress_reporter::{ProgressMessage, ProgressReporter, UpdateState};
use rumqttc::{AsyncClient, Event, MqttOptions, Packet};
use tokio::time::sleep;

use common::{DummySource, DummyUpdateInfo};
use libupdated::mqtt_consent::{
    CheckForUpdatesMsg, ConsentRequestMsg, ConsentResponseMsg, ConsentResponseState, MqttConsent,
    MqttConsentConfig, MqttReporter, UpdateProgressMsg,
};
use libupdated::traits::consent_handler::ConsentHandler;
use libupdated::update_source::{Update, UpdateInfo, UpdateSource};
use libupdated::update_trigger::UpdateTrigger;

mod common;

pub fn start_broker(port: u16) {
    let config = config::Config::builder()
        .add_source(File::from_str(
            &format!(
                r#"
id = 0
[router]
id = 0
max_connections = 10010
max_outgoing_packet_count = 200
max_segment_size = 104857600
max_segment_count = 10
[v4.1]
name = "v4-1"
listen = "0.0.0.0:{}"
next_connection_delay_ms = 1
[v4.1.connections]
connection_timeout_ms = 60000
max_payload_size = 20480
max_inflight_count = 100
dynamic_filters = true
        "#,
                port
            ),
            FileFormat::Toml,
        ))
        .build()
        .unwrap();
    let mut broker = rumqttd::Broker::new(config.try_deserialize().unwrap());

    thread::spawn(move || {
        broker.start().unwrap();
        panic!("Broker stopped!");
    });
    thread::sleep(Duration::from_secs(1));
}

#[tokio::test]
async fn test_mqtt_consent() {
    let port = rand::random::<u16>() % 1000 + 40000;
    start_broker(port);
    let (codesys, mut eventloop) = MqttConsentMock::new("localhost", port);

    let config = MqttConsentConfig {
        mqtt_host: "localhost".to_string(),
        mqtt_port: port,
        mqtt_consent_response_topic: "consent_response".to_string(),
        mqtt_consent_request_topic: "consent_request".to_string(),
        mqtt_manual_update_topic: "manual_update".to_string(),
        mqtt_update_progress_topic: "update_progress".to_string(),
    };
    let mut mqtt_consent = MqttConsent::new(config).await.unwrap();

    tokio::select! {
        // timeout to stop the test after 5s
        _ = sleep(Duration::from_secs(5)) => {panic!("Timeout reached!")},
        // start the MqttConsent that should be tested. It will ask for consent and the mock will respond with "Accepted"
        _ = async move {
            // test asking for consent
            let update_info = DummyUpdateInfo {
                can_give_consent: true,
                metadata: HashMap::from([
                    ("changelog".to_string(), "important changes".to_string()),
                    ]),
                version: "1.0.0".to_string(),
                needs_consent: true,
            };
            // wait for 0.2s to give the codesys mock (the next select branch) some time to connect to the broker
            sleep(Duration::from_millis(200)).await;
            let result = mqtt_consent.ask_for_consent(&update_info).await;
            assert!(result.unwrap());
        } => {},
        // start the mock that will respond to the consent request if it matches the expected values
        _ = codesys.handle_consent(&mut eventloop, "consent_request", "consent_response", |msg| {
            assert_eq!(msg.update_id, "1.0.0");
            assert_eq!(msg.metadata.get("changelog").unwrap(), "important changes");
            ConsentResponseState::Accepted
        }) => {panic!("Codesys consent mock exited")},
    }
}

#[tokio::test]
async fn test_mqtt_manual_update() {
    let port = rand::random::<u16>() % 1000 + 40000;

    start_broker(port);

    tokio::select! {
        _ = sleep(Duration::from_secs(5)) => {panic!("Timeout reached!")},
        _ = async move {
            let config = MqttConsentConfig {
                mqtt_host: "localhost".to_string(),
                mqtt_port: port,
                mqtt_consent_response_topic: "consent_response".to_string(),
                mqtt_consent_request_topic: "consent_request".to_string(),
                mqtt_manual_update_topic: "manual_update".to_string(),
                mqtt_update_progress_topic: "update_progress".to_string(),
            };

            let mqtt_consent = MqttConsent::new(config).await.unwrap();

            // test waiting for manual update request
            let result = mqtt_consent.manual_update_trigger(()).update_triggered().await;
            assert!(result.is_ok());

        } => {},
        _ = async move {
            // connect to broker
            let options = MqttOptions::new("updated-mqtt-test", "localhost", port);
            let (client, mut eventloop) = AsyncClient::new(options, 10);

            tokio::spawn(async move {
                loop {
                    eventloop.poll().await.unwrap();
                }
            });

            sleep(Duration::from_secs(1)).await;
            // send manual update message
            let msg = CheckForUpdatesMsg {};
            client.publish("manual_update", rumqttc::QoS::ExactlyOnce, false, serde_json::to_vec(&msg).unwrap()).await.unwrap();
            sleep(Duration::from_secs(100)).await;
        } => {},
    }
}

#[tokio::test]
async fn test_mqtt_progress() {
    let port = rand::random::<u16>() % 1000 + 40000;

    start_broker(port);

    let mut source = DummySource {
        pending_update: Some(DummyUpdateInfo {
            needs_consent: false,
            metadata: HashMap::from([("changelog".to_string(), "Changelog".to_string())]),
            version: "1.0.0".to_string(),
            can_give_consent: false,
        }),
    };

    let update_info = source.check_for_updates().await.unwrap();
    let update = update_info.update().unwrap();

    let progress_msg = ProgressMessage {
        state: UpdateState::Downloading,
        cnt_of: (7, 62),
        message: "test progress".to_string(),
    };
    let update_progress_msg = UpdateProgressMsg {
        update_id: update.version().to_string(),
        cnt: progress_msg.cnt_of.0,
        of: progress_msg.cnt_of.1,
        state: progress_msg.state.clone().into(),
    };

    // connect to broker
    let options = MqttOptions::new("updated-mqtt-test", "localhost", port);
    let (client, mut eventloop) = AsyncClient::new(options, 10);
    client
        .subscribe("update_progress", rumqttc::QoS::ExactlyOnce)
        .await
        .unwrap();

    let config = MqttConsentConfig {
        mqtt_host: "localhost".to_string(),
        mqtt_port: port,
        mqtt_consent_response_topic: "consent_response".to_string(),
        mqtt_consent_request_topic: "consent_request".to_string(),
        mqtt_manual_update_topic: "manual_update".to_string(),
        mqtt_update_progress_topic: "update_progress".to_string(),
    };

    tokio::select! {
        _ = sleep(Duration::from_secs(5)) => {panic!("Timeout reached!")},
        _ = async move {
            let mqtt_consent = MqttConsent::new(config).await.unwrap();
            let mut mqtt_progress = MqttReporter::new(&mqtt_consent);

            mqtt_progress.report_retry(&update, &progress_msg, 5, Duration::from_secs(1)).await.unwrap();
            sleep(Duration::from_secs(100)).await;
        } => {},
        _ = async move {
            loop {
                if let Event::Incoming(Packet::Publish(p)) = eventloop.poll().await.unwrap() {
                    let msg = std::str::from_utf8(&p.payload).unwrap();
                    if p.topic.as_str() == "update_progress" {
                        assert_eq!(serde_json::from_str::<UpdateProgressMsg>(msg).unwrap(), update_progress_msg);
                        break;
                    }
                }
            }
        } => {},
    }
}

#[tokio::test]
async fn serialize_json() {
    let msg = ConsentRequestMsg {
        update_id: "1.0.0".to_string(),
        metadata: HashMap::from([("changelog".to_string(), "important changes".to_string())]),
        consent_token: 42,
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert_eq!(
        json,
        r#"{"update_id":"1.0.0","metadata":{"changelog":"important changes"},"consent_token":42}"#
    );

    let msg: ConsentRequestMsg = serde_json::from_str(&json).unwrap();
    assert_eq!(msg.update_id, "1.0.0");
    assert_eq!(msg.metadata.get("changelog").unwrap(), "important changes");
    assert_eq!(msg.consent_token, 42);

    let msg = ConsentResponseMsg {
        consent: ConsentResponseState::Accepted,
        consent_token: 42,
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert_eq!(json, r#"{"consent":"accepted","consent_token":42}"#);

    let msg: ConsentResponseMsg = serde_json::from_str(&json).unwrap();
    assert_eq!(msg.consent, ConsentResponseState::Accepted);
    assert_eq!(msg.consent_token, 42);

    let msg = CheckForUpdatesMsg {};
    let json = serde_json::to_string(&msg).unwrap();
    assert_eq!(json, "{}");

    let msg: CheckForUpdatesMsg = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, CheckForUpdatesMsg {});

    let msg = UpdateProgressMsg {
        update_id: "4".to_string(),
        cnt: 42,
        of: 87,
        state: UpdateState::Downloading,
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert_eq!(
        json,
        r#"{"update_id":"4","cnt":42,"of":87,"state":"downloading"}"#
    );

    let msg: UpdateProgressMsg = serde_json::from_str(&json).unwrap();
    assert_eq!(msg.update_id, "4");
    assert_eq!(msg.cnt, 42);
    assert_eq!(msg.of, 87);
    assert_eq!(msg.state, UpdateState::Downloading);

    let msg = UpdateProgressMsg {
        update_id: "4".to_string(),
        cnt: 42,
        of: 87,
        state: UpdateState::Failed("error".to_string()),
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert_eq!(
        json,
        r#"{"update_id":"4","cnt":42,"of":87,"state":{"failed":"error"}}"#
    );

    let msg: UpdateProgressMsg = serde_json::from_str(&json).unwrap();
    assert_eq!(msg.update_id, "4");
    assert_eq!(msg.cnt, 42);
    assert_eq!(msg.of, 87);
    assert_eq!(msg.state, UpdateState::Failed("error".to_string()));
}
