// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

use std::{collections::HashMap, error::Error, time::Duration};

use rumqttc::{AsyncClient, ClientError, Event, EventLoop, MqttOptions, Packet};
use serde::{Deserialize, Serialize};
use tokio::{
    sync::watch::{self, Receiver, Sender},
    task::JoinHandle,
    time::{Instant, sleep},
};
use tracing::{error, info};

use crate::progress_reporter::ProgressReporter;
use crate::{
    LOG,
    consent_handler::{ConsentHandler, ConsentHandlerError},
    progress_reporter::UpdateState,
    update_source::UpdateInfo,
    update_trigger::{UpdateTrigger, UpdateTriggerError},
    update_workflow::UpdateError,
};

// Make errors that occur during the consent process compatible with the UpdateError type

impl From<ClientError> for UpdateError {
    fn from(e: ClientError) -> Self {
        UpdateError::ConsentHandlerError(ConsentHandlerError::Error(format!("mqtt error: {}", e)))
    }
}

impl From<watch::error::RecvError> for UpdateError {
    fn from(e: watch::error::RecvError) -> Self {
        UpdateError::ConsentHandlerError(ConsentHandlerError::Error(format!(
            "tokio recv error: {}",
            e
        )))
    }
}

impl From<serde_json::Error> for UpdateError {
    fn from(e: serde_json::Error) -> Self {
        UpdateError::ConsentHandlerError(ConsentHandlerError::Error(format!(
            "serde json error: {}",
            e
        )))
    }
}

/// Mqtt message to request consent for an update
#[derive(Debug, Deserialize, Serialize)]
pub struct ConsentRequestMsg {
    /// id of the update
    pub update_id: String,
    /// metadata of the new version, e.g. version, changelog, …
    pub metadata: HashMap<String, String>,
    /// unique id for the consent request. The response must contain the same id.
    pub consent_token: u32,
}

#[derive(Debug, Deserialize, Serialize, Copy, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ConsentResponseState {
    /// Consent to install the update is given
    Accepted,
    /// Consent to install the update is declined
    Declined,
}

/// Mqtt message to respond to a consent request
#[derive(Debug, Deserialize, Serialize, Copy, Clone)]
pub struct ConsentResponseMsg {
    /// the response, how to proceed with the update: accept or decline
    pub consent: ConsentResponseState,
    /// the unique id of the consent request
    pub consent_token: u32,
}

impl Default for ConsentResponseMsg {
    fn default() -> Self {
        Self {
            consent: ConsentResponseState::Declined,
            consent_token: 0,
        }
    }
}

/// Mqtt message to trigger a manual update check
#[derive(Debug, Serialize, Deserialize, Copy, Clone, PartialEq, Eq, Default)]
pub struct CheckForUpdatesMsg {}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct UpdateProgressMsg {
    pub update_id: String,
    pub cnt: u32,
    pub of: u32,
    pub state: UpdateState,
}

/// Mqtt-based implementation of the ConsentHandler trait. If consent is requested, a message is sent to the configured mqtt topic.
/// It then waits for a response and returns the user response.
pub struct MqttConsent {
    /// Configuration for the mqtt connection
    config: MqttConsentConfig,
    /// The mqtt client
    client: AsyncClient,
    /// Channel to receive consent responses from the event loop
    consent_response: Receiver<ConsentResponseMsg>,
    /// Channel to receive manual update requests from the event loop
    manual_update: Receiver<CheckForUpdatesMsg>,
    /// Handle to the event loop task
    evt_loop_handle: JoinHandle<()>,
}

/// Update trigger that starts an update after a *CheckForUpdates* message is received.
pub struct MqttUpdateTrigger<T: Clone> {
    manual_update: Receiver<CheckForUpdatesMsg>,
    trigger_value: T,
}

impl<T> UpdateTrigger<T> for MqttUpdateTrigger<T>
where
    T: Clone,
{
    async fn update_triggered(&mut self) -> Result<T, UpdateTriggerError> {
        self.manual_update.changed().await.map_err(|e| {
            UpdateTriggerError::TriggerError(format!("Manual update channel closed: {e}"))
        })?;
        Ok(self.trigger_value.clone())
    }
}

pub struct MqttReporter {
    client: AsyncClient,
    topic: String,
}

impl MqttReporter {
    pub fn new(mqtt_consent: &MqttConsent) -> Self {
        Self {
            client: mqtt_consent.client.clone(),
            topic: mqtt_consent.config.mqtt_update_progress_topic.clone(),
        }
    }
}

impl ProgressReporter for MqttReporter {
    async fn report(
        &mut self,
        update: &impl crate::update_source::Update,
        message: &crate::progress_reporter::ProgressMessage,
    ) -> Result<(), UpdateError> {
        let msg = serde_json::to_string(&UpdateProgressMsg {
            update_id: update.version().to_string(),
            cnt: message.cnt_of.0,
            of: message.cnt_of.1,
            state: message.state.clone(),
        })?;
        self.client
            .publish(&self.topic, rumqttc::QoS::ExactlyOnce, false, msg)
            .await?;
        Ok(())
    }
}

