use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

static APARATKIDS_URL_RE: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"https?://(?:www\.)?aparatkids\.com/(?:w|m)/([a-zA-Z0-9]+)").unwrap()
});

static APARAT_URL_RE: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"https?://(?:www\.)?aparat\.com/(?:v|m)/([a-zA-Z0-9]+)").unwrap()
});

pub struct AparatkidsResolved {
  pub m3u8_url: String,
  pub title: Option<String>,
  pub thumbnail: Option<String>,
  pub duration: Option<f64>,
}

pub fn is_aparatkids_url(url: &str) -> bool {
  APARATKIDS_URL_RE.is_match(url) || APARAT_URL_RE.is_match(url)
}

pub async fn resolve_aparatkids_url(url: &str) -> Result<AparatkidsResolved, String> {
  let client = reqwest::Client::builder()
    .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
    .build()
    .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

  if APARAT_URL_RE.is_match(url) {
    resolve_aparat_com(&client, url).await
  } else {
    resolve_aparatkids_com(&client, url).await
  }
}

/// Resolve aparat.com URLs via their JSON API
async fn resolve_aparat_com(
  client: &reqwest::Client,
  url: &str,
) -> Result<AparatkidsResolved, String> {
  let caps = APARAT_URL_RE
    .captures(url)
    .ok_or_else(|| "Invalid aparat.com URL".to_string())?;
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

  // Extract m3u8 URL
  let m3u8_url = extract_aparat_m3u8(attrs)?;

  // Extract metadata
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

  Ok(AparatkidsResolved {
    m3u8_url,
    title,
    thumbnail,
    duration,
  })
}

fn extract_aparat_m3u8(attrs: &Value) -> Result<String, String> {
  // Try hls_link first
  if let Some(hls) = attrs.get("hls_link").and_then(|v| v.as_str()) {
    if !hls.is_empty() {
      return Ok(hls.to_string());
    }
  }

  // Try hls.link
  if let Some(hls_obj) = attrs.get("hls") {
    if let Some(link) = hls_obj.get("link").and_then(|v| v.as_str()) {
      if !link.is_empty() {
        return Ok(link.to_string());
      }
    }
  }

  // Try manifest field
  if let Some(manifest) = attrs.get("manifest").and_then(|v| v.as_str()) {
    if manifest.starts_with("#EXTM3U") {
      // The manifest itself is m3u8 content - we need the URL, not content
      // Fall through to file_link
    }
  }

  // Try file_link as fallback (direct MP4)
  if let Some(file_link) = attrs.get("file_link").and_then(|v| v.as_str()) {
    if !file_link.is_empty() {
      return Ok(file_link.to_string());
    }
  }

  Err("No video stream found in API response".to_string())
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
  let re = Regex::new(r#"<meta\s+(?:property|name)=["']og:video["']\s+content=["']([^"']+)["']"#).ok()?;
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
  let re = Regex::new(r#"<meta\s+(?:property|name)=["']og:image["']\s+content=["']([^"']+)["']"#).ok()?;
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
  let re = Regex::new(r#"<meta\s+(?:property|name)=["']video:duration["']\s+content=["'](\d+)["']"#).ok()?;
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
    assert!(!is_aparatkids_url("https://youtube.com/watch?v=abc"));
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
    let html = r#"uxEvents.movie.poster="https://static.cdn.asset.filimo.com/flmt/mov_177556_347970.jpg";"#;
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
  fn test_aparat_url_patterns() {
    assert!(APARAT_URL_RE.is_match("https://www.aparat.com/v/lyo8514"));
    assert!(APARAT_URL_RE.is_match("https://www.aparat.com/m/bdmq6"));
    assert!(APARAT_URL_RE.is_match("https://aparat.com/v/lyo8514"));
    assert!(!APARAT_URL_RE.is_match("https://www.aparat.com/w/test"));
  }
}
