use std::collections::HashMap;

use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

static APARATKIDS_URL_RE: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"https?://(?:www\.)?aparatkids\.com/(?:w|m)/([a-zA-Z0-9]+)").unwrap()
});

static APARAT_VIDEO_RE: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"https?://(?:www\.)?aparat\.com/v/([a-zA-Z0-9]+)").unwrap());

static APARAT_MOVIE_RE: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"https?://(?:www\.)?aparat\.com/m/([a-zA-Z0-9]+)").unwrap());

static APARAT_SHORTS_RE: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"https?://(?:www\.)?aparat\.com/shorts/(\d+)").unwrap());

static APARAT_PLAYLIST_PARAM_RE: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"[?&]playlist=(\d+)").unwrap());

pub struct AparatkidsResolved {
  pub m3u8_url: String,
  pub title: Option<String>,
  pub thumbnail: Option<String>,
  pub duration: Option<f64>,
  pub filesize: Option<u64>,
}

pub fn is_aparatkids_url(url: &str) -> bool {
  APARATKIDS_URL_RE.is_match(url)
    || APARAT_VIDEO_RE.is_match(url)
    || APARAT_MOVIE_RE.is_match(url)
    || APARAT_SHORTS_RE.is_match(url)
}

/// Check if a URL has a playlist query parameter
pub fn aparat_playlist_id(url: &str) -> Option<String> {
  APARAT_PLAYLIST_PARAM_RE
    .captures(url)
    .and_then(|caps| caps.get(1))
    .map(|m| m.as_str().to_string())
}

pub async fn resolve_aparatkids_url(url: &str) -> Result<AparatkidsResolved, String> {
  let client = reqwest::Client::builder()
    .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
    .build()
    .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

  if APARAT_SHORTS_RE.is_match(url) {
    resolve_aparat_shorts(&client, url).await
  } else if APARAT_VIDEO_RE.is_match(url) {
    resolve_aparat_video(&client, url).await
  } else if APARAT_MOVIE_RE.is_match(url) {
    resolve_aparat_movie(&client, url).await
  } else {
    resolve_aparatkids_com(&client, url).await
  }
}

/// Resolve aparat.com /v/ video URLs via their JSON API
async fn resolve_aparat_video(
  client: &reqwest::Client,
  url: &str,
) -> Result<AparatkidsResolved, String> {
  let caps = APARAT_VIDEO_RE
    .captures(url)
    .ok_or_else(|| "Invalid aparat.com video URL".to_string())?;
  let video_hash = caps.get(1).unwrap().as_str();

  let api_url = format!(
    "https://www.aparat.com/api/fa/v1/video/video/show/videohash/{}?pr=1&mf=1",
    video_hash
  );

  let resp = client
    .get(&api_url)
    .header("Referer", "https://www.aparat.com/")
    .header("Accept", "application/json")
    .send()
    .await
    .map_err(|e| format!("Failed to fetch video API: {e}"))?;

  let status = resp.status();
  if !status.is_success() {
    return Err(format!("Video API returned status {}", status));
  }

  let json: Value = resp
    .json()
    .await
    .map_err(|e| format!("Failed to parse video API response: {e}"))?;

  let attrs = json
    .get("data")
    .and_then(|d| d.get("attributes"))
    .ok_or_else(|| "Invalid API response: missing video attributes".to_string())?;

  let m3u8_url = extract_aparat_m3u8(attrs)?;

  let title = attrs
    .get("title")
    .and_then(|v| v.as_str())
    .map(|s| s.to_string());

  let thumbnail = attrs
    .get("big_poster")
    .or_else(|| attrs.get("medium_poster"))
    .or_else(|| attrs.get("small_poster"))
    .and_then(|v| v.as_str())
    .map(|s| s.to_string());

  let duration = attrs
    .get("duration")
    .and_then(|v| v.as_f64())
    .filter(|&d| d > 0.0);

  // Try to extract filesize from API response
  let filesize = attrs
    .get("filesize")
    .or_else(|| attrs.get("size"))
    .and_then(|v| v.as_u64())
    .filter(|&s| s > 0)
    .or_else(|| {
      // Some APIs return duration in seconds and bitrate, estimate filesize
      let dur = duration?;
      let bitrate = attrs.get("bitrate").and_then(|v| v.as_u64()).or_else(|| {
        attrs
          .get("encodings")
          .and_then(|e| e.as_array())
          .and_then(|arr| arr.first())
          .and_then(|e| e.get("bitrate"))
          .and_then(|v| v.as_u64())
      })?;
      if bitrate > 0 {
        Some((dur * bitrate as f64 / 8.0) as u64)
      } else {
        None
      }
    });

  Ok(AparatkidsResolved {
    m3u8_url,
    title,
    thumbnail,
    duration,
    filesize,
  })
}

