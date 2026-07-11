use crate::models::download::{DownloadOverrides, FormatOptions};
use crate::models::payloads::MediaAddWithFormatPayload;
use crate::models::{MediaAddPayload, MediaFatalPayload, PlaylistEntry};
use crate::runners::aparatkids::{
  aparat_playlist_id, fetch_aparat_playlist, is_aparatkids_url, resolve_aparatkids_url,
  AparatkidsResolved,
};
use crate::runners::ytdlp_info::{run_ytdlp_info_fetch, YtdlpInfoFetchError};
use crate::{
  models::{ParsedMedia, ParsedPlaylist},
  scheduling::concurrency::DynamicSemaphore,
  scheduling::dispatcher::{DispatchEntry, DispatchRequest, GenericDispatcher},
};
use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

#[derive(Clone)]
pub struct FetchSender(pub UnboundedSender<DispatchRequest<FetchRequest>>);

#[derive(Clone)]
pub enum FetchRequest {
  Initial {
    group_id: String,
    id: String,
    url: String,
    overrides: Box<Option<DownloadOverrides>>,
  },
  Playlist {
    group_id: String,
    entries: Vec<PlaylistEntry>,
    overrides: Box<Option<DownloadOverrides>>,
  },
  Size {
    group_id: String,
    id: String,
    url: String,
    format: FormatOptions,
  },
  SizePlaylist {
    group_id: String,
    playlist: ParsedPlaylist,
    format: FormatOptions,
  },
}

#[derive(Clone)]
pub struct FetchEntry {
  pub group_id: String,
  pub id: String,
  pub url: String,
  pub total: usize,
  pub format: Option<FormatOptions>,
  pub overrides: Option<DownloadOverrides>,
}

impl DispatchEntry for FetchEntry {
  fn group_id(&self) -> &String {
    &self.group_id
  }
  fn group_key(&self) -> Option<&String> {
    None
  }
  fn set_numbering(&mut self, _autonumber: u64, _group_autonumber: Option<u64>) {}
}

static GROUP_COUNTERS: LazyLock<Mutex<HashMap<String, usize>>> =
  LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn setup_fetch_dispatcher(
  app: &AppHandle,
  sem: Arc<DynamicSemaphore>,
) -> GenericDispatcher<FetchRequest> {
  GenericDispatcher::start(
    app.clone(),
    sem,
    expand_fetch_request,
    |tx: UnboundedSender<DispatchRequest<FetchRequest>>, app: AppHandle, entry: FetchEntry| async move {
      handle_fetch_entry(tx, app, entry).await;
    },
  )
}

fn expand_fetch_request(req: FetchRequest) -> Vec<FetchEntry> {
  match req {
    FetchRequest::Initial {
      group_id,
      id,
      url,
      overrides,
    } => {
      vec![FetchEntry {
        group_id,
        id,
        url,
        total: 1,
        format: None,
        overrides: *overrides,
      }]
    }
    FetchRequest::Playlist {
      group_id,
      entries,
      overrides,
    } => {
      let total = entries.len();
      GROUP_COUNTERS
        .lock()
        .unwrap()
        .insert(group_id.clone(), total);
      entries
        .into_iter()
        .map(|e| FetchEntry {
          group_id: group_id.clone(),
          id: Uuid::new_v4().to_string(),
          url: e.video_url,
          total,
          format: None,
          overrides: *overrides.clone(),
        })
        .collect()
    }
    FetchRequest::Size {
      group_id,
      id,
      url,
      format,
    } => {
      vec![FetchEntry {
        group_id,
        id,
        url,
        total: 1,
        format: Some(format),
        overrides: None,
      }]
    }
    FetchRequest::SizePlaylist {
      group_id,
      playlist,
      format,
    } => {
      let total = playlist.entries.len();
      playlist
        .entries
        .into_iter()
        .map(|e| FetchEntry {
          group_id: group_id.clone(),
          id: Uuid::new_v4().to_string(),
          url: e.video_url,
          total,
          format: Some(format.clone()),
          overrides: None,
        })
        .collect()
    }
  }
}

