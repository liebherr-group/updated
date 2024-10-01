// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

use bytes::Bytes;
use lazy_static::lazy_static;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::task::JoinHandle;

use tokio::sync::mpsc::{Receiver, Sender, channel};
use tracing::info;

use crate::LOG;
use crate::installer::{
    InstallProgress, InstallationFeedback, InstallerError, Rollback, RollbackStatus,
};
use crate::persistent_store::PersistentStore;
use crate::uboot::{UBootConfig, UBootEnv};
use crate::update_source::{StreamError, StreamReceiver};
use crate::{
    installer::Installer,
    update_source::Update,
    update_workflow::{self, UpdateError, WorkflowError},
};

/// SWUpdate install modes
#[derive(Clone, Default)]
pub enum SWUpdateInstallMode {
    /// Stream the update directly into the target, without any temporary copy
    #[default]
    Stream,
    /// Copy the update to disk before sending it to the SWUpdate client
    TemporaryCopy {
        /// Directory used for the temporary copy on disk
        tmp_dir: PathBuf,
        /// Size of the buffer used to read the update file from disk
        bufsize: usize,
    },
}

/// Options for the SWUpdate installer
#[derive(Clone)]
pub struct SWUpdateOptions {
    /// the install mode to use (direct stream vs. temporary copy)
    pub install_mode: SWUpdateInstallMode,
    /// run the postupdate script after the installation
    pub run_postupdate: bool,
    /// do not actually install the update, just simulate the installation
    pub dry_run: bool,
    /// path to the swupdate client binary
    pub swupdate_client_bin: String,
    /// path to the swupdate IPC socket
    pub swupdate_ipc_socket: String,
    /// software and mode to use for the installation
    pub swupdate_sw_mode: Option<(String, String)>,
    /// u-boot environment to roll back updates
    pub uboot_env: UBootConfig,
}

impl Default for SWUpdateOptions {
    /// default configuration for the SWUpdate installer
    fn default() -> Self {
        Self {
            install_mode: SWUpdateInstallMode::default(),
            run_postupdate: true,
            dry_run: false,
            swupdate_client_bin: "/usr/bin/swupdate-client".to_string(),
            swupdate_ipc_socket: "/run/swupdate/sockinstctrl".to_string(),
            swupdate_sw_mode: None,
            uboot_env: UBootConfig::default(),
        }
    }
}

impl SWUpdateOptions {
    pub fn from_config(config: &HashMap<String, String>) -> Result<Self, WorkflowError> {
        let use_streaming = update_workflow::get_config_as_bool(config, "swupdate_use_streaming")?;
        Ok(Self {
            install_mode: if use_streaming {
                SWUpdateInstallMode::Stream
            } else {
                SWUpdateInstallMode::TemporaryCopy {
                    tmp_dir: update_workflow::get_config(config, "swupdate_tmp_dir")?.into(),
                    bufsize: update_workflow::get_config(config, "swupdate_bufsize")?
                        .parse()
                        .map_err(|_| {
                            WorkflowError::InvalidConfiguration(
                                "failed to parse swupdate_bufsize".to_string(),
                            )
                        })?,
                }
            },
            run_postupdate: update_workflow::get_config_as_bool(config, "swupdate_run_postupdate")?,
            dry_run: update_workflow::get_config_as_bool(config, "swupdate_dry_run")?,
            swupdate_client_bin: update_workflow::get_config(config, "swupdate_client_bin")?,
            swupdate_ipc_socket: update_workflow::get_config(config, "swupdate_ipc_socket")?,
            swupdate_sw_mode: if config.contains_key("swupdate_software_set") {
                Some((
                    update_workflow::get_config(config, "swupdate_software_set")?,
                    update_workflow::get_config(config, "swupdate_running_mode")?,
                ))
            } else {
                None
            },
            uboot_env: UBootConfig {
                setenv_bin: update_workflow::get_config(config, "swupdate_setenv_bin")?,
                printenv_bin: update_workflow::get_config(config, "swupdate_printenv_bin")?,
            },
        })
    }
}

/// SWUpdate installer, that uses the swupdate client command line tool to install updates
pub struct SWUpdate {
    /// configuration options for the SWUpdate installer
    options: SWUpdateOptions,
}

impl SWUpdate {
    /// Creates a new SWUpdate installer instance.
    /// ```
    /// use libupdated::swupdate::{SWUpdate, SWUpdateOptions};
    ///
    /// let installer = SWUpdate::new(SWUpdateOptions::default());
    /// ```
    #[allow(dead_code)]
    pub fn new(options: SWUpdateOptions) -> Self {
        Self { options }
    }