/// Resolve aparat.com /m/ movie URLs via their JSON API
async fn resolve_aparat_movie(
  client: &reqwest::Client,
  url: &str,
) -> Result<AparatkidsResolved, String> {
  let caps = APARAT_MOVIE_RE
    .captures(url)
    .ok_or_else(|| "Invalid aparat.com movie URL".to_string())?;
  let movie_hash = caps.get(1).unwrap().as_str();

  let api_url = format!(
    "https://www.aparat.com/api/fa/v1/movie/movie/one/moviehash/{}",
    movie_hash
  );

  let resp = client
    .get(&api_url)
    .header("Referer", "https://www.aparat.com/")
    .header("Accept", "application/json")
    .send()
    .await
    .map_err(|e| format!("Failed to fetch movie API: {e}"))?;

  let status = resp.status();
  if !status.is_success() {
    return Err(format!("Movie API returned status {}", status));
  }

  let json: Value = resp
    .json()
    .await
    .map_err(|e| format!("Failed to parse movie API response: {e}"))?;

  // Navigate to movieData[0]
  let movie_data = json
    .get("data")
    .and_then(|d| d.get("attributes"))
    .and_then(|a| a.get("movieData"))
    .and_then(|m| m.as_array())
    .and_then(|arr| arr.first())
    .ok_or_else(|| "Invalid movie API response: no movie data".to_string())?;

  // Extract m3u8 from playerOption.multiSRC
  let m3u8_url = extract_movie_m3u8(movie_data)?;

  // Extract metadata from General
  let general = movie_data.get("General");
  let title = general
    .and_then(|g| g.get("title"))
    .or_else(|| general.and_then(|g| g.get("title_fa")))
    .and_then(|v| v.as_str())
    .map(|s| s.to_string());

  let thumbnail = general
    .and_then(|g| g.get("cover"))
    .or_else(|| general.and_then(|g| g.get("thumbnails")))
    .and_then(|v| v.as_str())
    .map(|s| s.to_string());

  let duration = movie_data
    .get("playerOption")
    .and_then(|p| p.get("duration"))
    .and_then(|v| v.as_f64())
    .filter(|&d| d > 0.0)
    .or_else(|| {
      general
        .and_then(|g| g.get("duration"))
        .and_then(|v| v.as_f64())
        .filter(|&d| d > 0.0)
    });

  // Try to extract filesize from movie data
  let filesize = movie_data
    .get("playerOption")
    .and_then(|p| p.get("filesize"))
    .or_else(|| movie_data.get("playerOption").and_then(|p| p.get("size")))
    .and_then(|v| v.as_u64())
    .filter(|&s| s > 0);

  Ok(AparatkidsResolved {
    m3u8_url,
    title,
    thumbnail,
    duration,
    filesize,
  })
}

