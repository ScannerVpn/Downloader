use crate::models::download::{DownloadOverrides, FormatOptions};
use crate::models::DownloadItem;
use crate::models::SubtitleInventory;
use crate::runners::aparatkids::{is_aparatkids_url, resolve_aparatkids_url};
use crate::runners::template_context::TemplateContext;
use crate::runners::ytdlp_download::{run_ytdlp_download, YtdlpDownloadError};
use crate::scheduling::concurrency::DynamicSemaphore;
use crate::scheduling::dispatcher::{DispatchEntry, DispatchRequest, GenericDispatcher};
use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::{Arc, Mutex};
use tauri::AppHandle;
use tokio::sync::mpsc::UnboundedSender;

#[derive(Clone)]
pub struct DownloadSender(pub UnboundedSender<DispatchRequest<DownloadRequest>>);

#[derive(Clone)]
pub enum DownloadRequest {
  Batch {
    group_id: String,
    items: Vec<DownloadItem>,
  },
}

#[derive(Clone)]
pub struct DownloadEntry {
  pub group_id: String,
  pub id: String,
  pub url: String,
  pub format: FormatOptions,
  pub subtitle_inventory: Option<SubtitleInventory>,
  pub overrides: Option<DownloadOverrides>,
  pub template_context: TemplateContext,
}

impl From<(DownloadItem, String)> for DownloadEntry {
  fn from(item: (DownloadItem, String)) -> Self {
    Self {
      group_id: item.1,
      id: item.0.id,
      url: item.0.url,
      format: item.0.format,
      subtitle_inventory: item.0.subtitle_inventory,
      overrides: item.0.overrides,
      template_context: item.0.template_context,
    }
  }
}

impl DispatchEntry for DownloadEntry {
  fn group_id(&self) -> &String {
    &self.group_id
  }
  fn group_key(&self) -> Option<&String> {
    self.template_context.values.get("playlist_id")
  }
  fn set_numbering(&mut self, autonumber: u64, group_autonumber: Option<u64>) {
    self
      .template_context
      .values
      .insert("autonumber".to_string(), autonumber.to_string());
    if let Some(group_autonumber) = group_autonumber {
      self.template_context.values.insert(
        "playlist_autonumber".to_string(),
        group_autonumber.to_string(),
      );
    }
  }
}

static DOWNLOAD_COUNTERS: LazyLock<Mutex<HashMap<String, usize>>> =
  LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn setup_download_dispatcher(
  app: &AppHandle,
  sem: Arc<DynamicSemaphore>,
) -> GenericDispatcher<DownloadRequest> {
  GenericDispatcher::start(
    app.clone(),
    sem,
    |req: DownloadRequest| match req {
      DownloadRequest::Batch { group_id, items } => {
        let total = items.len();
        DOWNLOAD_COUNTERS
          .lock()
          .unwrap()
          .insert(group_id.clone(), total);
        items
          .into_iter()
          .map(|item| DownloadEntry::from((item, group_id.clone())))
          .collect()
      }
    },
    |tx, app: AppHandle, entry: DownloadEntry| async move {
      // Resolve AparatKids/Aparat URLs to direct m3u8 links before passing to yt-dlp
      let resolved_url = if is_aparatkids_url(&entry.url) {
        let resolve_result = tokio::time::timeout(
          std::time::Duration::from_secs(35),
          resolve_aparatkids_url(&entry.url),
        )
        .await;
        match resolve_result {
          Ok(Ok(resolved)) => {
            tracing::info!(
              original = %entry.url,
              resolved = %resolved.m3u8_url,
              "Resolved AparatKids URL to m3u8 for download"
            );
            resolved.m3u8_url
          }
          Ok(Err(e)) => {
            tracing::warn!(
              url = %entry.url,
              error = %e,
              "Failed to resolve AparatKids URL for download, using original"
            );
            entry.url.clone()
          }
          Err(_) => {
            tracing::warn!(
              url = %entry.url,
              "Timeout resolving AparatKids URL for download, using original"
            );
            entry.url.clone()
          }
        }
      } else {
        entry.url.clone()
      };

      let mut entry = entry;
      entry.url = resolved_url;

      tracing::info!("starting download id={} url={}", entry.id, entry.url);

      if let Err(e) = run_ytdlp_download(app.clone(), entry.clone()).await {
        tracing::warn!(
          download_id = %entry.id,
          group_id = %entry.group_id,
          error = %e,
          "Failed to run ytdlp download",
        );
        if should_report_to_sentry(&e) {
          sentry::capture_error(&e);
        }
      }

      let mut counters = DOWNLOAD_COUNTERS.lock().unwrap();
      if let Some(cnt) = counters.get_mut(&entry.group_id) {
        *cnt -= 1;
        if *cnt == 0 {
          counters.remove(&entry.group_id);
          let _ = tx.send(DispatchRequest::Cleanup {
            group_id: entry.group_id.clone(),
          });
        }
      }
    },
  )
}

fn should_report_to_sentry(err: &YtdlpDownloadError) -> bool {
  matches!(
    err,
    YtdlpDownloadError::SpawnFailed(_)
      | YtdlpDownloadError::InvalidDiagnosticRules(_)
      | YtdlpDownloadError::EventStreamEnded
  )
}
