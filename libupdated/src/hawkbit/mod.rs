// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

mod hawkbit_builder;
mod hawkbit_progress;
mod hawkbit_update;

use crate::LOG;
use crate::util::download::DownloadClient;
use async_trait::async_trait;
use regex::Regex;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info};

pub use hawkbit::ddi::ClientAuthorization;
use hawkbit::ddi::{CancelAction, Client, ConfigRequest, Execution, Finished};

use crate::update_source::{StreamError, UpdateSourceError};
use crate::update_workflow::UpdateError;

pub use self::hawkbit_builder::HawkbitBuilder;
pub use self::hawkbit_progress::HawkbitProgressReporter;
pub use self::hawkbit_update::{HawkbitUpdate, HawkbitUpdateInfo};

/// The hawkbit crate uses its own error type, which we make compatible with our UpdateError type.
impl From<hawkbit::ddi::Error> for UpdateError {
    fn from(e: hawkbit::ddi::Error) -> Self {
        UpdateError::UpdateSourceError(UpdateSourceError::ConnectionError(format!(
            "ddi error: {}",
            e
        )))
    }
}

/// The hawkbit crate uses its own error type, which we make compatible with our UpdateError type.
impl From<hawkbit::ddi::Error> for StreamError {
    fn from(e: hawkbit::ddi::Error) -> Self {
        StreamError::ReceiveError(format!("ddi error: {}", e))
    }
}

/// Hawkbit uses the hawkbit DDI API (`hawkbit` crate) to implement the UpdateSource trait.
#[derive(Debug)]
pub struct Hawkbit {
    /// The client struct that serves as an entry point to poll the hawkbit server.
    client: Client,
    /// The polling interval suggested by the server.
    polling_interval: std::time::Duration,
    /// Options to configure the behavior of the Hawkbit update source.
    /// This includes whether to handle config data requests and cancel actions from the hawkbit server.
    options: HawkbitOptions,
}

#[async_trait]
pub trait AttributesFn: core::fmt::Debug + Sync + Send {
    async fn run(&self) -> Option<HashMap<String, String>>;
}

#[derive(Debug, Clone)]
pub enum Attributes {
    Script(String),
    Function(Arc<dyn AttributesFn>),
    Default,
}

/// Options to configure the behavior of the Hawkbit update source.
#[derive(Clone, Debug)]
pub struct HawkbitOptions {
    /// Acknowledge cancel actions from the server.
    pub handle_cancel: bool,
    /// Acknowledge config data requests from the server.
    pub handle_config: bool,
    /// The default polling interval to use at the start or if the server does not suggest one.
    pub default_polling_interval: Duration,
    /// The certificate to use for the connection to the Hawkbit server.
    pub server_cert: Option<String>,
    /// The certificate to use to authenticate the client.
    pub client_cert: Option<String>,
    /// Use an attributes script to generate the attributes sent to hawkbit
    pub attributes_script: Attributes,
    /// Timeout for connections to the hawkbit server
    pub timeout: Duration,
}

impl Hawkbit {
    /// Create a new Hawkbit update source. It may fail if the DDI client cannot be created.
    ///
    /// arguments:
    /// - url: The URL of the Hawkbit server.
    /// - tenant: The tenant to use on the Hawkbit server (the docker container uses DEFAULT).
    /// - controller: The controller (name of the client) to use on the Hawkbit server.
    /// - token: The token to authenticate with the Hawkbit server.
    /// - options: Options to configure the behavior of the Hawkbit update source.
    pub async fn new(
        url: &str,
        tenant: &str,
        controller: &str,
        authorization: ClientAuthorization,
        options: HawkbitOptions,
    ) -> Result<Self, UpdateError> {
        let server_cert = options.server_cert.as_deref();
        let client_cert = options.client_cert.as_deref();

        let builder =
            DownloadClient::client_builder(server_cert, client_cert, Some(options.timeout))
                .await
                .map_err(|e| {
                    UpdateSourceError::ConfigurationError(format!(
                        "Failed to create client builder: {e}"
                    ))
                })?;

        let client =
            Client::new_from_client_builder(url, tenant, controller, authorization, builder)?;
        Ok(Self {
            client,
            polling_interval: options.default_polling_interval,
            options,
        })
    }

