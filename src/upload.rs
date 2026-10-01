//! One call to Rdio Scanner: `POST <server>/api/call-upload`, multipart, with
//! the fields Trunk Recorder's Rdio Scanner uploader sends.

use std::path::Path;
use std::time::Duration;

use serde_json::json;
use trunk_recorder_plugin::{Attempt, ConcludedCall, Multipart};

pub struct Uploader {
    agent: ureq::Agent,
    url: String,
}

pub struct Upload<'a> {
    pub system_id: u32,
    pub api_key: &'a str,
    pub call: &'a ConcludedCall,
    /// The M4A, or the WAV.
    pub audio: &'a Path,
}

impl Uploader {
    pub fn new(server: &str) -> Uploader {
        let agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_global(Some(Duration::from_secs(120)))
            .http_status_as_error(false)
            .user_agent(concat!("trunk-pro-rdioscanner/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        Uploader { agent, url: format!("{}/api/call-upload", server.trim_end_matches('/')) }
    }

    pub fn upload(&self, u: &Upload) -> Attempt {
        let audio = match std::fs::read(u.audio) {
            Ok(b) => b,
            Err(e) => return Attempt::Fail(format!("can't read {}: {e}", u.audio.display())),
        };
        let (body, content_type) = form(u, &audio);
        let mut resp = match self.agent.post(&self.url).header("Content-Type", &content_type).send(&body) {
            Ok(r) => r,
            Err(e) => return Attempt::Retry(e.to_string()),
        };
        let status = resp.status().as_u16();
        let text = resp.body_mut().read_to_string().unwrap_or_default();
        outcome(status, &text)
    }
}

fn outcome(status: u16, text: &str) -> Attempt {
    // (A 202: Rdio Scanner took it for later.)
    if (200..300).contains(&status) {
        return Attempt::Done { url: String::new() };
    }
    let what: String = text.trim().lines().next().unwrap_or("").chars().take(200).collect();
    if status == 401 || status == 403 || what.contains("API key") {
        return Attempt::Fail(format!("Rdio Scanner refused the API key ({what})"));
    }
    let why = if what.is_empty() { format!("HTTP {status}") } else { format!("HTTP {status}: {what}") };
    match status {
        // Something else about the request was wrong; it won't get better.
        400..=499 if status != 408 && status != 429 => Attempt::Fail(why),
        _ => Attempt::Retry(why),
    }
}

/// The multipart body, and its content type.
fn form(u: &Upload, audio: &[u8]) -> (Vec<u8>, String) {
    let c = &u.call.call;
    let round2 = |x: f64| (x * 100.0).round() / 100.0;
    let sources: Vec<_> = c
        .src_list
        .iter()
        .map(|s| {
            let tag = if s.tag.is_empty() { &s.tag_ota } else { &s.tag };
            if tag.is_empty() {
                json!({ "pos": round2(s.pos), "src": s.src })
            } else {
                json!({ "pos": round2(s.pos), "src": s.src, "tag": tag })
            }
        })
        .collect();
    let freqs: Vec<_> = c
        .freq_list
        .iter()
        .map(
            |f| json!({ "freq": f.freq, "time": f.time, "pos": round2(f.pos), "len": round2(f.len), "errorCount": f.error_count, "spikeCount": f.spike_count }),
        )
        .collect();
    // (Trunk Recorder sends patches only when the call was patched.)
    let patches: &[u32] = if c.patched_talkgroups.len() > 1 { &c.patched_talkgroups } else { &[] };
    let m4a = u.audio.extension().is_some_and(|e| e == "m4a");
    let name = u.audio.file_name().map_or("call".into(), |n| n.to_string_lossy().into_owned());
    let fields = [
        ("audioName", name.clone()),
        ("audioType", if m4a { "audio/mp4" } else { "audio/wav" }.to_string()),
        ("dateTime", c.start_time.to_string()),
        ("frequencies", serde_json::to_string(&freqs).unwrap_or_default()),
        ("frequency", c.freq.to_string()),
        ("key", u.api_key.to_string()),
        ("patches", serde_json::to_string(&patches).unwrap_or_default()),
        ("talkgroup", c.talkgroup.to_string()),
        ("talkgroupGroup", c.talkgroup_group.clone()),
        // Trunk Recorder's names cross over here: its alpha tag is Rdio Scanner's label, its group tag Rdio Scanner's tag.
        ("talkgroupLabel", c.talkgroup_tag.clone()),
        ("talkgroupTag", c.talkgroup_group_tag.clone()),
        ("talkgroupName", c.talkgroup_description.clone()),
        ("sources", serde_json::to_string(&sources).unwrap_or_default()),
        ("system", u.system_id.to_string()),
        ("systemLabel", c.short_name.clone()),
    ];
    let mut form = Multipart::new().file("audio", &name, "application/octet-stream", audio);
    for (n, v) in fields {
        form = form.text(n, v);
    }
    form.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn responses() {
        assert!(matches!(outcome(200, "Call imported successfully."), Attempt::Done { .. }));
        assert!(matches!(outcome(202, ""), Attempt::Done { .. }));
        assert!(matches!(outcome(401, "Invalid API key"), Attempt::Fail(_)));
        assert!(matches!(outcome(417, "Incomplete call data: no talkgroup"), Attempt::Fail(_)));
        assert!(matches!(outcome(500, ""), Attempt::Retry(_)));
        assert!(matches!(outcome(429, ""), Attempt::Retry(_)));
    }
}