    /// Enable or disable automatic rollback in the bootloader.
    /// If enabled, the bootloader will automatically trigger a rollback, if the device is rebooted a set number of times (bootlimit).
    /// The current bootcount is _not_ reset to 0.
    pub async fn auto_rollback(&self, enabled: bool) -> Result<(), InstallerError> {
        let mut uboot = UBootEnv::new(self.options.uboot_env.clone());
        let v = if enabled { "1" } else { "0" };

        uboot.save("upgrade_available", v).await.map_err(|e| {
            InstallerError::RollbackFailed(format!("Failed to save upgrade_available flag: {}", e))
        })?;
        uboot.save("ustate", v).await.map_err(|e| {
            InstallerError::RollbackFailed(format!("Failed to save ustate flag: {}", e))
        })?;

        Ok(())
    }
}

impl Rollback for SWUpdate {
    async fn rollback(&self) -> Result<RollbackStatus, InstallerError> {
        let mut uboot = UBootEnv::new(self.options.uboot_env.clone());

        // This is the default swupdate rollback implementation for most devices.
        // Some devices might need a different rollback implementation.
        // For example, the DC5 uses a register to store the bootcount instead of the U-Boot environment.
        // In this case, a different Rollback implementation should be provided in the project.

        // Enable the rollback feature, so the bootcount is checked.
        self.auto_rollback(true).await?;

        // Get the bootcount limit that triggers a rollback
        let max_bootcount = uboot
            .load("bootlimit")
            .await
            .map_err(|e| {
                InstallerError::RollbackFailed(format!("Failed to read bootlimit: {}", e))
            })?
            .ok_or(InstallerError::RollbackFailed(
                "bootlimit not found".to_string(),
            ))?;

        // Set the current bootcount to this limit.
        uboot.save("bootcount", &max_bootcount).await.map_err(|e| {
            InstallerError::RollbackFailed(format!("Failed to save bootcount: {}", e))
        })?;

        Ok(RollbackStatus::RebootRequired)
    }
}

/// Get an update stream for the given update. If `use_streaming` is disabled, the update is saved to disk first.
///
/// It is a problem to report errors that occur in the spawned task that forwards the stream, as it is detached from the main task.
/// A oneshot channel is therefore used to send any errors that occur in the spawned task to the main task and should be awaited/checked there.
async fn get_update_stream(
    options: &SWUpdateOptions,
    update: &impl Update,
) -> Result<StreamReceiver, UpdateError> {
    let result = match &options.install_mode {
        SWUpdateInstallMode::Stream => {
            // stream the update directly
            update.stream()?
        }
        SWUpdateInstallMode::TemporaryCopy { tmp_dir, bufsize } => {
            // save the update to disk and send it from there
            tokio::fs::create_dir_all(tmp_dir).await?;
            let update_files = update.save_to_disk(Some(PathBuf::from(tmp_dir))).await?;
            let (tx, rx) = channel(1);
            let bufsize: usize = *bufsize;
            // Spawn task that reads the file and sends it to the channel.
            // Any errors are sent to the main task through the error channel.
            tokio::spawn(async move {
                async {
                    for update_file in update_files {
                        let file = File::open(update_file.filename).await?;

                        // send the file in 1MB chunks
                        let mut reader = BufReader::with_capacity(bufsize, file);

                        loop {
                            let len = {
                                let buffer = reader.fill_buf().await?;
                                tx.send(Ok(Bytes::copy_from_slice(buffer))).await?;
                                buffer.len()
                            };

                            if len == 0 {
                                break;
                            }

                            reader.consume(len);
                        }
                    }

                    Ok::<(), StreamError>(())
                }
                .await
                .map_err(|e| {
                    // send the error to the main task
                    tx.try_send(Err(e)).ok();
                })
                .ok();
            });
            rx
        }
    };
    Ok(result)
}

impl Installer for SWUpdate {
    fn install(&self, update: &impl Update) -> Result<impl InstallProgress, UpdateError> {
        let (feedback_sender, feedback_receiver) = channel(1);

        let options = self.options.clone();
        let update = update.clone();
        let client_handle = tokio::spawn(async move {
            // get the data stream for the update
            let update_stream = get_update_stream(&options, &update).await?;

            // configure the swupdate client for the installation
            let mut client_builder = SwupdateClientBuilder::new(
                &options.swupdate_client_bin,
                &options.swupdate_ipc_socket,
            )
            .update_stream(update_stream, update.size())
            .feedback_stream(feedback_sender);

            if options.run_postupdate {
                client_builder = client_builder.run_postupdate();
            }

            if options.dry_run {
                client_builder = client_builder.dry_run();
            }

            if let Some((software, mode)) = options.swupdate_sw_mode.as_ref() {
                client_builder = client_builder.sw_mode(software, mode);
            }

            let mut client = client_builder.build();

            // do the installation
            client.run().await
        });
        Ok(SWUpdateProgress {
            feedback_receiver,
            client_handle: Some(client_handle),
        })
    }
}

