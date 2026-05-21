use super::{AssetRef, Backend};
use crate::{
    asset::{Asset, AssetType},
    lockfile::LockfileEntry,
    sync::backend::Params,
    web_api::WebApiClient,
};
use anyhow::Context;
use fs_err::tokio as fs;
use log::{info, warn};
use relative_path::RelativePathBuf;
use roblox_install::RobloxStudio;
use std::{env, path::PathBuf};

pub struct Studio {
    identifier: String,
    sync_path: PathBuf,
    cloud: Option<WebApiClient>,
}

impl Backend for Studio {
    async fn new(params: Params) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        let studio = RobloxStudio::locate()?;
        let content_path = studio.content_path();

        let cwd = env::current_dir()?;
        let cwd_name = cwd
            .file_name()
            .and_then(|s| s.to_str())
            .context("Failed to get current directory name")?;

        let project_name = cwd_name
            .to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join("-");

        let identifier = format!(".asphalt-{project_name}");
        let sync_path = content_path.join(&identifier);

        info!("Assets will be synced to: {}", sync_path.display());

        if sync_path.exists() {
            fs::remove_dir_all(&sync_path).await?;
        }

        Ok(Self {
            identifier,
            sync_path,
            cloud: params
                .api_key
                .map(|api_key| WebApiClient::new(api_key, params.creator, params.expected_price)),
        })
    }

    async fn sync(
        &self,
        asset: &Asset,
        lockfile_entry: Option<&LockfileEntry>,
    ) -> anyhow::Result<Option<AssetRef>> {
        if matches!(asset.ty, AssetType::Model(_) | AssetType::Animation) {
            return match lockfile_entry {
                Some(entry) => Ok(Some(AssetRef::Cloud(entry.asset_id))),
                None => {
                    let Some(client) = &self.cloud else {
                        warn!(
                            "Models and Animations cannot be synced to Studio without having been uploaded first or providing an API key"
                        );
                        return Ok(None);
                    };

                    info!(
                        "Uploading model or animation for Studio sync: {}",
                        asset.path
                    );
                    let asset_id = client
                        .upload(asset)
                        .await
                        .context("Failed to upload model or animation while syncing to Studio")?;

                    Ok(Some(AssetRef::Cloud(asset_id)))
                }
            };
        }

        let rel_target_path =
            RelativePathBuf::from(&asset.hash.to_string()).with_extension(&asset.ext);
        let target_path = rel_target_path.to_logical_path(&self.sync_path);

        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        fs::write(&target_path, &asset.data).await?;

        Ok(Some(AssetRef::Studio(format!(
            "{}/{}",
            self.identifier, rel_target_path
        ))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Creator, CreatorType};
    use relative_path::RelativePathBuf;
    use std::env;

    #[tokio::test]
    async fn sync_generates_single_rbxasset_uri() {
        let temp_dir = assert_fs::TempDir::new().unwrap();
        let asset = Asset::new(
            RelativePathBuf::from("image.png"),
            b"fake png".to_vec().into(),
        )
        .unwrap();

        let studio = Studio {
            identifier: ".asphalt-test".to_string(),
            sync_path: temp_dir.path().to_path_buf(),
            cloud: None,
        };

        let asset_ref = studio.sync(&asset, None).await.unwrap().unwrap();
        let expected = format!("rbxasset://.asphalt-test/{}.png", asset.hash);

        assert_eq!(asset_ref.to_string(), expected);
    }

    #[tokio::test]
    async fn sync_uploads_missing_models_when_api_key_is_available() {
        unsafe {
            env::set_var("ASPHALT_TEST", "true");
        }

        let temp_dir = assert_fs::TempDir::new().unwrap();
        let asset = Asset::new(
            RelativePathBuf::from("model.fbx"),
            b"fake fbx".to_vec().into(),
        )
        .unwrap();
        let expected_id = asset.hash.as_u64();

        let studio = Studio {
            identifier: ".asphalt-test".to_string(),
            sync_path: temp_dir.path().to_path_buf(),
            cloud: Some(WebApiClient::new(
                "test".to_string(),
                Creator {
                    ty: CreatorType::User,
                    id: 123,
                },
                None,
            )),
        };

        let asset_ref = studio.sync(&asset, None).await.unwrap().unwrap();

        assert_eq!(asset_ref.to_string(), format!("rbxassetid://{expected_id}"));
    }
}
