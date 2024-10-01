// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use futures::StreamExt;
use hawkbit::ddi::ConfirmationInfo;
use tracing::info;

use crate::{
    LOG,
    update_source::{
        StreamError, StreamSender, Update, UpdateFile, UpdateInfo, UpdateSource, UpdateSourceError,
    },
    update_workflow::UpdateError,
};

use super::Hawkbit;

/// The HawkbitUpdateInfo struct holds information about a pending update on the Hawkbit server.
#[derive(Debug)]
pub struct HawkbitUpdateInfo {
    /// The reply to the poll request that returned this update info.
    poll_reply: hawkbit::ddi::Reply,
    /// The type of update info: either a request for confirmation or the actual update.
    info: InfoType,
    /// The Hawkbit client to be able to poll the server.
    client: hawkbit::ddi::Client,
}

/// The InfoType enum holds either a request for confirmation (Confirm) or the update (Update).
#[derive(Debug)]
pub(crate) enum InfoType {
    /// The update still needs confirmation.
    Confirm(ConfirmationInfo),
    /// The update is ready to be downloaded/installed.
    Update(Arc<hawkbit::ddi::Update>),
}

/// The HawkbitUpdate struct used to download the update. Its lifetime is tied to the HawkbitUpdateInfo, which owns the ddi::Update object.
#[derive(Debug, Clone)]
pub struct HawkbitUpdate {
    /// Reference to the fetched update object.
    pub(crate) update: Arc<hawkbit::ddi::Update>,
}

impl HawkbitUpdateInfo {
    /// Try to create a HawkbitUpdateInfo from a poll reply. If the reply does not contain an update or confirmation request, an error is returned.
    ///
    /// arguments:
    /// - reply: The poll reply from the Hawkbit server.
    /// - client: The Hawkbit client to be able to poll the server.
    ///
    /// returns:
    /// - Ok(HawkbitUpdateInfo): The created HawkbitUpdateInfo object.
    /// - Err(UpdateSourceError::InvalidState): If the poll reply does not contain an update or confirmation request.
    pub(crate) async fn try_from_reply(
        reply: hawkbit::ddi::Reply,
        client: hawkbit::ddi::Client,
    ) -> Result<Self, UpdateError> {
        // there is a link to a deployment base, so we have an update that does not need confirmation (anymore)
        if let Some(update) = reply.update() {
            return Ok(Self {
                poll_reply: reply,
                info: InfoType::Update(Arc::new(update.fetch().await?)),
                client,
            });
        }

        // there is a link to a confirmation base, so we need to confirm the update first
        if let Some(confirmation) = reply.confirmation_base() {
            return Ok(Self {
                poll_reply: reply,
                info: InfoType::Confirm(confirmation.update_info().await?),
                client,
            });
        }

        // no update or confirmation request found -> we cannot create an update info
        Err(UpdateSourceError::InvalidState.into())
    }
}

impl UpdateInfo for HawkbitUpdateInfo {
    fn needs_consent(&self) -> bool {
        // if the UpdateInfo holds a confirmation request, consent is needed
        matches!(self.info, InfoType::Confirm(_))
    }

    /// Collect all metadata from all chunks and return it
    fn metadata(&self) -> HashMap<String, String> {
        match self.info {
            InfoType::Update(ref update) => {
                let chunks: Vec<hawkbit::ddi::Chunk> = update.chunks().collect();
                chunks
                    .iter()
                    .flat_map(|c| c.metadata().map(|(k, v)| (k.to_string(), v.to_string())))
                    .collect()
            }
            InfoType::Confirm(ref info) => info.metadata().into_iter().collect(),
        }
    }

    /// Return a version string for the update.
    /// Currently, this uses the hawkbit action ID as version as we cannot be sure that metadata for the update is available.
    fn version(&self) -> String {
        match self.info {
            InfoType::Update(ref update) => update.action_id().to_string(),
            InfoType::Confirm(ref info) => info.action_id().to_string(),
        }
    }

    /// Give consent to the update. If the update info is a confirmation request, the confirmation is sent to the server.
    /// Then, the update info is updated to hold the actual update by polling the hawkbit server.
    /// If the server does not return an update, after sending consent, an `UpdateSourceError::InvalidState` is returned.
    async fn give_consent(&mut self) -> Result<(), UpdateError> {
        match self.info {
            InfoType::Update(_) => Ok(()),
            InfoType::Confirm(_) => {
                self.poll_reply
                    .confirmation_base()
                    .unwrap()
                    .confirm()
                    .await?;

                self.poll_reply = self.client.poll().await?;
                self.info = InfoType::Update(Arc::new(
                    self.poll_reply
                        .update()
                        .ok_or(UpdateSourceError::InvalidState)?
                        .fetch()
                        .await?,
                ));

                Ok(())
            }
        }
    }

