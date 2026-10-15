//! HTTP client for the HyperDeck REST API.
//!
//! The manual documents paths such as `/transports/0` without a prefix. Devices
//! serve them under `/control/api/v1`. [`RestClient::new`] joins that prefix
//! unless you pass a base URL that already ends at the API root.

use hyperdeck_core::rest::{
    ActiveMedia, AudioFormat, Bookmark, Clip, CodecFormat, DeviceState, NasHost, PlaybackState,
    ProductInfo, RecordState, TimecodePair, Timeline, TransportMode, VideoFormatInfo, WorkingSet,
};
use reqwest::StatusCode;

/// Error from a REST call.
#[derive(Debug)]
pub enum RestError {
    /// Transport failure.
    Http(reqwest::Error),
    /// The deck returned a status this method does not treat as success.
    Status(StatusCode),
}

impl core::fmt::Display for RestError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Http(err) => write!(f, "{err}"),
            Self::Status(code) => write!(f, "unexpected HTTP status {code}"),
        }
    }
}

impl std::error::Error for RestError {}

/// Client for one deck.
pub struct RestClient {
    http: reqwest::Client,
    base: String,
}

impl RestClient {
    /// `host` may be `192.168.1.10` or a full `http://.../control/api/v1` URL.
    pub fn new(host: &str) -> Self {
        let base = if host.starts_with("http://") || host.starts_with("https://") {
            host.trim_end_matches('/').to_string()
        } else {
            format!("http://{host}/control/api/v1")
        };
        Self {
            http: reqwest::Client::new(),
            base,
        }
    }

    /// `GET /transports/0`
    pub async fn transport(&self) -> Result<TransportMode, RestError> {
        self.get_json("/transports/0").await
    }

    /// `PUT /transports/0`
    pub async fn set_transport_mode(&self, mode: &str) -> Result<(), RestError> {
        self.put_json("/transports/0", &serde_json::json!({ "mode": mode }))
            .await
    }

    /// `POST /transports/0/stop`
    pub async fn stop(&self) -> Result<(), RestError> {
        self.post_empty("/transports/0/stop").await
    }

    /// `POST /transports/0/play`
    pub async fn play(&self) -> Result<(), RestError> {
        self.post_empty("/transports/0/play").await
    }

    /// `GET /transports/0/playback`
    pub async fn playback(&self) -> Result<PlaybackState, RestError> {
        self.get_json("/transports/0/playback").await
    }

    /// `PUT /transports/0/playback`
    pub async fn set_playback(&self, state: &PlaybackState) -> Result<(), RestError> {
        self.put_json("/transports/0/playback", state).await
    }

    /// `POST /transports/0/record`
    pub async fn record(&self, clip_name: Option<&str>) -> Result<(), RestError> {
        let body = match clip_name {
            Some(name) => serde_json::json!({ "clipName": name }),
            None => serde_json::json!({}),
        };
        self.post_json("/transports/0/record", &body).await
    }

    /// `GET /transports/0/record`
    pub async fn record_state(&self) -> Result<RecordState, RestError> {
        self.get_json("/transports/0/record").await
    }

    /// `GET /transports/0/timecode`
    pub async fn timecode(&self) -> Result<TimecodePair, RestError> {
        self.get_json("/transports/0/timecode").await
    }

    /// `GET /system/product`
    pub async fn product(&self) -> Result<ProductInfo, RestError> {
        self.get_json("/system/product").await
    }

    /// `GET /system/codecFormat`
    pub async fn codec_format(&self) -> Result<CodecFormat, RestError> {
        self.get_json("/system/codecFormat").await
    }

    /// `PUT /system/codecFormat`
    pub async fn set_codec_format(&self, format: &CodecFormat) -> Result<(), RestError> {
        self.put_json("/system/codecFormat", format).await
    }

    /// `GET /system/supportedCodecFormats` returns the raw JSON array wrapper.
    pub async fn supported_codecs(&self) -> Result<serde_json::Value, RestError> {
        self.get_json("/system/supportedCodecFormats").await
    }

