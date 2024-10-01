// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: <text>
// Copyright(c) 2026 Liebherr-Digital Development Center GmbH
// Written by Thomas Witte <thomas.witte@liebherr.com>
// </text>

use std::path::Path;

use tracing::{error, info};

use crate::{
    LOG,
    persistent_store::{PersistentStore, StoreError},
};

pub struct FileStore {
    file: String,
}

impl FileStore {
    pub fn new(file: &str) -> Self {
        Self {
            file: file.to_string(),
        }
    }

    async fn read_content(&self) -> Result<std::collections::HashMap<String, String>, StoreError> {
        if !tokio::fs::try_exists(&self.file).await? {
            info!(
                LOG,
                "Creating a empty store file {}, as it does not exist yet", self.file
            );
            tokio::fs::create_dir_all(Path::new(&self.file).parent().unwrap_or(Path::new("/")))
                .await?;
            tokio::fs::write(&self.file, "{}").await?;
        }

        let json = tokio::fs::read_to_string(&self.file).await?;
        match serde_json::from_str(&json) {
            Ok(map) => Ok(map),
            Err(e) => {
                error!(
                    LOG,
                    "Failed to parse store file {}. Trying to re-create it. Error: {e}", self.file
                );
                let backup_file = format!("{}.bak", &self.file);
                tokio::fs::copy(&self.file, &backup_file).await?;
                tokio::fs::write(&self.file, "{}").await?;
                info!(
                    LOG,
                    "Backup of corrupted store file created at {backup_file}"
                );
                Ok(std::collections::HashMap::new())
            }
        }
    }
}

impl PersistentStore for FileStore {
    async fn save(&mut self, key: &str, value: &str) -> Result<(), StoreError> {
        let mut map = self.read_content().await?;

        // update the map
        map.insert(key.to_string(), value.to_string());

        // write back to the store file
        tokio::fs::write(&self.file, serde_json::to_string(&map)?).await?;

        Ok(())
    }

    async fn load(&self, key: &str) -> Result<Option<String>, StoreError> {
        let map = self.read_content().await?;

        // get the value
        let value = map.get(key).map(|s| s.to_string());

        Ok(value)
    }

    async fn delete(&mut self, key: &str) -> Result<(), StoreError> {
        let mut map = self.read_content().await?;

        // update the map
        map.remove(key);

        // write back to the store file
        tokio::fs::write(&self.file, serde_json::to_string(&map)?).await?;

        Ok(())
    }

    async fn exists(&self, key: &str) -> Result<bool, StoreError> {
        let map = self.read_content().await?;

        Ok(map.contains_key(key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_file_store() {
        let file = "/tmp/test_file_store.json";
        tokio::fs::remove_file(file).await.ok();
        let mut store = FileStore::new(file);

        // load non-existing value
        let value = store.load("key").await.unwrap();
        assert_eq!(value, None);

        // save a value
        store.save("key", "value").await.unwrap();

        // load the value
        let value = store.load("key").await.unwrap();
        assert_eq!(value, Some("value".to_string()));

        // delete the value
        store.delete("key").await.unwrap();

        // check if the value is deleted
        let value = store.load("key").await.unwrap();
        assert_eq!(value, None);
    }

    #[tokio::test]
    async fn test_file_store_json() {
        let file = "/tmp/test_file_store_json.json";
        tokio::fs::write(file, r#"{"test": "hello", "foo": "bar"}"#)
            .await
            .ok();
        let mut store = FileStore::new(file);

        let value = store.load("test").await.unwrap();
        assert_eq!(value, Some("hello".to_string()));

        assert!(store.exists("foo").await.unwrap());

        // save a value with special characters
        store.save("😈", r#"{"key": "value"}"#).await.unwrap();

        // load the value
        let value = store.load("😈").await.unwrap();
        assert_eq!(value, Some(r#"{"key": "value"}"#.to_string()));
    }

    #[tokio::test]
    async fn test_file_store_corrupted() {
        let file = "/tmp/test_file_store_corrupted.json";
        tokio::fs::write(file, r#"{"test": "hello", "foo": "bar""#)
            .await
            .expect("Failed to write corrupted file");
        let store = FileStore::new(file);
        let value = store.load("test").await.unwrap();
        assert_eq!(value, None);

        // the file should now be re-created
        let recreated = tokio::fs::read_to_string(file)
            .await
            .expect("Failed to read re-created file");
        assert_eq!(recreated, "{}");

        // test whether the backup file is created
        let backup_file = format!("{}.bak", file);
        assert!(tokio::fs::try_exists(&backup_file).await.unwrap());

        tokio::fs::remove_file(&backup_file).await.ok();
        tokio::fs::remove_file(file).await.ok();
    }
}