    /// Returns a default [builder](HawkbitBuilder) for the [`Hawkbit`] update source.
    // This method will help users to discover the builder.
    pub fn builder() -> HawkbitBuilder {
        HawkbitBuilder::default()
    }

    pub fn preferred_polling_interval(&self) -> &std::time::Duration {
        &self.polling_interval
    }

    /// Run an attributes script to generate the attributes sent to hawkbit.
    async fn run_attributes_script(&self) -> Option<HashMap<String, String>> {
        let script = match &self.options.attributes_script {
            Attributes::Script(script) => script,
            Attributes::Function(f) => return f.run().await,
            Attributes::Default => return None,
        };

        info!(target = LOG, "Running attributes script");
        let output = tokio::process::Command::new(script)
            .output()
            .await
            .map_err(|e| {
                error!(LOG, "Could not run attributes script: {:?}", e);
                e
            })
            .ok()?;

        if !output.status.success() {
            error!(LOG, "Attributes script failed: {:?}", output);
            return None;
        }

        serde_json::from_slice(&output.stdout)
            .map_err(|e| {
                error!(
                    LOG,
                    "Could not parse output of attributes script as json: {:?}", e
                );
                e
            })
            .ok()
    }

    /// Tries to get a version string for the currently running OS by parsing
    /// `/etc/product-release` or if not available `/etc/os-release`.
    pub async fn get_os_version(&self) -> Option<String> {
        if let Ok(content) = tokio::fs::read_to_string("/etc/product-release").await {
            if let Some(os_version) = Regex::new(r#"PRODUCT_VERSION="(.*)""#).ok().and_then(|re| {
                re.captures(&content)
                    .map(|os_version| os_version[1].to_string())
            }) {
                return Some(os_version);
            }
        }

        if let Ok(content) = tokio::fs::read_to_string("/etc/os-release").await {
            if let Some(os_version) = Regex::new(r#"VERSION_ID=(.*)"#).ok().and_then(|re| {
                re.captures(&content)
                    .map(|os_version| os_version[1].to_string())
            }) {
                return Some(os_version);
            }
        }

        None
    }

    /// Handle and reply to a config data request from the Hawkbit server. If an attributes script is configured, it is run to generate the config data.
    /// If the script fails or is not configured, default config data is uploaded instead.
    ///
    /// arguments:
    /// - request: The config data request from the Hawkbit server.
    ///
    /// returns:
    /// - Ok(()): If the config data request was handled successfully.
    /// - Err(UpdateError): If an error occurred while sending the response.
    async fn handle_config_request(&self, request: ConfigRequest) -> Result<(), UpdateError> {
        let data = match self.run_attributes_script().await {
            Some(attributes) => attributes,
            None => {
                info!(target = LOG, "Uploading default config data");
                HashMap::from([
                    (
                        "Client".to_string(),
                        format!("{} v{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")),
                    ),
                    (
                        "OSVersion".to_string(),
                        self.get_os_version().await.unwrap_or("NA".to_string()),
                    ),
                ])
            }
        };

        request
            .upload(
                Execution::Closed,
                Finished::Success,
                Some(hawkbit::ddi::Mode::Replace),
                data,
                vec![],
            )
            .await?;

        Ok(())
    }

    /// Handle and reply to a cancel action from the Hawkbit server.
    /// Currently, the cancel action is only logged and does not need to change state on the client side.
    ///
    /// arguments:
    /// - cancel: The cancel action from the Hawkbit server.
    ///
    /// returns:
    /// - Ok(()): If the cancel action was handled successfully.
    /// - Err(UpdateError): If an error occurred while answering the request.
    async fn handle_cancel_action(&self, cancel: CancelAction) -> Result<(), UpdateError> {
        let action = cancel.id().await?;
        info!(action, "Cancel action");

        cancel
            .send_feedback(Execution::Proceeding, Finished::None, vec!["Cancelling"])
            .await?;

        cancel
            .send_feedback(Execution::Closed, Finished::Success, vec![])
            .await?;

        info!(target = LOG, "cancelled");
        Ok(())
    }
}
