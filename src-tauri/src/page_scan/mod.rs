use crate::{PageScanResult, ScannedVideo};
use std::collections::HashSet;

/// Fetch a webpage and extract all video sources found in the HTML.
pub async fn scan(url: &str) -> Result<PageScanResult, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .build()
        .map_err(|e| e.to_string())?;

    let html = client.get(url).send().await.map_err(|e| format!("Could not reach page: {}", e))?.text().await.map_err(|e| e.to_string())?;

    let page_title = extract_tag_content(&html, "title").unwrap_or_default();
    let mut videos: Vec<ScannedVideo> = vec![];
    let mut seen: HashSet<String> = HashSet::new();

    // 1. <video src="..."> and <source src="....mp4/webm/mov">
    if let Ok(re) = regex::Regex::new(r#"(?i)(?:<video[^>]*src|<source[^>]*src)=["']([^"']+\.(?:mp4|webm|mov))["']"#) {
        for cap in re.captures_iter(&html) {
            let vurl = resolve(url, &cap[1]);
            if seen.insert(vurl.clone()) {
                videos.push(ScannedVideo { url: vurl, title: None, source: "video-tag".into(), platform: None });
            }
        }
    }

    // 2. <iframe src="..."> from known platforms
    if let Ok(re) = regex::Regex::new(r#"(?i)<iframe[^>]*src=["']([^"']+)["']"#) {
        for cap in re.captures_iter(&html) {
            let src = &cap[1];
            if let Some(platform) = detect_embed(src) {
                let vurl = resolve(url, src);
                if seen.insert(vurl.clone()) {
                    videos.push(ScannedVideo { url: vurl, title: None, source: "iframe-embed".into(), platform: Some(platform) });
                }
            }
        }
    }

    // 3. JSON-LD VideoObject
    if let Ok(re) = regex::Regex::new(r#"(?is)<script[^>]*type=["']application/ld\+json["'][^>]*>(.*?)</script>"#) {
        for cap in re.captures_iter(&html) {
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(&cap[1]) {
                extract_video_objects(&data, url, &mut videos, &mut seen);
            }
        }
    }

    // 4. og:video meta tags
    if let Ok(re) = regex::Regex::new(r#"(?i)<meta[^>]*property=["']og:video(?:\.secure_url)?["'][^>]*content=["']([^"']+)["']"#) {
        for cap in re.captures_iter(&html) {
            let vurl = resolve(url, &cap[1]);
            if seen.insert(vurl.clone()) {
                videos.push(ScannedVideo { url: vurl, title: None, source: "og-meta".into(), platform: None });
            }
        }
    }

    // 5. Direct video file URLs in href/src attributes
    if let Ok(re) = regex::Regex::new(r#"(?i)(?:src|href)=["']([^"']+\.(?:mp4|webm|mov|mkv))["']"#) {
        for cap in re.captures_iter(&html) {
            let vurl = resolve(url, &cap[1]);
            if seen.insert(vurl.clone()) {
                videos.push(ScannedVideo { url: vurl, title: None, source: "direct-file".into(), platform: None });
            }
        }
    }

    Ok(PageScanResult { page_url: url.to_string(), page_title, videos })
}

fn extract_tag_content(html: &str, tag: &str) -> Option<String> {
    let pattern = format!(r#"<{}[^>]*>(.*?)</{}>"#, tag, tag);
    let re = regex::Regex::new(&pattern).ok()?;
    re.captures(html).and_then(|c| c.get(1)).map(|m| m.as_str().trim().to_string())
}

fn detect_embed(src: &str) -> Option<String> {
    if src.contains("youtube.com/embed/") || src.contains("youtu.be/") { Some("youtube".into()) }
    else if src.contains("player.vimeo.com/video/") { Some("vimeo".into()) }
    else if src.contains("wistia.net") || src.contains("fast.wistia.com") { Some("wistia".into()) }
    else if src.contains("dailymotion.com/embed/") { Some("dailymotion".into()) }
    else { None }
}

fn extract_video_objects(data: &serde_json::Value, base_url: &str, videos: &mut Vec<ScannedVideo>, seen: &mut HashSet<String>) {
    let items = if data.is_array() { data.as_array().unwrap() } else { std::slice::from_ref(data) };

    for item in items.iter() {
        let ty = item["@type"].as_str().unwrap_or("");
        if ty == "VideoObject" || ty.contains("Video") {
            if let Some(vurl) = item["contentUrl"].as_str().or(item.get("embedUrl").and_then(|v| v.as_str())) {
                let full_url = resolve(base_url, vurl);
                if seen.insert(full_url.clone()) {
                    videos.push(ScannedVideo {
                        url: full_url,
                        title: item["name"].as_str().or(item["headline"].as_str()).map(String::from),
                        source: "json-ld".into(),
                        platform: None,
                    });
                }
            }
        }

        if let Some(graph) = item.get("@graph") { extract_video_objects(graph, base_url, videos, seen); }
        if let Some(part) = item.get("hasPart") { extract_video_objects(part, base_url, videos, seen); }
    }
}

fn resolve(base: &str, relative: &str) -> String {
    if relative.starts_with("http://") || relative.starts_with("https://") {
        return relative.to_string();
    }
    let base_trimmed = base.trim_end_matches('/');
    if relative.starts_with('/') {
        let scheme_end = base.find("://").map(|i| i + 3).unwrap_or(0);
        let host_end = base[scheme_end..].find('/').map(|i| scheme_end + i).unwrap_or(base.len());
        format!("{}{}", &base[..host_end], relative)
    } else {
        format!("{}/{}", base_trimmed, relative)
    }
}
