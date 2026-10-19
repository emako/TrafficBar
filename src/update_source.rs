//! Velopack update sources for this app. GitHub uses the crate's built-in source;
//! Gitee has no upstream helper, so we mirror the GitHub Releases feed flow against
//! Gitee Open API v5.

use std::path::Path;
use std::sync::mpsc::Sender;

use serde::Deserialize;
use velopack::bundle::Manifest;
use velopack::download::{self, HttpHeader, HttpOptions};
use velopack::sources::UpdateSource;
use velopack::{Error, VelopackAsset, VelopackAssetFeed};

#[derive(Deserialize)]
struct GiteeRelease {
    name: Option<String>,
    #[serde(default)]
    prerelease: bool,
    created_at: Option<String>,
    #[serde(default)]
    assets: Vec<GiteeReleaseAsset>,
}

#[derive(Deserialize)]
struct GiteeReleaseAsset {
    name: Option<String>,
    browser_download_url: Option<String>,
}

/// Reads Velopack packages attached to Gitee Releases (`releases.{channel}.json` + nupkg).
#[derive(Clone)]
pub struct GiteeSource {
    owner: String,
    repo: String,
}

impl GiteeSource {
    /// `repo_url` is a page like `https://gitee.com/owner/repo` (not the API URL).
    pub fn new(repo_url: &str) -> Result<Self, Error> {
        let url = url::Url::parse(repo_url.trim_end_matches('/')).map_err(|e| Error::Other(e.to_string()))?;
        let mut segments = url
            .path_segments()
            .ok_or_else(|| Error::Other("Gitee URL has no path".into()))?
            .filter(|s| !s.is_empty());
        let owner = segments
            .next()
            .ok_or_else(|| Error::Other("Gitee URL missing owner".into()))?
            .to_string();
        let repo = segments
            .next()
            .ok_or_else(|| Error::Other("Gitee URL missing repo".into()))?
            .to_string();
        Ok(Self { owner, repo })
    }

    fn releases_url(&self) -> String {
        format!(
            "https://gitee.com/api/v5/repos/{}/{}/releases?page=1&per_page=10",
            self.owner, self.repo
        )
    }

    fn get_releases(&self) -> Result<Vec<GiteeRelease>, Error> {
        let options = HttpOptions {
            Headers: vec![HttpHeader {
                Name: "User-Agent".into(),
                Value: format!("TrafficBar/{}", env!("CARGO_PKG_VERSION")),
            }],
            TimeoutMilliseconds: 60_000,
        };
        let json = download::download_url_as_string(&self.releases_url(), Some(&options))?;
        let mut releases: Vec<GiteeRelease> = serde_json::from_str(&json)?;
        releases.retain(|r| !r.prerelease);
        releases.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(releases)
    }

    fn asset_url(release: &GiteeRelease, asset_name: &str) -> Result<String, Error> {
        let release_name = release.name.as_deref().unwrap_or("unknown");
        let asset = release
            .assets
            .iter()
            .find(|a| a.name.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(asset_name)))
            .ok_or_else(|| {
                Error::Other(format!(
                    "Could not find asset '{}' in Gitee Release '{}'.",
                    asset_name, release_name
                ))
            })?;
        asset
            .browser_download_url
            .clone()
            .ok_or_else(|| Error::Other(format!("Asset '{}' has no download URL.", asset_name)))
    }
}

impl UpdateSource for GiteeSource {
    fn get_release_feed(&self, channel: &str, _app: &Manifest, _staged_user_id: &str) -> Result<VelopackAssetFeed, Error> {
        let releases = self.get_releases()?;
        let releases_file = format!("releases.{channel}.json");
        let options = HttpOptions {
            Headers: vec![HttpHeader {
                Name: "User-Agent".into(),
                Value: format!("TrafficBar/{}", env!("CARGO_PKG_VERSION")),
            }],
            TimeoutMilliseconds: 60_000,
        };
        let mut all_assets = Vec::new();
        for release in &releases {
            let Ok(url) = Self::asset_url(release, &releases_file) else {
                continue;
            };
            let Ok(json) = download::download_url_as_string(&url, Some(&options)) else {
                continue;
            };
            if let Ok(feed) = serde_json::from_str::<VelopackAssetFeed>(&json) {
                all_assets.extend(feed.Assets);
            }
        }
        Ok(VelopackAssetFeed { Assets: all_assets })
    }

    fn download_release_entry(
        &self,
        asset: &VelopackAsset,
        local_file: &Path,
        progress_sender: Option<Sender<i16>>,
    ) -> Result<(), Error> {
        let releases = self.get_releases()?;
        let options = HttpOptions {
            Headers: vec![HttpHeader {
                Name: "User-Agent".into(),
                Value: format!("TrafficBar/{}", env!("CARGO_PKG_VERSION")),
            }],
            TimeoutMilliseconds: 0,
        };
        for release in &releases {
            if let Ok(url) = Self::asset_url(release, &asset.FileName) {
                return download::download_url_to_file(&url, local_file, Some(&options), |p| {
                    if let Some(tx) = &progress_sender {
                        let _ = tx.send(p);
                    }
                });
            }
        }
        Err(Error::Other(format!(
            "Could not find asset '{}' in any Gitee release.",
            asset.FileName
        )))
    }
}
