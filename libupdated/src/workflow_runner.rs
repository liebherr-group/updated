// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

use std::process::ExitCode;

#[cfg(feature = "cli")]
use clap::Parser;
#[cfg(feature = "systemd")]
use sd_notify::{NotifyState, notify, watchdog_enabled};
use tokio::{
    signal::unix::{SignalKind, signal},
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

#[cfg(feature = "systemd")]
use std::time::Duration;

use crate::{LOG, update_workflow::WorkflowPlugin};

#[cfg(feature = "cli")]
const VERSION: &str = env!("VERGEN_GIT_DESCRIBE");
#[cfg(feature = "cli")]
const BUILD_DATE: &str = env!("VERGEN_BUILD_DATE");
#[cfg(feature = "cli")]
const TARGET_TRIPLE: &str = env!("VERGEN_CARGO_TARGET_TRIPLE");

pub struct Updated {
    workflow_name: String,
    config_file: String,
    join_set: JoinSet<Result<(), ExitCode>>,
    ct: CancellationToken,
}

#[cfg(feature = "cli")]
#[derive(Parser)]
#[command(name = "updated")]
#[command(version = VERSION)]
#[command(about = "SOTA workflow runner", long_about = None)]
struct Cli {
    #[arg(short, long)]
    workflow: String,

    #[arg(short, long)]
    config: String,
}

impl Updated {
    pub fn new(workflow_name: String, config_file: String) -> Self {
        let join_set = JoinSet::new();
        let ct = CancellationToken::new();

        Self {
            workflow_name,
            config_file,
            join_set,
            ct,
        }
    }

    #[cfg(feature = "cli")]
    pub fn from_cli() -> Self {
        let cli = Cli::parse();

        Self::new(cli.workflow, cli.config)
    }

    #[cfg(feature = "systemd")]
    pub fn spawn_watchdog_task(&mut self) {
        let mut usec: u64 = 0;
        if watchdog_enabled(false, &mut usec) {
            info!(target: LOG, "Systemd watchdog is enabled with a {} µs timeout. Sending keep alive.", usec);
            self.join_set.spawn(Self::keep_alive(
                self.ct.clone(),
                Duration::from_micros(usec / 2),
            ));
        } else {
            info!(target: LOG, "Systemd watchdog is not enabled.");
        }
    }

    pub fn spawn_signal_handler(&mut self) {
        let ct_clone = self.ct.clone();
        self.join_set.spawn(async move {
            let result = Self::os_signals(&ct_clone).await;
            ct_clone.cancel();
            result
        });
    }

    pub async fn run(mut self) -> ExitCode {
        let ct_clone = self.ct.clone();
        let workflow_name = self.workflow_name.clone();
        let config_file = self.config_file.clone();
        self.join_set.spawn(async move {
            match Self::init(&ct_clone, workflow_name, config_file).await {
                Ok(()) => Ok(()),
                Err(code) => {
                    ct_clone.cancel();
                    Err(code)
                }
            }
        });

        // Notify systemd that the service is ready
        #[cfg(feature = "systemd")]
        notify(false, &[NotifyState::Ready]).expect("Could not send READY=1 to systemd.");

        // If any of the tasks wants to return with a specific exit code, return this exit code
        let mut stopping = false;
        for result in self.join_set.join_all().await.iter() {
            if !stopping {
                #[cfg(feature = "systemd")]
                notify(false, &[NotifyState::Stopping])
                    .expect("Failed to send STOPPING=1 to systemd.");
                stopping = true;
            }

            match result {
                Err(code) => return *code,
                _ => continue,
            }
        }

        // Otherwise return success
        ExitCode::SUCCESS
    }

    /// Returns a list of all available workflow names.
    pub fn available_workflow_names() -> Vec<&'static str> {
        inventory::iter::<WorkflowPlugin>
            .into_iter()
            .map(|w| w.name)
            .collect()
    }

    /// Initialize and start the selected workflow.
    /// args:
    ///   ct: CancellationToken - used to signal the workflow or application to stop
    ///   workflow_name: String - the name of the workflow to start (it needs to be registered with the #\[workflow\] attribute)
    ///   config_file: String   - the path to the configuration file for the workflow
    ///
    /// The workflow is selected at runtime based on the workflow_name argument. The config file is loaded into a HashMap<String, String> and passed to the workflow function. Based on the file extension of the config file, it is parsed as either a JSON file or a dotenv file. If the dotenv config is used, environment variables of the process are considered as well but keys in the config file always take precedence. All keys are converted to lowercase when loaded from a dotenv config.
    pub async fn init(
        ct: &CancellationToken,
        workflow_name: String,
        config_file: String,
    ) -> Result<(), ExitCode> {
        #[cfg(feature = "cli")]
        info!(
            target: LOG,
            version = VERSION,
            date = BUILD_DATE,
            target = TARGET_TRIPLE,
            "{}.",
            clap::crate_description!()
        );

        let workflow_fn = inventory::iter::<WorkflowPlugin>
            .into_iter()
            .find(|&w| w.name == workflow_name)
            .ok_or_else(|| {
                error!(target: LOG, "Workflow not found: {}", workflow_name);
                ExitCode::FAILURE
            })?
            .func;

        // parse config file
        let config: std::collections::HashMap<String, String> = if config_file.ends_with(".json") {
            let content = tokio::fs::read_to_string(&config_file).await.map_err(|e| {
                error!(target: LOG, "Could not parse workflow config as json file: {}", e);
                ExitCode::FAILURE
            })?;
            serde_json::from_str(&content).expect("Could not parse workflow config file.")
        } else {
            dotenvy::from_filename_override(&config_file).map_err(|e| {
                error!(target: LOG, "Could not parse workflow config as env file: {}", e);
                ExitCode::FAILURE
            })?;
            let mut config = std::collections::HashMap::new();
            for (k, v) in dotenvy::vars() {
                config.insert(k.to_lowercase(), v.to_string());
            }
            config
        };

        tokio::select! {
            _ = ct.cancelled() => {
                info!(target: LOG, "Shutting down.");
            }
            result = workflow_fn(config) => {
                match result {
                    Ok(exit_code) => {
                        info!(target: LOG, "Update workflow finished with exit code {exit_code:?}.");
                        if exit_code != ExitCode::SUCCESS {
                            return Err(exit_code);
                        }
                    },
                    Err(e) => {
                        error!(target: LOG, "Update workflow failed: {}", e);
                        return Err(ExitCode::FAILURE);
                    }
                };
            }
        };

        Ok(())
    }

    #[cfg(feature = "systemd")]
    async fn keep_alive(
        ct: CancellationToken,
        watchdog_duration: Duration,
    ) -> Result<(), ExitCode> {
        use tokio::time::sleep;
        use tracing::trace;

        loop {
            tokio::select! {
                _ = ct.cancelled() => {
                    break;
                }
                _ = sleep(watchdog_duration) => {
                    trace!(target: LOG, "Sending systemd keep alive.");
                    notify(false, &[NotifyState::Watchdog]).expect("Could not send WATCHDOG=1 to systemd.");
                }
            }
        }

        Ok(())
    }

    async fn wait_for_signal(ct: &CancellationToken) -> Result<(), std::io::Error> {
        let mut sighup = signal(SignalKind::hangup())?;
        let mut sigint = signal(SignalKind::interrupt())?;
        let mut sigquit = signal(SignalKind::quit())?;
        let mut sigterm = signal(SignalKind::terminate())?;
        tokio::select! {
            _ = sighup.recv() => {
                info!(target: LOG, signal = "SIGHUP", "Signal received.");
            }
            _ = sigint.recv() => {
                info!(target: LOG, signal = "SIGINT", "Signal received.");
            }
            _ = sigquit.recv() => {
                info!(target: LOG, signal = "SIGQUIT", "Signal received.");
            }
            _ = sigterm.recv() => {
                info!(target: LOG, signal = "SIGTERM", "Signal received.");
            }
            _ = ct.cancelled() => {}
        }

        Ok(())
    }

    pub async fn os_signals(ct: &CancellationToken) -> Result<(), ExitCode> {
        Self::wait_for_signal(ct).await.map_err(|e| {
            error!(target: LOG, "Failed to register signal handler: {}", e);
            ExitCode::FAILURE
        })?;

        Ok(())
    }
}