    /// `GET /system/videoFormat`
    pub async fn video_format(&self) -> Result<Option<VideoFormatInfo>, RestError> {
        self.get_json("/system/videoFormat").await
    }

    /// `GET /system/supportedVideoFormats`
    pub async fn supported_video_formats(&self) -> Result<serde_json::Value, RestError> {
        self.get_json("/system/supportedVideoFormats").await
    }

    /// `POST /system/reboot`
    pub async fn reboot(&self) -> Result<(), RestError> {
        self.post_empty("/system/reboot").await
    }

    /// `GET /media/workingset`
    pub async fn working_set(&self) -> Result<WorkingSet, RestError> {
        self.get_json("/media/workingset").await
    }

    /// `GET /media/active`
    pub async fn active_media(&self) -> Result<Option<ActiveMedia>, RestError> {
        match self.http.get(self.url("/media/active")).send().await {
            Ok(response) if response.status() == StatusCode::NO_CONTENT => Ok(None),
            Ok(response) if response.status().is_success() => {
                response.json().await.map(Some).map_err(RestError::Http)
            }
            Ok(response) => Err(RestError::Status(response.status())),
            Err(err) => Err(RestError::Http(err)),
        }
    }

    /// `PUT /media/active`
    pub async fn set_active_media(&self, workingset_index: u32) -> Result<(), RestError> {
        self.put_json(
            "/media/active",
            &serde_json::json!({ "workingsetIndex": workingset_index }),
        )
        .await
    }

    /// `GET /media/devices/{name}`
    pub async fn device(&self, name: &str) -> Result<DeviceState, RestError> {
        self.get_json(&format!("/media/devices/{name}")).await
    }

    /// `GET /clips`
    pub async fn clips(&self) -> Result<Vec<Clip>, RestError> {
        let value: serde_json::Value = self.get_json("/clips").await?;
        Ok(
            serde_json::from_value(value.get("clips").cloned().unwrap_or(value))
                .unwrap_or_default(),
        )
    }

    /// `GET /clips/devices/{name}`
    pub async fn clips_on(&self, device: &str) -> Result<Vec<Clip>, RestError> {
        let value: serde_json::Value = self.get_json(&format!("/clips/devices/{device}")).await?;
        Ok(
            serde_json::from_value(value.get("clips").cloned().unwrap_or(value))
                .unwrap_or_default(),
        )
    }

    /// `GET /timelines/0`
    pub async fn timeline(&self) -> Result<Timeline, RestError> {
        self.get_json("/timelines/0").await
    }

    /// `POST /timelines/0/clear`
    pub async fn clear_timeline(&self) -> Result<(), RestError> {
        self.post_empty("/timelines/0/clear").await
    }

    /// `POST /timelines/0` with a list of clip ids appended at the end.
    pub async fn add_clips(&self, clip_ids: &[u32]) -> Result<(), RestError> {
        self.post_json("/timelines/0", &serde_json::json!({ "clips": clip_ids }))
            .await
    }

    /// `DELETE /timelines/0/clips/{index}`
    pub async fn remove_timeline_clip(&self, index: u32) -> Result<(), RestError> {
        self.delete(&format!("/timelines/0/clips/{index}")).await
    }

    /// `GET /audio/recordFormat`
    pub async fn audio_format(&self) -> Result<AudioFormat, RestError> {
        self.get_json("/audio/recordFormat").await
    }

    /// `PUT /audio/recordFormat`
    pub async fn set_audio_format(&self, format: &AudioFormat) -> Result<(), RestError> {
        self.put_json("/audio/recordFormat", format).await
    }

    /// `GET /media/nas/bookmarks`
    pub async fn nas_bookmarks(&self) -> Result<Vec<Bookmark>, RestError> {
        let value: serde_json::Value = self.get_json("/media/nas/bookmarks").await?;
        Ok(
            serde_json::from_value(value.get("bookmarks").cloned().unwrap_or(value))
                .unwrap_or_default(),
        )
    }