struct SWUpdateProgress {
    feedback_receiver: Receiver<InstallationFeedback>,
    client_handle: Option<JoinHandle<Result<(), UpdateError>>>,
}

impl InstallProgress for SWUpdateProgress {
    async fn next(&mut self) -> Result<Option<InstallationFeedback>, UpdateError> {
        // wait for feedback or until the channel is closed
        if let Some(msg) = self.feedback_receiver.recv().await {
            return Ok(Some(msg));
        }

        // if the channel is closed, wait for the client to finish and thereby consume the handle
        if let Some(handle) = self.client_handle.take() {
            handle.await.map_err(|e| {
                UpdateError::InstallerError(InstallerError::InstallationFailed(format!(
                    "swupdate client failed: {}",
                    e
                )))
            })??;
        }

        // the installation is finished
        Ok(None)
    }
}

/// Builder for the SwupdateClient struct
pub struct SwupdateClientBuilder {
    /// path to the swupdate client binary
    binary: PathBuf,
    /// command line flags for the swupdate client
    flags: Vec<String>,
    /// channel to send the update to the swupdate client
    stdin_channel: Option<StreamReceiver>,
    /// channel to receive parsed feedback from the swupdate client
    stdout_channel: Option<Sender<InstallationFeedback>>,
    /// the size of the update that will be installed
    update_size: Option<u64>,
}

impl SwupdateClientBuilder {
    pub fn new(binary: &str, ipc_socket: &str) -> Self {
        Self {
            binary: PathBuf::from(binary),
            flags: vec!["-s".to_string(), ipc_socket.to_string()],
            stdin_channel: None,
            stdout_channel: None,
            update_size: None,
        }
    }

    pub fn update_stream(mut self, update: StreamReceiver, update_size: u64) -> Self {
        self.stdin_channel = Some(update);
        self.update_size = Some(update_size);
        self
    }

    pub fn feedback_stream(mut self, feedback: Sender<InstallationFeedback>) -> Self {
        self.flags.push("-v".to_string());
        self.stdout_channel = Some(feedback);
        self
    }

    pub fn run_postupdate(mut self) -> Self {
        self.flags.push("-p".to_string());
        self
    }

    pub fn dry_run(mut self) -> Self {
        self.flags.push("-d".to_string());
        self
    }

    pub fn sw_mode(mut self, software: &str, mode: &str) -> Self {
        self.flags.push("-e".to_string());
        self.flags.push(format!("{software},{mode}"));
        self
    }

    pub fn build(self) -> SwupdateClient {
        SwupdateClient {
            binary: self.binary,
            flags: self.flags,
            stdin_channel: self.stdin_channel,
            stdout_channel: self.stdout_channel,
            update_size: self.update_size,
        }
    }
}

/// Helper struct to wrap around the swupdate client command line tool.
/// It exposes stdin and stdout as channels and parses progress feedback into `InstallationFeedback` messages.
///
/// ```
/// use libupdated::swupdate::{SwupdateClient, SwupdateClientBuilder};
///
/// let (tx, rx) = tokio::sync::mpsc::channel(1);
/// let sz = 1024; // size of the update
///
/// let client = SwupdateClientBuilder::new("/usr/bin/swupdate-client", "/var/run/swupdate.socket")
///    .update_stream(rx, sz)
///    .dry_run()
///    .build();
///
/// //client.run().await.expect("installation failed");
/// ```
pub struct SwupdateClient {
    binary: PathBuf,
    flags: Vec<String>,
    stdin_channel: Option<StreamReceiver>,
    stdout_channel: Option<Sender<InstallationFeedback>>,
    update_size: Option<u64>,
}

