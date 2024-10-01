// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

use std::collections::HashMap;
use std::time::Duration;

use crate::update_workflow::{self, WorkflowError};

use super::{Attributes, ClientAuthorization, Hawkbit, HawkbitOptions};

// Default hawkBit server URL.
// Can be used for testing but has to be updated in the configuration for production servers.
// See with_config()
const DEFAULT_URL: &str = "https://localhost:8443";

// Default hawkBit tenant.
const DEFAULT_TENANT: &str = "default";

// Default flag for cancel actions from the hawkBit server.
const DEFAULT_HANDLE_CANCEL: bool = true;

// Default flag for config requests from the hawkBit server.
const DEFAULT_HANDLE_CONFIG: bool = true;

// Default hawkBit server polling interval is 5 minutes.
const DEFAULT_POLLING_INTERVAL_SECS: Duration = Duration::from_secs(300);

// Default timeout for connections to the hawkBit server is 30 seconds.
const DEFAULT_TIMEOUT_SECS: Duration = Duration::from_secs(30);

/// A builder used to configure and create a [`Hawkbit`] update source.
///
/// # Examples
///
/// Create a builder prefilled with default values, add some config and finally build the hawkBit update source:
/// ```
/// use::std::collections::HashMap;
/// use libupdated::hawkbit::{Hawkbit, HawkbitBuilder};
/// use libupdated::update_workflow::WorkflowError;
///
/// # fn main() -> Result<(), WorkflowError> {
/// let config = HashMap::from([
///     (
///         "sota_hawkbit_url".to_string(),
///         "https://localhost:8443".to_string(),
///     ),
///     ("sota_tenant".to_string(), "DEFAULT".to_string()),
///     ("sota_target_id".to_string(), "unique_client_id_aka_controller_id".to_string()),
///     ("sota_auth_mode".to_string(), "target".to_string()),
///     ("sota_auth_token".to_string(), "unique_target_token".to_string()),
///     (
///         "sota_default_polling_interval_sec".to_string(),
///         "10".to_string(),
///     ),
/// ]);
///
/// let mut builder = Hawkbit::builder();
/// let hawkbit = builder.with_config(&config)?.build();
///
/// # Ok(())
/// # }
/// ```
pub struct HawkbitBuilder {
    url: String,
    tenant: String,
    controller: String,
    authorization: ClientAuthorization,
    options: HawkbitOptions,
}

impl HawkbitBuilder {
    /// Creates a blank [`HawkbitBuilder`]
    ///
    /// # Examples
    ///
    /// ```
    /// use libupdated::hawkbit::HawkbitBuilder;
    /// let mut builder = HawkbitBuilder::new();
    /// ```
    pub fn new() -> Self {
        HawkbitBuilder {
            url: DEFAULT_URL.to_string(),
            tenant: DEFAULT_TENANT.to_string(),
            controller: String::new(),
            authorization: ClientAuthorization::None,
            options: HawkbitOptions {
                handle_cancel: DEFAULT_HANDLE_CANCEL,
                handle_config: DEFAULT_HANDLE_CONFIG,
                default_polling_interval: DEFAULT_POLLING_INTERVAL_SECS,
                server_cert: None,
                client_cert: None,
                attributes_script: Attributes::Default,
                timeout: DEFAULT_TIMEOUT_SECS,
            },
        }
    }