    /// `POST /media/nas/bookmarks`
    pub async fn add_nas_bookmark(
        &self,
        url: &str,
        username: Option<&str>,
        password: Option<&str>,
    ) -> Result<(), RestError> {
        self.post_json(
            "/media/nas/bookmarks",
            &serde_json::json!({
                "url": url,
                "username": username.unwrap_or("guest"),
                "password": password.unwrap_or(""),
            }),
        )
        .await
    }

    /// `DELETE /media/nas/bookmarks/{url}` with slashes encoded as `%2F`.
    pub async fn delete_nas_bookmark(&self, url: &str) -> Result<(), RestError> {
        self.delete(&format!("/media/nas/bookmarks/{}", encode_nas_url(url)))
            .await
    }

    /// `GET /media/nas/discovered`
    pub async fn nas_discovered(&self) -> Result<Vec<NasHost>, RestError> {
        let value: serde_json::Value = self.get_json("/media/nas/discovered").await?;
        Ok(
            serde_json::from_value(value.get("hosts").cloned().unwrap_or(value))
                .unwrap_or_default(),
        )
    }

    /// `GET /event/list`
    pub async fn events(&self) -> Result<Vec<String>, RestError> {
        let value: serde_json::Value = self.get_json("/event/list").await?;
        Ok(
            serde_json::from_value(value.get("events").cloned().unwrap_or(value))
                .unwrap_or_default(),
        )
    }

    /// `GET /monitoring/display`
    pub async fn displays(&self) -> Result<Vec<String>, RestError> {
        let value: serde_json::Value = self.get_json("/monitoring/display").await?;
        Ok(
            serde_json::from_value(value.get("displays").cloned().unwrap_or(value))
                .unwrap_or_default(),
        )
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, RestError> {
        let response = self
            .http
            .get(self.url(path))
            .send()
            .await
            .map_err(RestError::Http)?;
        self.json(response).await
    }

    async fn put_json<T: serde::Serialize>(&self, path: &str, body: &T) -> Result<(), RestError> {
        let response = self
            .http
            .put(self.url(path))
            .json(body)
            .send()
            .await
            .map_err(RestError::Http)?;
        self.expect_success(response).await
    }

    async fn post_json<T: serde::Serialize>(&self, path: &str, body: &T) -> Result<(), RestError> {
        let response = self
            .http
            .post(self.url(path))
            .json(body)
            .send()
            .await
            .map_err(RestError::Http)?;
        self.expect_success(response).await
    }

    async fn post_empty(&self, path: &str) -> Result<(), RestError> {
        let response = self
            .http
            .post(self.url(path))
            .send()
            .await
            .map_err(RestError::Http)?;
        self.expect_success(response).await
    }

    async fn delete(&self, path: &str) -> Result<(), RestError> {
        let response = self
            .http
            .delete(self.url(path))
            .send()
            .await
            .map_err(RestError::Http)?;
        self.expect_success(response).await
    }

    async fn expect_success(&self, response: reqwest::Response) -> Result<(), RestError> {
        if response.status().is_success() {
            Ok(())
        } else {
            Err(RestError::Status(response.status()))
        }
    }

    async fn json<T: serde::de::DeserializeOwned>(
        &self,
        response: reqwest::Response,
    ) -> Result<T, RestError> {
        if !response.status().is_success() {
            return Err(RestError::Status(response.status()));
        }
        response.json().await.map_err(RestError::Http)
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }
}

/// Encodes `/` as `%2F` without encoding the scheme's other characters twice.
///
/// The manual requires slashes in a NAS URL path parameter to be `%2F`.
pub fn encode_nas_url(url: &str) -> String {
    url.replace('/', "%2F")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nas_slashes_are_encoded() {
        assert_eq!(
            encode_nas_url("smb://host/Share/folder"),
            "smb:%2F%2Fhost%2FShare%2Ffolder"
        );
    }
}