/// Resolve aparat.com /shorts/ URLs - shorts use numeric IDs that can be looked up via the video API
async fn resolve_aparat_shorts(
  client: &reqwest::Client,
  url: &str,
) -> Result<AparatkidsResolved, String> {
  let caps = APARAT_SHORTS_RE
    .captures(url)
    .ok_or_else(|| "Invalid aparat.com shorts URL".to_string())?;
  let shorts_id = caps.get(1).unwrap().as_str();

  // Aparat shorts are videos; try the video API with the numeric ID
  let api_url = format!(
    "https://www.aparat.com/api/fa/v1/video/video/show/videohash/{}?pr=1&mf=1",
    shorts_id
  );

  let resp = client
    .get(&api_url)
    .header("Referer", "https://www.aparat.com/")
    .header("Accept", "application/json")
    .send()
    .await
    .map_err(|e| format!("Failed to fetch shorts API: {e}"))?;

  let status = resp.status();
  if !status.is_success() {
    return Err(format!("Shorts API returned status {}", status));
  }

  let json: Value = resp
    .json()
    .await
    .map_err(|e| format!("Failed to parse shorts API response: {e}"))?;

  let attrs = json
    .get("data")
    .and_then(|d| d.get("attributes"))
    .ok_or_else(|| "Invalid API response: missing video attributes".to_string())?;

  let m3u8_url = extract_aparat_m3u8(attrs)?;

  let title = attrs
    .get("title")
    .and_then(|v| v.as_str())
    .map(|s| s.to_string());

  let thumbnail = attrs
    .get("big_poster")
    .or_else(|| attrs.get("medium_poster"))
    .or_else(|| attrs.get("small_poster"))
    .and_then(|v| v.as_str())
    .map(|s| s.to_string());

  let duration = attrs
    .get("duration")
    .and_then(|v| v.as_f64())
    .filter(|&d| d > 0.0);

  let filesize = attrs
    .get("filesize")
    .or_else(|| attrs.get("size"))
    .and_then(|v| v.as_u64())
    .filter(|&s| s > 0);

  Ok(AparatkidsResolved {
    m3u8_url,
    title,
    thumbnail,
    duration,
    filesize,
  })
}

/// Fetch all video URLs from an aparat playlist by querying the video API
/// The playlist data is embedded in the video API response under relationships.video.data and included
pub async fn fetch_aparat_playlist(
  playlist_id: &str,
  first_video_url: &str,
) -> Result<Vec<String>, String> {
  let client = reqwest::Client::builder()
    .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
    .build()
    .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

  let video_hash = APARAT_VIDEO_RE
    .captures(first_video_url)
    .and_then(|caps| caps.get(1))
    .map(|m| m.as_str())
    .ok_or_else(|| "Cannot extract video hash from URL".to_string())?;

  let api_url = format!(
    "https://www.aparat.com/api/fa/v1/video/video/show/videohash/{}?pr=1&mf=1",
    video_hash
  );

  let resp = client
    .get(&api_url)
    .header("Referer", "https://www.aparat.com/")
    .header("Accept", "application/json")
    .send()
    .await
    .map_err(|e| format!("Failed to fetch video API: {e}"))?;

  let status = resp.status();
  if !status.is_success() {
    return Err(format!("Video API returned status {}", status));
  }

  let json: Value = resp
    .json()
    .await
    .map_err(|e| format!("Failed to parse video API response: {e}"))?;

  // Find the playlist in included and extract video IDs from its relationships
  let video_ids: Vec<String> = json
    .get("included")
    .and_then(|i| i.as_array())
    .and_then(|arr| {
      arr
        .iter()
        .find(|item| item.get("type").and_then(|t| t.as_str()) == Some("playlist"))
    })
    .and_then(|playlist| playlist.get("relationships"))
    .and_then(|r| r.get("video"))
    .and_then(|v| v.get("data"))
    .and_then(|d| d.as_array())
    .map(|arr| {
      arr
        .iter()
        .filter_map(|item| item.get("id").and_then(|v| v.as_str()).map(String::from))
        .collect()
    })
    .unwrap_or_default();

  if video_ids.is_empty() {
    return Err("No playlist entries found in API response".to_string());
  }

  let mut id_to_uid: HashMap<String, String> = HashMap::new();
  if let Some(included) = json.get("included").and_then(|i| i.as_array()) {
    for item in included {
      if let (Some(id), Some(uid)) = (
        item.get("id").and_then(|v| v.as_str()),
        item
          .get("attributes")
          .and_then(|a| a.get("uid"))
          .and_then(|u| u.as_str()),
      ) {
        id_to_uid.insert(id.to_string(), uid.to_string());
      }
    }
  }

  let mut urls: Vec<String> = Vec::new();
  for video_id in &video_ids {
    if let Some(uid) = id_to_uid.get(video_id) {
      urls.push(format!("https://www.aparat.com/v/{}", uid));
    }
  }

  if urls.is_empty() {
    return Err("Could not map playlist video IDs to URLs".to_string());
  }

  tracing::info!(
    playlist_id = %playlist_id,
    count = urls.len(),
    "Resolved aparat playlist entries"
  );
  Ok(urls)
}

