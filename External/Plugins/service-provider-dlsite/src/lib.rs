use std::{collections::BTreeMap, time::Duration};

use momobako_mutsuki_plugin_sdk::{export_mutsuki_momobako_plugin, PluginCallEnvelope};
use serde::Deserialize;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LookupPayload {
    id: String,
    source_url: Option<String>,
}

export_mutsuki_momobako_plugin!(
    "momobako.service.provider.dlsite",
    "0.1.0",
    protocols = ["provider.lookupMetadataCandidate"],
    requires = [],
    permissions = ["network", "useProvider", "writeCandidates"],
    handle_call
);

fn handle_call(request: PluginCallEnvelope) -> Result<serde_json::Value, String> {
    match request.method.as_str() {
        "provider.lookupMetadataCandidate" => {
            let payload: LookupPayload =
                serde_json::from_value(request.payload).map_err(|error| error.to_string())?;
            let work_id = normalize_work_id(&payload.id)
                .ok_or_else(|| "DLsite provider requires an RJ work id".to_string())?;
            let source_url = payload
                .source_url
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| default_source_url(&work_id));
            let body = fetch_body(&source_url)?;
            let candidate = parse_candidate(&work_id, &source_url, &body)?;
            Ok(serde_json::json!({
                "provider": "dlsite",
                "id": work_id,
                "sourceUrl": source_url,
                "fetchedAt": now_rfc3339(),
                "candidate": candidate
            }))
        }
        method => Err(format!("unsupported method: {method}")),
    }
}

fn normalize_work_id(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    for index in 0..bytes.len().saturating_sub(1) {
        if !bytes[index].eq_ignore_ascii_case(&b'R')
            || !bytes[index + 1].eq_ignore_ascii_case(&b'J')
        {
            continue;
        }
        let start = index + 2;
        let mut end = start;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
        if (6..=8).contains(&(end - start)) {
            return Some(format!("RJ{}", &value[start..end]));
        }
    }
    None
}

fn default_source_url(work_id: &str) -> String {
    format!("https://www.dlsite.com/maniax/work/=/product_id/{work_id}.html")
}

fn fetch_body(url: &str) -> Result<String, String> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("provider lookup only supports http and https URLs".to_string());
    }
    let response = reqwest::blocking::Client::builder()
        .user_agent("MomoBakoProvider/1")
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| format!("provider client error: {error}"))?
        .get(url)
        .header(
            reqwest::header::ACCEPT,
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        )
        .send()
        .map_err(|error| format!("provider request failed: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("provider returned HTTP {status}"));
    }
    response
        .text()
        .map_err(|error| format!("provider body error: {error}"))
}

fn parse_candidate(
    work_id: &str,
    source_url: &str,
    body: &str,
) -> Result<serde_json::Value, String> {
    let mut fields = BTreeMap::from([
        ("workId".to_string(), serde_json::json!(work_id)),
        ("rjCode".to_string(), serde_json::json!(work_id)),
        ("sourceUrl".to_string(), serde_json::json!(source_url)),
    ]);
    insert_string(
        &mut fields,
        "workTitle",
        html_meta_content(body, "og:title").or_else(|| html_title(body)),
    );
    insert_string(
        &mut fields,
        "circle",
        html_meta_content(body, "product:brand").or_else(|| json_like_string(body, "maker_name")),
    );
    insert_string(
        &mut fields,
        "releaseDate",
        json_like_string(body, "regist_date"),
    );
    insert_number(&mut fields, "price", json_like_number(body, "price"));
    insert_number(&mut fields, "dlCount", json_like_number(body, "dl_count"));
    insert_number(
        &mut fields,
        "reviewCount",
        json_like_number(body, "review_count"),
    );
    insert_number(
        &mut fields,
        "rateAverage",
        json_like_number(body, "rate_average_2dp"),
    );
    if let Some(cover) = html_meta_content(body, "og:image") {
        fields.insert("cover".to_string(), serde_json::json!(cover));
    }
    if fields.len() <= 3 {
        return Err("provider did not return usable metadata".to_string());
    }
    Ok(serde_json::json!({
        "source": "dlsite",
        "confidence": "external-id",
        "fields": fields
    }))
}

fn insert_string(
    fields: &mut BTreeMap<String, serde_json::Value>,
    key: &str,
    value: Option<String>,
) {
    let Some(value) = value
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
    else {
        return;
    };
    fields.insert(key.to_string(), serde_json::json!(value));
}

fn insert_number(fields: &mut BTreeMap<String, serde_json::Value>, key: &str, value: Option<f64>) {
    let Some(value) = value.filter(|item| item.is_finite()) else {
        return;
    };
    fields.insert(key.to_string(), serde_json::json!(value));
}

fn html_meta_content(body: &str, property: &str) -> Option<String> {
    let property_marker = format!("property=\"{property}\"");
    let name_marker = format!("name=\"{property}\"");
    for tag in body
        .split('<')
        .filter(|chunk| chunk.trim_start().starts_with("meta"))
    {
        if !tag.contains(&property_marker) && !tag.contains(&name_marker) {
            continue;
        }
        if let Some(content) = html_attribute(tag, "content") {
            return Some(html_decode_basic(&content));
        }
    }
    None
}

fn html_title(body: &str) -> Option<String> {
    let start = body.find("<title>")? + "<title>".len();
    let end = body[start..].find("</title>")? + start;
    Some(html_decode_basic(&body[start..end]))
}

fn html_attribute(tag: &str, name: &str) -> Option<String> {
    let marker = format!("{name}=\"");
    let start = tag.find(&marker)? + marker.len();
    let end = tag[start..].find('"')? + start;
    Some(tag[start..end].to_string())
}

fn html_decode_basic(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .trim()
        .to_string()
}

fn json_like_string(body: &str, key: &str) -> Option<String> {
    let marker = format!("\"{key}\"");
    let start = body.find(&marker)?;
    let after_key = &body[start + marker.len()..];
    let colon = after_key.find(':')?;
    let after_colon = after_key[colon + 1..].trim_start();
    if !after_colon.starts_with('"') {
        return None;
    }
    let mut escaped = false;
    let mut end = None;
    for (index, ch) in after_colon[1..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            end = Some(index + 1);
            break;
        }
    }
    let raw = &after_colon[..=end?];
    serde_json::from_str::<String>(raw).ok()
}

fn json_like_number(body: &str, key: &str) -> Option<f64> {
    let marker = format!("\"{key}\"");
    let start = body.find(&marker)?;
    let after_key = &body[start + marker.len()..];
    let colon = after_key.find(':')?;
    let after_colon = after_key[colon + 1..].trim_start();
    let raw = if let Some(stripped) = after_colon.strip_prefix('"') {
        let end = stripped.find('"')?;
        stripped[..end].replace(',', "")
    } else {
        after_colon
            .chars()
            .take_while(|ch| ch.is_ascii_digit() || matches!(ch, '.' | '-'))
            .collect()
    };
    raw.parse::<f64>().ok()
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_html_candidate() {
        let body = r#"
          <meta property="og:title" content="Rain Voice" />
          <meta property="product:brand" content="Blue Circle" />
          <meta property="og:image" content="https://example.test/cover.jpg" />
          <script>{"dl_count":1234,"price":770,"review_count":9,"rate_average_2dp":4.8}</script>
        "#;
        let candidate = parse_candidate("RJ123456", "https://example.test/RJ123456", body).unwrap();
        assert_eq!(candidate["source"], "dlsite");
        assert_eq!(candidate["fields"]["workTitle"], "Rain Voice");
        assert_eq!(candidate["fields"]["dlCount"], 1234.0);
    }
}