    /// Get a handle to download the update. A `UpdateSourceError::ConsentRequired`` error is returned if the update still needs confirmation.
    fn update(&self) -> Result<impl Update, UpdateError> {
        match &self.info {
            InfoType::Update(update) => Ok(HawkbitUpdate {
                update: update.clone(),
            }),
            InfoType::Confirm(_) => Err(UpdateSourceError::ConsentRequired.into()),
        }
    }
}

impl Update for HawkbitUpdate {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn version(&self) -> &str {
        self.update.action_id()
    }

    /// The size of the update in bytes.
    /// In case that the update contains more than one artifact, the
    /// sum of all download sizes is returned.
    /// If the update contains no files to download, size() returns 0.
    fn size(&self) -> u64 {
        let mut size = 0;
        for chunk in self.update.chunks() {
            for artifact in chunk.artifacts() {
                size += artifact.size() as u64;
            }
        }
        size
    }

    async fn save_to_disk_by_name(
        &self,
        filename: String,
        save_dir: Option<PathBuf>,
    ) -> Result<UpdateFile<impl Update>, UpdateError> {
        for chunk in self.update.chunks() {
            let artifact = chunk.artifacts().find(|a| a.filename() == filename);
            if let Some(artifact) = artifact {
                let save_dir = save_dir.unwrap_or(PathBuf::from(format!(
                    "/tmp/updated/update_{}",
                    self.version()
                )));
                let downloaded_artifact = artifact.download(save_dir.as_path()).await?;
                downloaded_artifact.check_sha256().await?;

                return UpdateFile::new(downloaded_artifact.file(), self).await;
            }
        }
        Err(UpdateError::UpdateSourceError(
            UpdateSourceError::FetchError(format!("Could not find artifact {filename} in update.")),
        ))
    }

    async fn stream_by_name(&self, filename: String, tx: &StreamSender) {
        for chunk in self.update.chunks() {
            let artifact = chunk.artifacts().find(|a| a.filename() == filename);
            if let Some(artifact) = artifact {
                info!(LOG, "Streaming artifact {}", filename);
                match artifact.download_stream_with_sha256_check().await {
                    Ok(mut stream) => {
                        while let Some(bytes) = stream.next().await {
                            if let Err(e) = tx.send(bytes.map_err(StreamError::from)).await {
                                tx.try_send(Err(e.into())).ok();
                            }
                        }
                    }
                    Err(e) => {
                        tx.try_send(Err(e.into())).ok();
                    }
                };
                return;
            }
        }

        // the artifact could not be found in the update
        tx.try_send(Err(StreamError::SendError(format!(
            "Could not find artifact {filename} in update."
        ))))
        .ok();
    }

    fn files(&self) -> Vec<String> {
        let mut result = Vec::new();
        for chunk in self.update.chunks() {
            for artifact in chunk.artifacts() {
                result.push(artifact.filename().to_string());
            }
        }
        result
    }
}

impl UpdateSource for Hawkbit {
    /// Poll the Hawkbit server for updates. If an update is available, the update info is returned.
    /// If the Hawkbit client is configured to handle config data requests or cancel actions, these are handled as well.
    /// This might be undesired if the client runs in parallel to another client that handles the download (swupdate-suricatta).
    #[allow(refining_impl_trait)]
    async fn check_for_updates(&mut self) -> Result<HawkbitUpdateInfo, UpdateError> {
        let reply = self.client.poll().await?;
        self.polling_interval = reply.polling_sleep()?;

        if self.options.handle_config
            && let Some(request) = reply.config_data_request()
        {
            self.handle_config_request(request).await?;
        }

        if self.options.handle_cancel
            && let Some(cancel) = reply.cancel_action()
        {
            self.handle_cancel_action(cancel).await?;
        }

        if reply.confirmation_base().is_some() || reply.update().is_some() {
            return HawkbitUpdateInfo::try_from_reply(reply, self.client.clone()).await;
        }

        Err(UpdateError::NoPendingUpdates)
    }
}