impl SwupdateClient {
    /// Runs the swupdate client command line tool with the configured options.
    /// It spawns a process for the command line tool with an empty environment and pipes its stdin/stdout.
    pub async fn run(&mut self) -> Result<(), UpdateError> {
        // build the command to spawn, e.g.: /usr/bin/swupdate-client -s /var/run/swupdate.ipc -v -e stable,root_a -d
        let mut cmd = Command::new(&self.binary);
        cmd.env_clear().kill_on_drop(true).args(&self.flags);

        // pipe stdin/stdout if channels are configured
        if self.stdin_channel.is_some() {
            cmd.stdin(Stdio::piped());
        }

        if self.stdout_channel.is_some() {
            cmd.stdout(Stdio::piped());
        }

        // spawn the child process. We can now take its stdin/stdout.
        let mut child = cmd.spawn()?;

        let stdout_channel = self.stdout_channel.take();
        let update_size = self.update_size;

        // spawn a task that continuously feeds incoming data from the stdin channel to the child process
        let stdin_task = if let Some(mut stdin_channel) = self.stdin_channel.take() {
            let stdout_channel = stdout_channel.clone();
            let mut stdin = child.stdin.take().expect("child stdin should be piped");

            Some(tokio::spawn(async move {
                let mut bytes_received = 0usize;
                while let Some(chunk) = stdin_channel.recv().await {
                    let chunk = chunk?;
                    bytes_received += chunk.len();
                    // report the number of bytes received to the feedback channel
                    // in order to rate limit the messages, we only send a message every 10MB
                    if let Some(stdout_channel) = &stdout_channel {
                        if bytes_received / (1024 * 1024 * 10)
                            > (bytes_received - chunk.len()) / (1024 * 1024 * 10)
                        {
                            stdout_channel
                                .send(InstallationFeedback {
                                    status: "0".to_string(),
                                    message: format!("Received {} bytes", bytes_received),
                                    progress: if update_size.is_some() {
                                        Some((bytes_received as u64, update_size.unwrap_or(0)))
                                    } else {
                                        None
                                    },
                                })
                                .await
                                .map_err(|err| {
                                    InstallerError::InstallationFailed(format!(
                                        "failed to report bytes received: {}",
                                        err
                                    ))
                                })?;
                        }
                    }
                    stdin.write_all(&chunk).await?;
                }
                Ok::<(), UpdateError>(())
            }))
        } else {
            None
        };

        // spawn a task that continuously reads the stdout of the child process.
        // This output is parsed into InstallationFeedback messages that are then sent to the feedback channel.
        let stdout_task = if let Some(stdout_channel) = stdout_channel {
            let stdout = child.stdout.take().expect("child stdout should be piped");

            Some(tokio::spawn(async move {
                let mut reader = BufReader::new(stdout);
                let mut buffer = Vec::new();
                loop {
                    buffer.clear();
                    let len = reader.read_until(b'\n', &mut buffer).await?;
                    if len == 0 {
                        break;
                    }

                    let msg = String::from_utf8_lossy(&buffer).to_string();

                    lazy_static! {
                        static ref STATUS_RE: regex::Regex =
                            regex::Regex::new(r"Status: (\d+) message: (.*)").unwrap();
                    }
                    if let Some(captures) = STATUS_RE.captures(&msg) {
                        let status = captures
                            .get(1)
                            .expect("regex should have a capture group 1")
                            .as_str();
                        let message = captures
                            .get(2)
                            .expect("regex should have a capture group 2")
                            .as_str();
                        stdout_channel
                            .send(InstallationFeedback {
                                status: status.to_string(),
                                message: message.to_string(),
                                progress: None,
                            })
                            .await
                            .map_err(|err| {
                                InstallerError::InstallationFailed(format!(
                                    "pipe to swupdate client broken: {}",
                                    err
                                ))
                            })?;
                    } else {
                        info!(LOG, "swupdate client: {}", msg);
                    }
                }
                Ok::<(), UpdateError>(())
            }))
        } else {
            None
        };

        // wait for the stdin and stdout tasks to finish and report any errors
        if let Some(task) = stdin_task {
            task.await.map_err(|e| {
                UpdateError::InstallerError(InstallerError::InstallationFailed(format!(
                    "stdin task failed: {}",
                    e
                )))
            })??;
        }

        if let Some(task) = stdout_task {
            task.await.map_err(|e| {
                UpdateError::InstallerError(InstallerError::InstallationFailed(format!(
                    "stdout task failed: {}",
                    e
                )))
            })??;
        }

        // wait for the child process to finish and check its exit status
        let status = child.wait().await?;

        if !status.success() {
            return Err(UpdateError::InstallerError(
                InstallerError::InstallationFailed(format!(
                    "swupdate client failed with exit code: {}",
                    status
                )),
            ));
        }

        Ok(())
    }
}
