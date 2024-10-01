// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

use std::collections::HashMap;
use std::path::PathBuf;

use libupdated::installer::{InstallProgress, InstallationFeedback, Installer};
use libupdated::persistent_store::{PersistentStore, StoreError};
use libupdated::traits::update_source::{UpdateInfo, UpdateSource, UpdateSourceError};
use libupdated::update_source::{StreamError, StreamSender, Update, UpdateFile};
use libupdated::update_workflow::UpdateError;

#[derive(Clone)]
pub struct DummyUpdate {
    pub version: String,
}

impl Update for DummyUpdate {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn version(&self) -> &str {
        &self.version
    }

    fn size(&self) -> u64 {
        "dummy content".len() as u64
    }

    async fn save_to_disk_by_name(
        &self,
        artifact: String,
        save_path: Option<PathBuf>,
    ) -> Result<UpdateFile<impl Update>, UpdateError> {
        assert_eq!(artifact, "dummy.txt");
        let file_path = save_path.unwrap_or(PathBuf::from("/tmp")).join("dummy.txt");
        if let Err(err) = tokio::fs::write(file_path.as_path(), "dummy content").await {
            Err(UpdateError::from(UpdateSourceError::ConnectionError(
                format!("save_to_disk failed: {}", err),
            )))
        } else {
            Ok(UpdateFile::new(&file_path, self).await?)
        }
    }

    async fn stream_by_name(&self, artifact: String, tx: &StreamSender) -> () {
        assert_eq!(artifact, "dummy.txt");
        tx.send(Err(StreamError::Unsupported)).await.ok();
    }

    fn files(&self) -> Vec<String> {
        vec!["dummy.txt".to_string()]
    }
}

#[derive(Clone)]
pub struct DummyUpdateInfo {
    pub needs_consent: bool,
    pub metadata: HashMap<String, String>,
    pub version: String,
    pub can_give_consent: bool,
}

impl UpdateInfo for DummyUpdateInfo {
    fn needs_consent(&self) -> bool {
        self.needs_consent
    }

    fn metadata(&self) -> HashMap<String, String> {
        self.metadata.clone()
    }

    fn version(&self) -> String {
        self.version.clone()
    }

    async fn give_consent(&mut self) -> Result<(), UpdateError> {
        if !self.needs_consent {
            return Ok(());
        }

        if self.can_give_consent {
            self.needs_consent = false;
            return Ok(());
        }

        Err(UpdateError::from(UpdateSourceError::ConnectionError(
            "connection lost".to_string(),
        )))
    }

    fn update(&self) -> Result<impl Update, UpdateError> {
        if self.needs_consent {
            Err(UpdateError::from(UpdateSourceError::ConsentRequired))
        } else {
            Ok(DummyUpdate {
                version: self.version.clone(),
            })
        }
    }
}
pub struct DummySource {
    pub pending_update: Option<DummyUpdateInfo>,
}

impl UpdateSource for DummySource {
    async fn check_for_updates(&mut self) -> Result<impl UpdateInfo, UpdateError> {
        if let Some(update) = self.pending_update.take() {
            Ok(update)
        } else {
            Err(UpdateError::NoPendingUpdates)
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct HashMapStore {
    map: HashMap<String, String>,
}

impl PersistentStore for HashMapStore {
    async fn delete(&mut self, key: &str) -> Result<(), StoreError> {
        self.map.remove(key);
        Ok(())
    }

    async fn exists(&self, key: &str) -> Result<bool, StoreError> {
        Ok(self.map.contains_key(key))
    }

    async fn save(&mut self, key: &str, value: &str) -> Result<(), StoreError> {
        self.map.insert(key.to_string(), value.to_string());
        Ok(())
    }

    async fn load(&self, key: &str) -> Result<Option<String>, StoreError> {
        Ok(self.map.get(key).cloned())
    }
}

pub struct DummyInstall;

impl Installer for DummyInstall {
    fn install(&self, _update: &impl Update) -> Result<impl InstallProgress, UpdateError> {
        Ok(DummyInstallProgress {})
    }
}

pub struct DummyInstallProgress;

impl InstallProgress for DummyInstallProgress {
    async fn next(&mut self) -> Result<Option<InstallationFeedback>, UpdateError> {
        Ok(None)
    }
}