/// Configuration for the mqtt consent handler
#[derive(Clone)]
pub struct MqttConsentConfig {
    /// hostname of the mqtt broker
    pub mqtt_host: String,
    /// port of the mqtt broker
    pub mqtt_port: u16,
    /// topic to receive consent responses on
    pub mqtt_consent_response_topic: String,
    /// topic to send consent requests to
    pub mqtt_consent_request_topic: String,
    /// topic to receive manual update requests on
    pub mqtt_manual_update_topic: String,
    /// topic to send progress messages to
    pub mqtt_update_progress_topic: String,
}

impl Drop for MqttConsent {
    fn drop(&mut self) {
        // disconnect the mqtt client and abort the event loop task
        match self.client.try_disconnect() {
            Ok(_) => {}
            Err(e) => {
                error!(target: LOG, "Error disconnecting from MQTT: {:?}", e);
            }
        }
        self.evt_loop_handle.abort();
    }
}

impl MqttConsent {
    /// Create a new MqttConsent instance
    ///
    /// Arguments:
    /// - config: Configuration for the mqtt connection
    ///
    /// Returns:
    /// - Result<MqttConsent, UpdateError>: The created MqttConsent instance or an error if the connection to the mqtt broker could not be established
    pub async fn new(config: MqttConsentConfig) -> Result<Self, UpdateError> {
        let mut options =
            MqttOptions::new("updated-mqtt-consent", &config.mqtt_host, config.mqtt_port);
        // set keep alive, as we expect very infrequent messages but want to keep the connection
        options.set_keep_alive(Duration::from_secs(5));
        let (client, mut eventloop) = AsyncClient::new(options, 10);

        // subscribe to the consent response and manual update topics. Use highest QoS level to ensure a stable consent process.
        // However, the GUI might still downgrade the QoS level.
        client
            .subscribe(
                &config.mqtt_consent_response_topic,
                rumqttc::QoS::ExactlyOnce,
            )
            .await?;
        client
            .subscribe(&config.mqtt_manual_update_topic, rumqttc::QoS::ExactlyOnce)
            .await?;

        // Prepare channels to relay messages from the event loop task to the main task
        let (consent_response_tx, consent_response) = watch::channel(ConsentResponseMsg::default());
        let (manual_update_tx, manual_update) = watch::channel(CheckForUpdatesMsg::default());

        // spawn the event loop task, errors are logged and the event handler is restarted
        let client_clone = client.clone();
        let config_clone = config.clone();
        let evt_loop_handle = tokio::spawn(async move {
            let mut last_log_time = Instant::now();
            let mut disconnected = false;
            loop {
                match handle_events(
                    &config_clone,
                    &consent_response_tx,
                    &manual_update_tx,
                    &mut eventloop,
                )
                .await
                {
                    Ok(_) => {
                        if disconnected {
                            disconnected = false;

                            error!(target: LOG, "Disconnect detected; resubscribing!");
                            client_clone
                                .try_subscribe(
                                    &config_clone.mqtt_consent_response_topic,
                                    rumqttc::QoS::ExactlyOnce,
                                )
                                .unwrap_or_else(|err| {
                                    error!(target: LOG, "Error while resubscribing: {err}");
                                });
                            client_clone
                                .try_subscribe(
                                    &config_clone.mqtt_manual_update_topic,
                                    rumqttc::QoS::ExactlyOnce,
                                )
                                .unwrap_or_else(|err| {
                                    error!(target: LOG, "Error while resubscribing: {err}");
                                });
                        }

                        continue;
                    }
                    Err(e) => {
                        // rate limit error messages, if, for example, the mqtt broker is down
                        if Instant::now() > last_log_time + Duration::from_secs(10) {
                            error!(target: LOG, "Error in mqtt event loop: {:?}", e);
                            last_log_time = Instant::now();
                        }

                        disconnected = true;
                    }
                }
                // avoid high CPU load if the broker is down
                sleep(Duration::from_millis(100)).await;
            }
        });

        let mqtt_consent = Self {
            config,
            client,
            consent_response,
            manual_update,
            evt_loop_handle,
        };

        // clear any old consent request if it is still retained
        mqtt_consent.clear_consent_request()?;

        Ok(mqtt_consent)
    }

    /// Create an update trigger that waits for a *CheckForUpdates* message.
    pub fn manual_update_trigger<T: Clone>(&self, trigger_value: T) -> MqttUpdateTrigger<T> {
        MqttUpdateTrigger {
            manual_update: self.manual_update.clone(),
            trigger_value,
        }
    }

    /// Sends a consent request message for a given UpdateInfo and returns the consent token
    async fn send_consent_request(
        &self,
        update_info: &impl UpdateInfo,
    ) -> Result<u32, UpdateError> {
        let consent_token = rand::random();
        let msg = serde_json::to_string(&ConsentRequestMsg {
            metadata: update_info.metadata(),
            update_id: update_info.version(),
            consent_token,
        })?;
        self.client
            .publish(
                &self.config.mqtt_consent_request_topic,
                rumqttc::QoS::ExactlyOnce,
                true,
                msg.clone(),
            )
            .await?;
        info!(
            LOG,
            "Sent consent request for update {} (msg: {:?}))",
            update_info.version(),
            msg
        );
        Ok(consent_token)
    }