fn extract_aparat_m3u8(attrs: &Value) -> Result<String, String> {
  if let Some(hls) = attrs.get("hls_link").and_then(|v| v.as_str()) {
    if !hls.is_empty() {
      return Ok(hls.to_string());
    }
  }

  if let Some(hls_obj) = attrs.get("hls") {
    if let Some(link) = hls_obj.get("link").and_then(|v| v.as_str()) {
      if !link.is_empty() {
        return Ok(link.to_string());
      }
    }
  }

  if let Some(file_link) = attrs.get("file_link").and_then(|v| v.as_str()) {
    if !file_link.is_empty() {
      return Ok(file_link.to_string());
    }
  }

  Err("No video stream found in API response".to_string())
}

fn extract_movie_m3u8(movie_data: &Value) -> Result<String, String> {
  // Try playerOption.multiSRC
  if let Some(player) = movie_data.get("playerOption") {
    if let Some(multi_src) = player.get("multiSRC").and_then(|v| v.as_array()) {
      for src_group in multi_src {
        if let Some(arr) = src_group.as_array() {
          for src_entry in arr {
            if let Some(src_url) = src_entry.get("src").and_then(|v| v.as_str()) {
              if src_url.contains(".m3u8") || src_url.contains("m3u8") {
                return Ok(src_url.to_string());
              }
            }
          }
        }
      }
    }
  }

  // Try watch_action.movie_src
  if let Some(watch) = movie_data.get("watch_action") {
    if let Some(movie_src) = watch.get("movie_src").and_then(|v| v.as_str()) {
      if !movie_src.is_empty() {
        return Ok(movie_src.to_string());
      }
    }
  }

  Err("No video stream found in movie data".to_string())
}

/// Resolve aparatkids.com URLs by parsing the page HTML
async fn resolve_aparatkids_com(
  client: &reqwest::Client,
  url: &str,
) -> Result<AparatkidsResolved, String> {
  let html = client
    .get(url)
    .send()
    .await
    .map_err(|e| format!("Failed to fetch page: {e}"))?
    .text()
    .await
    .map_err(|e| format!("Failed to read page content: {e}"))?;

  let m3u8_url =
    extract_m3u8_url(&html).ok_or_else(|| "Failed to extract video URL from page".to_string())?;

  let title = extract_title(&html);
  let thumbnail = extract_thumbnail(&html);
  let duration = extract_duration(&html);

  Ok(AparatkidsResolved {
    m3u8_url,
    title,
    thumbnail,
    duration,
    filesize: None,
  })
}

fn extract_m3u8_url(html: &str) -> Option<String> {
  if let Some(url) = extract_from_player_data(html) {
    return Some(url);
  }

  if let Some(url) = extract_from_og_video(html) {
    return Some(url);
  }

  None
}

fn extract_from_player_data(html: &str) -> Option<String> {
  let re = Regex::new(r"var\s+player_data\s*=\s*(\{.+?\})\s*;").ok()?;
  let caps = re.captures(html)?;
  let json_str = caps.get(1)?.as_str();

  let data: Value = serde_json::from_str(json_str).ok()?;
  let multi_src = data.get("multiSRC")?.as_array()?;

  for src_group in multi_src {
    if let Some(arr) = src_group.as_array() {
      for src_entry in arr {
        if let Some(src_url) = src_entry.get("src").and_then(|v| v.as_str()) {
          if src_url.contains(".m3u8") || src_url.contains("m3u8") {
            return Some(src_url.to_string());
          }
        }
      }
    }
  }

  None
}

