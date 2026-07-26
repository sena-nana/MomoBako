use std::{
    collections::{BTreeMap, HashSet},
    time::Duration,
};

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
    "momobako.service.provider.asmr-one",
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
                .ok_or_else(|| "ASMR One provider requires an RJ work id".to_string())?;
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
                "provider": "asmr-one",
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
    format!(
        "https://api.asmr-200.com/api/workInfo/{}",
        work_id.trim_start_matches("RJ")
    )
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
        .header(reqwest::header::ACCEPT, "application/json, text/plain, */*")
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
    let value =
        serde_json::from_str::<serde_json::Value>(body).map_err(|error| error.to_string())?;
    let payload = value
        .get("data")
        .filter(|item| item.is_object())
        .unwrap_or(&value);
    let mut fields = BTreeMap::from([
        ("workId".to_string(), serde_json::json!(work_id)),
        ("rjCode".to_string(), serde_json::json!(work_id)),
        ("sourceUrl".to_string(), serde_json::json!(source_url)),
    ]);
    insert_string(
        &mut fields,
        "workTitle",
        first_string(payload, &["title", "name", "workTitle"]),
    );
    insert_string(
        &mut fields,
        "circle",
        nested_string(
            payload,
            &[&["circle", "name"], &["maker", "name"], &["circleName"]],
        ),
    );
    insert_string(
        &mut fields,
        "series",
        nested_string(payload, &[&["series", "name"], &["seriesName"]]),
    );
    insert_string(
        &mut fields,
        "releaseDate",
        first_string(payload, &["release", "releaseDate", "release_dtl"]),
    );
    insert_string(
        &mut fields,
        "ageRating",
        first_string(payload, &["ageCategory", "ageRating", "rate"]),
    );
    insert_number(&mut fields, "price", first_number(payload, &["price"]));
    insert_number(
        &mut fields,
        "dlCount",
        first_number(payload, &["dl_count", "dlCount", "sales"]),
    );
    insert_number(
        &mut fields,
        "reviewCount",
        first_number(payload, &["review_count", "reviewCount"]),
    );
    insert_number(
        &mut fields,
        "rateAverage",
        first_number(payload, &["rate_average_2dp", "rateAverage", "rating"]),
    );
    insert_array(
        &mut fields,
        "voiceActors",
        collect_named_array(payload, &["vas", "voiceActors", "creators"]),
    );
    insert_array(
        &mut fields,
        "scenarioTags",
        collect_named_array(payload, &["tags", "genres"]),
    );
    if let Some(cover) = first_string(
        payload,
        &["mainCoverUrl", "cover", "image_main", "imageMain"],
    ) {
        fields.insert("cover".to_string(), serde_json::json!(cover));
    }
    if fields.len() <= 3 {
        return Err("provider did not return usable metadata".to_string());
    }
    Ok(serde_json::json!({
        "source": "asmr-one",
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

fn insert_array(fields: &mut BTreeMap<String, serde_json::Value>, key: &str, values: Vec<String>) {
    if !values.is_empty() {
        fields.insert(key.to_string(), serde_json::json!(values));
    }
}

fn first_string(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(json_string))
}

fn first_number(value: &serde_json::Value, keys: &[&str]) -> Option<f64> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(json_number))
}

fn nested_string(value: &serde_json::Value, paths: &[&[&str]]) -> Option<String> {
    for path in paths {
        let mut current = value;
        let mut found = true;
        for key in *path {
            if let Some(next) = current.get(*key) {
                current = next;
            } else {
                found = false;
                break;
            }
        }
        if found {
            if let Some(text) = json_string(current) {
                return Some(text);
            }
        }
    }
    None
}

fn json_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn json_number(value: &serde_json::Value) -> Option<f64> {
    match value {
        serde_json::Value::Number(number) => number.as_f64(),
        serde_json::Value::String(text) => text.replace(',', "").parse::<f64>().ok(),
        _ => None,
    }
}

fn collect_named_array(value: &serde_json::Value, keys: &[&str]) -> Vec<String> {
    let mut values = Vec::new();
    let mut seen = HashSet::new();
    for key in keys {
        if let Some(raw) = value.get(*key) {
            collect_names(raw, &mut values, &mut seen);
        }
    }
    values
}

fn collect_names(value: &serde_json::Value, values: &mut Vec<String>, seen: &mut HashSet<String>) {
    match value {
        serde_json::Value::Array(items) => {
            for item in items {
                collect_names(item, values, seen);
            }
        }
        serde_json::Value::Object(map) => {
            if let Some(name) = map
                .get("name")
                .or_else(|| map.get("label"))
                .or_else(|| map.get("value"))
                .and_then(json_string)
            {
                push_unique(values, seen, name);
            }
        }
        serde_json::Value::String(text) => push_unique(values, seen, text.clone()),
        _ => {}
    }
}

fn push_unique(values: &mut Vec<String>, seen: &mut HashSet<String>, value: String) {
    let normalized = value.trim();
    if !normalized.is_empty() && seen.insert(normalized.to_string()) {
        values.push(normalized.to_string());
    }
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
    fn parses_json_candidate() {
        let body = r#"{
          "id": "RJ123456",
          "title": "Rain Voice",
          "circle": { "name": "Blue Circle" },
          "vas": [{ "name": "Aoi" }],
          "tags": [{ "name": "sleep" }],
          "dl_count": 1234,
          "rateAverage": 4.8
        }"#;
        let candidate = parse_candidate("RJ123456", "https://example.test/RJ123456", body).unwrap();
        assert_eq!(candidate["source"], "asmr-one");
        assert_eq!(candidate["fields"]["workTitle"], "Rain Voice");
        assert_eq!(candidate["fields"]["voiceActors"][0], "Aoi");
    }
}