    /// clears the retained consent request on the mqtt broker
    fn clear_consent_request(&self) -> Result<(), UpdateError> {
        self.client.try_publish(
            &self.config.mqtt_consent_request_topic,
            rumqttc::QoS::ExactlyOnce,
            true,
            "", // an empty message clears the retained message
        )?;
        info!(LOG, "Cleared consent request on mqtt broker",);
        Ok(())
    }
}

/// The event handler that handles incoming mqtt messages and relays them to the respective channels
/// Arguments:
/// - config: Configuration for the mqtt connection
/// - consent_response: Channel to relay consent responses to
/// - manual_update: Channel to relay manual update requests to
/// - eventloop: The event loop handle to poll for incoming messages
async fn handle_events(
    config: &MqttConsentConfig,
    consent_response: &Sender<ConsentResponseMsg>,
    manual_update: &Sender<CheckForUpdatesMsg>,
    eventloop: &mut EventLoop,
) -> Result<(), Box<dyn Error>> {
    loop {
        let notification = eventloop.poll().await?;
        match notification {
            Event::Incoming(Packet::Publish(p)) => {
                let msg = std::str::from_utf8(&p.payload)?;
                if p.topic.as_str() == config.mqtt_consent_response_topic {
                    consent_response.send(serde_json::from_str(msg)?)?;
                }

                if p.topic.as_str() == config.mqtt_manual_update_topic {
                    manual_update.send(serde_json::from_str(msg)?)?;
                }
            }
            Event::Incoming(Packet::ConnAck(_)) => {
                // if the notification is not a packet that was handled,
                // but a ConnAck that the connection to the broker was
                // (re-)established, give control back to the caller to
                // handle resubscribing to the topics etc.
                return Ok(());
            }
            _ => {}
        }
    }
}

impl ConsentHandler for MqttConsent {
    async fn ask_for_consent(
        &mut self,
        update_info: &impl UpdateInfo,
    ) -> Result<bool, UpdateError> {
        let consent_token = self.send_consent_request(update_info).await?;

        loop {
            self.consent_response.changed().await?;
            let response = self.consent_response.borrow();
            info!(LOG, "Received consent response: {:?}", response);

            // check the received consent_token
            if response.consent_token != consent_token {
                info!(
                    LOG,
                    "Ignoring response with wrong token ({} != {})",
                    response.consent_token,
                    consent_token
                );
                continue;
            }

            // the token matches, clear the consent message
            self.clear_consent_request()?;

            // check and return the consent response
            match response.consent {
                ConsentResponseState::Accepted => {
                    return Ok(true);
                }
                ConsentResponseState::Declined => {
                    return Ok(false);
                }
            }
        }
    }
}

pub mod mqtt_consent_mock {
    use rumqttc::{AsyncClient, Event, EventLoop, MqttOptions, Packet};

    use crate::mqtt_consent::{ConsentRequestMsg, ConsentResponseMsg};

    use super::{CheckForUpdatesMsg, ConsentResponseState};

    pub struct MqttConsentMock {
        client: AsyncClient,
    }

    impl MqttConsentMock {
        pub fn new(host: &str, port: u16) -> (Self, EventLoop) {
            let options = MqttOptions::new("updated-mqtt-test", host, port);
            let (client, eventloop) = AsyncClient::new(options, 10);
            (Self { client }, eventloop)
        }

        pub async fn trigger_manual_update(&self, topic: &str) {
            self.client
                .publish(
                    topic,
                    rumqttc::QoS::ExactlyOnce,
                    false,
                    serde_json::to_vec(&CheckForUpdatesMsg {}).unwrap(),
                )
                .await
                .unwrap();
        }

        pub async fn handle_consent(
            &self,
            eventloop: &mut EventLoop,
            request_topic: &str,
            response_topic: &str,
            expect_msg: impl Fn(&ConsentRequestMsg) -> ConsentResponseState,
        ) {
            // connect to broker
            self.client
                .subscribe(request_topic, rumqttc::QoS::ExactlyOnce)
                .await
                .unwrap();

            loop {
                if let Event::Incoming(Packet::Publish(p)) = eventloop.poll().await.unwrap() {
                    if p.topic.as_str() == request_topic && !p.payload.is_empty() {
                        // receive consent request message
                        let msg: ConsentRequestMsg = serde_json::from_slice(&p.payload).unwrap();
                        let result = expect_msg(&msg);

                        // send consent response message
                        let msg = ConsentResponseMsg {
                            consent: result,
                            consent_token: msg.consent_token,
                        };
                        self.client
                            .try_publish(
                                response_topic,
                                rumqttc::QoS::ExactlyOnce,
                                false,
                                serde_json::to_vec(&msg).unwrap(),
                            )
                            .unwrap();
                    }
                }
            }
        }
    }
}