fn extract_from_og_video(html: &str) -> Option<String> {
  let re =
    Regex::new(r#"<meta\s+(?:property|name)=["']og:video["']\s+content=["']([^"']+)["']"#).ok()?;
  let caps = re.captures(html)?;
  let url = caps.get(1)?.as_str().to_string();
  if url.contains("m3u8") || url.contains(".mp4") {
    Some(url)
  } else {
    None
  }
}

fn extract_title(html: &str) -> Option<String> {
  if let Some(name) = extract_ux_event_field(html, "nameFa") {
    return Some(name);
  }
  if let Some(name) = extract_ux_event_field(html, "nameEn") {
    return Some(name);
  }
  let re = Regex::new(r"<title>\s*(.+?)\s*</title>").ok()?;
  let caps = re.captures(html)?;
  let title = caps.get(1)?.as_str().trim().to_string();
  if title.is_empty() || title == "آپارات کودک" {
    None
  } else {
    Some(title)
  }
}

fn extract_ux_event_field(html: &str, field: &str) -> Option<String> {
  let pattern = format!(r#"uxEvents\.movie\.{}\s*=\s*['"]([^'"]+)['"]"#, field);
  let re = Regex::new(&pattern).ok()?;
  let caps = re.captures(html)?;
  let value = caps.get(1)?.as_str().trim().to_string();
  if value.is_empty() {
    None
  } else {
    Some(value)
  }
}

fn extract_thumbnail(html: &str) -> Option<String> {
  if let Some(poster) = extract_ux_event_field(html, "poster") {
    return Some(poster);
  }
  let re =
    Regex::new(r#"<meta\s+(?:property|name)=["']og:image["']\s+content=["']([^"']+)["']"#).ok()?;
  let caps = re.captures(html)?;
  let url = caps.get(1)?.as_str().trim().to_string();
  if url.is_empty() {
    None
  } else {
    Some(url)
  }
}

fn extract_duration(html: &str) -> Option<f64> {
  if let Some(dur_str) = extract_ux_event_field(html, "totalDuration") {
    if let Ok(dur) = dur_str.parse::<f64>() {
      if dur > 0.0 {
        return Some(dur);
      }
    }
  }
  let re =
    Regex::new(r#"<meta\s+(?:property|name)=["']video:duration["']\s+content=["'](\d+)["']"#)
      .ok()?;
  let caps = re.captures(html)?;
  let dur_str = caps.get(1)?.as_str();
  dur_str.parse::<f64>().ok()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_is_aparatkids_url() {
    assert!(is_aparatkids_url("https://www.aparatkids.com/w/0oy3n"));
    assert!(is_aparatkids_url("https://www.aparatkids.com/m/118063"));
    assert!(is_aparatkids_url("https://www.aparat.com/v/lyo8514"));
    assert!(is_aparatkids_url("https://www.aparat.com/m/bdmq6"));
    assert!(is_aparatkids_url("https://www.aparat.com/m/5afnr"));
    assert!(is_aparatkids_url(
      "https://www.aparat.com/shorts/2069019368604831744"
    ));
    assert!(is_aparatkids_url("https://aparat.com/v/abc123"));
    assert!(is_aparatkids_url(
      "https://www.aparat.com/v/yswc201?playlist=22795929"
    ));
    assert!(!is_aparatkids_url("https://youtube.com/watch?v=abc"));
  }

  #[test]
  fn test_aparat_playlist_id() {
    assert_eq!(
      aparat_playlist_id(
        "https://www.aparat.com/v/yswc201?playlist=22795929&refererRef=channel_page"
      ),
      Some("22795929".to_string())
    );
    assert_eq!(
      aparat_playlist_id("https://www.aparat.com/v/abc?foo=bar&playlist=12345"),
      Some("12345".to_string())
    );
    assert_eq!(
      aparat_playlist_id("https://www.aparat.com/v/abc?playlist=999"),
      Some("999".to_string())
    );
    assert_eq!(aparat_playlist_id("https://www.aparat.com/v/abc"), None);
  }

  #[test]
  fn test_extract_from_player_data() {
    let html = r#"var player_data={"multiSRC":[[{"src":"https://www.aparatkids.com/movie/watch/m3u8/test.m3u8","type":"application/vnd.apple.mpegurl"}]]};"#;
    let url = extract_from_player_data(html);
    assert!(url.is_some());
    assert!(url.unwrap().contains("m3u8"));
  }

  #[test]
  fn test_extract_title() {
    let html = r#"uxEvents.movie.nameFa="محله کوکوملون - فصل ۶ قسمت ۲";uxEvents.movie.nameEn="CoComelon Lane S06E02";"#;
    let title = extract_title(html);
    assert!(title.is_some());
    assert_eq!(title.unwrap(), "محله کوکوملون - فصل ۶ قسمت ۲");
  }

  #[test]
  fn test_extract_thumbnail() {
    let html =
      r#"uxEvents.movie.poster="https://static.cdn.asset.filimo.com/flmt/mov_177556_347970.jpg";"#;
    let thumb = extract_thumbnail(html);
    assert!(thumb.is_some());
    assert!(thumb.unwrap().contains("filimo.com"));
  }

  #[test]
  fn test_extract_duration() {
    let html = r#"uxEvents.movie.totalDuration="1071";"#;
    let dur = extract_duration(html);
    assert!(dur.is_some());
    assert_eq!(dur.unwrap(), 1071.0);
  }

  #[test]
  fn test_extract_playlist_item_url_video_hash() {
    let item = serde_json::json!({
      "video_hash": "yswc201",
      "title": "Test Video"
    });
    let url = extract_playlist_item_url(&item);
    assert_eq!(url, Some("https://www.aparat.com/v/yswc201".to_string()));
  }

  #[test]
  fn test_extract_playlist_item_url_video_hash_camel() {
    let item = serde_json::json!({
      "videoHash": "abc123"
    });
    let url = extract_playlist_item_url(&item);
    assert_eq!(url, Some("https://www.aparat.com/v/abc123".to_string()));
  }

  #[test]
  fn test_extract_playlist_item_url_shorts_id() {
    let item = serde_json::json!({
      "shorts_id": 2069019368604831744i64
    });
    let url = extract_playlist_item_url(&item);
    assert_eq!(
      url,
      Some("https://www.aparat.com/shorts/2069019368604831744".to_string())
    );
  }

  #[test]
  fn test_extract_playlist_item_url_numeric_hash_is_shorts() {
    let item = serde_json::json!({
      "hash": "2069019368604831744"
    });
    let url = extract_playlist_item_url(&item);
    assert_eq!(
      url,
      Some("https://www.aparat.com/shorts/2069019368604831744".to_string())
    );
  }

  #[test]
  fn test_extract_playlist_item_url_alphanumeric_hash_is_video() {
    let item = serde_json::json!({
      "hash": "yswc201"
    });
    let url = extract_playlist_item_url(&item);
    assert_eq!(url, Some("https://www.aparat.com/v/yswc201".to_string()));
  }

  #[test]
  fn test_extract_playlist_item_url_full_url_field() {
    let item = serde_json::json!({
      "url": "https://www.aparat.com/shorts/12345"
    });
    let url = extract_playlist_item_url(&item);
    assert_eq!(url, Some("https://www.aparat.com/shorts/12345".to_string()));
  }

  #[test]
  fn test_extract_playlist_item_url_empty() {
    let item = serde_json::json!({
      "title": "No URL"
    });
    let url = extract_playlist_item_url(&item);
    assert_eq!(url, None);
  }
}