    /// Sets the hawkBit configuration for the [`Hawkbit`] update source.
    ///
    /// The configuration is provided as key-value pairs.
    ///
    /// The following keys are required:
    ///
    /// - sota_hawkbit_url: The URL of the hawkBit server.
    /// - sota_tenant:      The tenant to use on the hawkBit server (e.g. "default").
    /// - sota_target_id:   The controller (name of the client) to use on the hawkBit server.
    /// - sota_auth_mode:   The method of authorization for the client.
    /// - sota_auth_token:  The token to authenticate with the hawkBit server.
    /// - sota_default_polling_interval_sec
    ///
    /// These keys are optional:
    /// - sota_server_cert
    /// - sota_attributes_script
    ///
    /// If the “sota_attributes_script” key is omitted, the script or an alternative function
    /// can be set with [`HawkbitBuilder::attributes`].
    ///
    /// # Errors
    ///
    /// Returns a [`WorkflowError`] if the configuration is incomplete or invalid.
    pub fn with_config(
        &mut self,
        config: &HashMap<String, String>,
    ) -> Result<&mut Self, WorkflowError> {
        // required config
        self.url = update_workflow::get_config(config, "sota_hawkbit_url")?;
        self.tenant = update_workflow::get_config(config, "sota_tenant")?;
        self.controller = update_workflow::get_config(config, "sota_target_id")?;
        let auth_mode = update_workflow::get_config(config, "sota_auth_mode")?;
        self.authorization = match auth_mode.to_lowercase().as_str() {
            "gateway" => ClientAuthorization::GatewayToken(update_workflow::get_config(
                config,
                "sota_auth_token",
            )?),
            "target" => ClientAuthorization::TargetToken(update_workflow::get_config(
                config,
                "sota_auth_token",
            )?),
            _ => ClientAuthorization::None,
        };

        self.options.default_polling_interval = Duration::from_secs(
            update_workflow::get_config_as_u64(config, "sota_default_polling_interval_sec")?,
        );

        // optional config
        self.options.server_cert = config.get("sota_server_cert").cloned();
        self.options.client_cert = config.get("sota_client_cert").cloned();
        if let Some(script) = config.get("sota_attributes_script") {
            self.options.attributes_script = Attributes::Script(script.clone());
        }
        self.options.timeout = update_workflow::get_config_as_u64(config, "download_timeout_sec")
            .map(Duration::from_secs)
            .unwrap_or(DEFAULT_TIMEOUT_SECS);
        Ok(self)
    }

    /// Set some means for the generation of the hawkBit attributes.
    ///
    /// The hawkBit attributes can be generated using either
    /// - a script ([`Attributes::Script`]) or
    /// - a function ([`Attributes::Function`])
    ///
    /// To use default attributes instead of generating them, set [`Attributes::Default`].
    ///
    /// # Examples
    ///
    /// Using a script:
    /// ```
    /// use libupdated::hawkbit::{Attributes, HawkbitBuilder};
    ///
    /// let script_path = "/path/to/attr_script".to_string();
    /// let attr = Attributes::Script(script_path);
    ///
    /// let hawkbit = HawkbitBuilder::new().attributes(attr);
    /// ```
    ///
    /// Using a function:
    /// ```
    /// use std::{collections::HashMap, sync::Arc};
    ///
    /// use async_trait::async_trait;
    ///
    /// use libupdated::hawkbit::{Attributes, AttributesFn, HawkbitBuilder};
    ///
    /// #[derive(Debug, Default)]
    /// struct MyAttributes {
    /// }
    ///
    /// #[async_trait]
    /// impl AttributesFn for MyAttributes {
    ///     async fn run(&self) -> Option<HashMap<String, String>> {
    ///         Some(HashMap::from([(
    ///             "Client".to_string(),
    ///             format!("{} v{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")),
    ///         )]))
    ///     }
    /// }
    ///
    /// let attr = Attributes::Function(Arc::new(MyAttributes::default()));
    ///
    /// let hawkbit = HawkbitBuilder::new().attributes(attr);
    /// ```
    pub fn attributes(&mut self, attr: Attributes) -> &mut Self {
        self.options.attributes_script = attr;
        self
    }

    /// Builds the [`Hawkbit`] update source.
    ///
    /// # Errors
    ///
    /// Returns a [`WorkflowError`] if the hawkBit DDI client could not be built.
    pub async fn build(&self) -> Result<Hawkbit, WorkflowError> {
        Hawkbit::new(
            &self.url,
            &self.tenant,
            &self.controller,
            self.authorization.clone(),
            self.options.clone(),
        )
        .await
        .map_err(|e| {
            WorkflowError::ExecutionFailed(format!("Failed to create Hawkbit client: {e}"))
        })
    }
}

impl Default for HawkbitBuilder {
    fn default() -> Self {
        Self::new()
    }
}