async fn handle_fetch_entry(
  tx: UnboundedSender<DispatchRequest<FetchRequest>>,
  app: AppHandle,
  entry: FetchEntry,
) {
  let FetchEntry {
    group_id,
    id,
    url,
    total,
    format,
    overrides,
  } = entry.clone();

  // Check if this is an aparat playlist URL and expand it
  if is_aparatkids_url(&url) {
    if let Some(playlist_id) = aparat_playlist_id(&url) {
      tracing::info!(url = %url, playlist_id = %playlist_id, "Detected aparat playlist URL, fetching playlist entries");
      match tokio::time::timeout(
        std::time::Duration::from_secs(35),
        fetch_aparat_playlist(&playlist_id, &url),
      )
      .await
      {
        Ok(Ok(video_urls)) => {
          let entries: Vec<PlaylistEntry> = video_urls
            .into_iter()
            .enumerate()
            .map(|(idx, video_url)| PlaylistEntry {
              video_url,
              index: idx,
            })
            .collect();
          let total = entries.len();
          tracing::info!(playlist_id = %playlist_id, count = total, "Fetched aparat playlist entries");

          // Emit a playlist media_add event so the frontend shows playlist selection UI
          let playlist = crate::models::ParsedPlaylist {
            id: id.clone(),
            url: Some(url.clone()),
            title: None,
            thumbnail: None,
            uploader: None,
            uploader_id: None,
            entries,
            playlist_id: Some(playlist_id.clone()),
            playlist_count: Some(total as u64),
          };

          let payload = MediaAddPayload {
            group_id: group_id.clone(),
            total,
            item: playlist,
          };
          let _ = app.emit("media_add", payload);

          let mut counters = GROUP_COUNTERS.lock().unwrap();
          if let Some(cnt) = counters.get_mut(&group_id) {
            *cnt = 0;
            counters.remove(&group_id);
            let _ = tx.send(DispatchRequest::Cleanup {
              group_id: group_id.clone(),
            });
          }
          return;
        }
        Ok(Err(e)) => {
          tracing::warn!(
            url = %url,
            playlist_id = %playlist_id,
            error = %e,
            "Failed to fetch aparat playlist, falling back to single video"
          );
        }
        Err(_) => {
          tracing::warn!(
            url = %url,
            playlist_id = %playlist_id,
            "Timeout fetching aparat playlist, falling back to single video"
          );
        }
      }
    }
  }

  // Resolve AparatKids/Aparat URLs to direct m3u8 links before passing to yt-dlp
  let aparatkids_meta: Option<AparatkidsResolved> = if is_aparatkids_url(&url) {
    let resolve_result = tokio::time::timeout(
      std::time::Duration::from_secs(35),
      resolve_aparatkids_url(&url),
    )
    .await;
    match resolve_result {
      Ok(Ok(resolved)) => {
        tracing::info!(original = %url, resolved = %resolved.m3u8_url, "Resolved AparatKids URL to m3u8");
        Some(resolved)
      }
      Ok(Err(e)) => {
        tracing::warn!(
          url = %url,
          error = %e,
          "Failed to resolve AparatKids URL"
        );
        let _ = app.emit(
          "media_fatal",
          MediaFatalPayload::with_exit(
            group_id.clone(),
            id.clone(),
            1,
            format!("Failed to resolve AparatKids video: {e}"),
          ),
        );
        return;
      }
      Err(_) => {
        tracing::warn!(
          url = %url,
          "Timeout resolving AparatKids URL"
        );
        let _ = app.emit(
          "media_fatal",
          MediaFatalPayload::with_exit(
            group_id.clone(),
            id.clone(),
            1,
            "Timeout resolving AparatKids video URL".to_string(),
          ),
        );
        return;
      }
    }
  } else {
    None
  };

  let resolved_url = aparatkids_meta
    .as_ref()
    .map(|m| m.m3u8_url.clone())
    .unwrap_or_else(|| url.clone());

  // For AparatKids URLs, build metadata directly from API — skip yt-dlp entirely
  // to avoid hanging on m3u8 stream URLs
  if let Some(ref meta) = aparatkids_meta {
    let single = crate::models::ParsedSingleVideo {
      id: id.clone(),
      url: Some(url.clone()),
      title: meta.title.clone(),
      thumbnail: meta.thumbnail.clone(),
      description: None,
      uploader_id: None,
      uploader: None,
      views: None,
      comments: None,
      likes: None,
      dislikes: None,
      duration: meta.duration,
      rating: None,
      extractor: Some("aparatkids".to_string()),
      video_codecs: vec![],
      audio_codecs: vec![],
      video_tracks: vec![],
      audio_tracks: vec![],
      formats: vec![],
      subtitle_inventory: crate::models::SubtitleInventory::default(),
      chapters: vec![],
      filesize: meta.filesize,
    };

    if let Some(fmt) = format {
      let payload = MediaAddWithFormatPayload {
        group_id: group_id.clone(),
        total,
        item: single,
        format: fmt,
      };
      let _ = app.emit("media_size", payload);
    } else {
      let payload = MediaAddPayload {
        group_id: group_id.clone(),
        total,
        item: single,
      };
      let _ = app.emit("media_add", payload);
    }

    let mut counters = GROUP_COUNTERS.lock().unwrap();
    if let Some(cnt) = counters.get_mut(&group_id) {
      *cnt -= 1;
      if *cnt == 0 {
        counters.remove(&group_id);
        let _ = tx.send(DispatchRequest::Cleanup {
          group_id: group_id.clone(),
        });
      }
    }
    return;
  }

  let result = run_ytdlp_info_fetch(
    &app,
    id.clone(),
    group_id.clone(),
    &resolved_url,
    format.clone(),
    overrides.clone(),
  )
  .await;

  let result = match result {
    Ok(v) => v,
    Err(e) => {
      tracing::warn!(
        fetch_id = %id,
        group_id = %group_id,
        url = %url,
        error = %e,
        "run_ytdlp_info_fetch failed"
      );
      if should_report_to_sentry(&e) {
        sentry::capture_error(&e);
      }

      None
    }
  };

  match result {
    Some(ParsedMedia::Single(mut single)) => {
      // Override metadata from AparatKids page if available
      if let Some(ref meta) = aparatkids_meta {
        if meta.title.is_some() {
          single.title = meta.title.clone();
        }
        if meta.thumbnail.is_some() {
          single.thumbnail = meta.thumbnail.clone();
        }
        if meta.duration.is_some() {
          single.duration = meta.duration;
        }
        if meta.filesize.is_some() {
          single.filesize = meta.filesize;
        }
        // Always use the original page URL for display
        single.url = Some(url.clone());
      }

      if let Some(format) = format {
        let payload = MediaAddWithFormatPayload {
          group_id: group_id.clone(),
          total,
          item: single,
          format,
        };
        let _ = app.emit("media_size", payload);
      } else {
        let payload = MediaAddPayload {
          group_id: group_id.clone(),
          total,
          item: single,
        };
        let _ = app.emit("media_add", payload);
      }

      let mut counters = GROUP_COUNTERS.lock().unwrap();
      if let Some(cnt) = counters.get_mut(&group_id) {
        *cnt -= 1;
        if *cnt == 0 {
          counters.remove(&group_id);
          let _ = tx.send(DispatchRequest::Cleanup {
            group_id: group_id.clone(),
          });
        }
      }
    }
    Some(ParsedMedia::Playlist(pl)) => {
      if let Some(format) = format {
        let _ = tx.send(DispatchRequest::Pipeline(FetchRequest::SizePlaylist {
          group_id: group_id.clone(),
          playlist: pl,
          format,
        }));
      } else {
        let payload = MediaAddPayload {
          group_id,
          total: pl.entries.len(),
          item: pl,
        };
        let _ = app.emit("media_add", payload);
      }
    }
    Some(ParsedMedia::Livestream(_)) => {
      let payload =
        MediaFatalPayload::internal(group_id.clone(), id, "Livestreams unsupported".into(), None);
      let _ = app.emit("media_fatal", payload);
      let mut counters = GROUP_COUNTERS.lock().unwrap();
      if let Some(cnt) = counters.get_mut(&group_id) {
        *cnt -= 1;
        if *cnt == 0 {
          counters.remove(&group_id);
          let _ = tx.send(DispatchRequest::Cleanup {
            group_id: group_id.clone(),
          });
        }
      }
    }
    None => {
      // Do nothing if no parsed result is returned. The events have already been sent.
    }
  }
}

fn should_report_to_sentry(err: &YtdlpInfoFetchError) -> bool {
  matches!(
    err,
    YtdlpInfoFetchError::InvalidDiagnosticRules(_)
      | YtdlpInfoFetchError::RunnerFailed(_)
      | YtdlpInfoFetchError::ParseFailed(_)
  )
}
