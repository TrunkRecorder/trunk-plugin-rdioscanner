//! Rdio Scanner — a Trunk Recorder Pro plugin that uploads recorded calls to
//! an [Rdio Scanner](https://github.com/chuot/rdio-scanner) server, as Trunk
//! Recorder's Rdio Scanner uploader does.

mod upload;

use std::collections::HashMap;
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use trunk_recorder_plugin::filter::patterns;
use trunk_recorder_plugin::{Attempt, CallQueue, ConcludedCall, Host, Manifest, Plugin, QueueOptions, Setup, TalkgroupFilter, format, topic};

use upload::{Upload, Uploader};

#[derive(Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase", default)]
struct Config {
    /// Server
    ///
    /// Your Rdio Scanner's web address, the one you open it with (like http://192.168.1.20:3000).
    #[schemars(url, extend("x-required" = true))]
    #[serde(alias = "rdioscannerServer")]
    server: String,
}

#[derive(Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase", default)]
struct SystemConfig {
    /// API key
    ///
    /// A key from the API keys section of Rdio Scanner's administration page. Leave it empty to not upload this system.
    #[schemars(extend("x-secret" = true, "x-required" = true))]
    #[serde(alias = "rdioscannerApiKey")]
    api_key: String,
    /// System ID
    ///
    /// The system's ID in Rdio Scanner.
    #[serde(alias = "rdioscannerSystemId")]
    #[schemars(extend("x-required" = true))]
    system_id: Option<u32>,
    /// Only these talkgroups
    ///
    /// Upload these talkgroups and no others: numbers, or patterns like 507* (* stands for any digits, ? for one). Leave it empty to upload every talkgroup.
    #[serde(deserialize_with = "patterns")]
    #[schemars(with = "Vec<String>")]
    talkgroup_allow: Vec<String>,
    /// Not these talkgroups
    ///
    /// Never upload these talkgroups: numbers or patterns, as above.
    #[serde(deserialize_with = "patterns")]
    #[schemars(with = "Vec<String>")]
    talkgroup_deny: Vec<String>,
}

/// Where a system's calls go.
struct Target {
    system_id: u32,
    api_key: String,
    filter: TalkgroupFilter,
}

struct RdioScanner {
    queue: CallQueue,
}

impl Plugin for RdioScanner {
    type Config = Config;
    type SystemConfig = SystemConfig;

    fn manifest() -> Manifest {
        Manifest {
            name: "Rdio Scanner".into(),
            subscribe: vec![topic::CALL_CONCLUDED.into()],
            // Smaller uploads; WAV when there's no encoder.
            audio_formats: vec![format::M4A.into()],
            ..trunk_recorder_plugin::manifest!()
        }
    }

    fn start(host: Host, setup: Setup<Config, SystemConfig>) -> Result<Self, String> {
        let server = setup.config.server.trim().to_string();
        if server.is_empty() {
            return Err("Add your Rdio Scanner's web address.".into());
        }
        if !(server.starts_with("https://") || server.starts_with("http://")) {
            return Err(format!("The server has to be a web address (http://…), not \"{server}\""));
        }
        let mut targets = HashMap::new();
        for s in &setup.systems {
            let Some(c) = &s.config else { continue };
            let api_key = c.api_key.trim().to_string();
            if api_key.is_empty() {
                continue;
            }
            let Some(system_id) = c.system_id else {
                return Err(format!("Add {}'s Rdio Scanner system ID (or clear its API key).", s.short_name));
            };
            let filter = TalkgroupFilter::new(&c.talkgroup_allow, &c.talkgroup_deny);
            let filtered = if filter.is_empty() { String::new() } else { format!(", talkgroups: {}", filter.describe()) };
            host.info(format!("uploading {} as system {system_id}{filtered}", s.short_name));
            targets.insert(s.short_name.clone(), Target { system_id, api_key, filter });
        }
        if targets.is_empty() {
            return Err("Add an Rdio Scanner API key and system ID to the systems you want to upload.".into());
        }
        if !setup.has_format(format::M4A) {
            host.info("no M4A encoder: uploading WAV");
        }
        let uploader = Uploader::new(&server);
        // The dashboard names the service by its host.
        let host_name = server.split_once("://").map_or(server.as_str(), |(_, r)| r).split('/').next().unwrap_or("").to_string();
        let opts = QueueOptions { noun: "upload", endpoint: Some(format!("Rdio Scanner at {host_name}")), ..QueueOptions::saved_in(&setup.data_dir) };
        let queue = CallQueue::start(host, opts, move |call: &ConcludedCall| {
            // By short name, a system's identity: a call saved for a later run still finds its system.
            let Some(t) = targets.get(&call.call.short_name) else {
                return Attempt::Skip("no Rdio Scanner API key for this system".into());
            };
            if call.call.encrypted {
                return Attempt::Skip("encrypted".into());
            }
            if !t.filter.passes(call.call.talkgroup) {
                return Attempt::Skip(format!("talkgroup {} isn't uploaded (talkgroup filter)", call.call.talkgroup));
            }
            let audio = call.files.m4a.as_ref().filter(|p| p.is_file()).unwrap_or(&call.files.wav);
            uploader.upload(&Upload { system_id: t.system_id, api_key: &t.api_key, call, audio })
        });
        Ok(RdioScanner { queue })
    }

    fn call_concluded(&mut self, call: ConcludedCall) {
        self.queue.push(call);
    }

    fn shutdown(&mut self, grace: Duration) {
        self.queue.shutdown(grace);
    }
}

fn main() {
    trunk_recorder_plugin::run::<RdioScanner>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use trunk_recorder_plugin::testing::{self, MockServer, Request};
    use trunk_recorder_plugin::{EXIT_CONFIG, HostMessage, Outcome, State};

    /// Rdio Scanner's call upload: key "good" works.
    fn rdio() -> MockServer {
        MockServer::start(|req: &Request| {
            let key = String::from_utf8(req.form_field("key").unwrap_or_default()).unwrap();
            match (req.path.as_str(), key.as_str()) {
                ("/api/call-upload", "good") => (200, "Call imported successfully.\n".into()),
                ("/api/call-upload", _) => (401, "Invalid API key\n".into()),
                _ => (404, String::new()),
            }
        })
    }

    fn hello(dir: &std::path::Path, server: &str, system: Value) -> HostMessage {
        let mut h = testing::hello(dir, json!({ "server": server }));
        h.systems[0].config = system;
        HostMessage::Hello(h)
    }

    #[test]
    fn uploads_a_call_as_trunk_recorder_does() {
        let (dir, server) = (testing::temp_dir("rdio"), rdio());
        let mut call = testing::call(&dir, "sys1", 101);
        call.call.talkgroup_group = "Fire".into();
        call.call.talkgroup_group_tag = "Fire Dispatch".into();
        call.call.talkgroup_description = "Fire Dispatch 1".into();
        call.call.src_list[0].tag = "E1".into();
        call.call.patched_talkgroups = vec![101, 202];
        let out =
            testing::run::<RdioScanner>([hello(&dir, server.url(), json!({ "apiKey": "good", "systemId": 11 })), HostMessage::CallConcluded(call.clone())]);
        assert!(out.ready(), "{:?}", out.messages);
        assert_eq!(out.results(), vec![(call.path.clone(), Outcome::Ok, String::new(), String::new())]);
        let r = &server.requests()[0];
        assert_eq!((r.method.as_str(), r.path.as_str()), ("POST", "/api/call-upload"));
        let field = |n: &str| String::from_utf8(r.form_field(n).unwrap()).unwrap();
        assert_eq!(field("key"), "good");
        assert_eq!(field("system"), "11");
        assert_eq!(field("systemLabel"), "sys1");
        assert_eq!(field("talkgroup"), "101");
        assert_eq!(field("talkgroupLabel"), "TG 101");
        assert_eq!(field("talkgroupTag"), "Fire Dispatch");
        assert_eq!(field("talkgroupName"), "Fire Dispatch 1");
        assert_eq!(field("talkgroupGroup"), "Fire");
        assert_eq!(field("frequency"), "851012500");
        assert_eq!(field("dateTime"), call.call.start_time.to_string());
        assert_eq!(field("patches"), "[101,202]");
        assert_eq!(field("audioType"), "audio/mp4");
        assert!(field("audioName").ends_with(".m4a"));
        let sources: Value = serde_json::from_str(&field("sources")).unwrap();
        assert_eq!(sources, json!([{ "pos": 0.0, "src": 1234, "tag": "E1" }]));
        let freqs: Value = serde_json::from_str(&field("frequencies")).unwrap();
        assert_eq!(freqs, json!([{ "freq": 851012500u64, "time": call.call.start_time, "pos": 0.0, "len": 3.0, "errorCount": 2, "spikeCount": 0 }]));
        assert_eq!(r.form_field("audio").unwrap(), std::fs::read(call.files.m4a.unwrap()).unwrap());
    }

    #[test]
    fn wav_without_an_encoder() {
        let (dir, server) = (testing::temp_dir("rdio"), rdio());
        let mut h = testing::hello(&dir, json!({ "server": server.url() }));
        h.systems[0].config = json!({ "apiKey": "good", "systemId": 1 });
        h.audio_formats = vec!["wav".into()];
        let mut call = testing::call(&dir, "sys1", 101);
        call.files.m4a = None;
        let out = testing::run::<RdioScanner>([HostMessage::Hello(h), HostMessage::CallConcluded(call.clone())]);
        assert_eq!(out.results()[0].1, Outcome::Ok);
        let r = &server.requests()[0];
        assert_eq!(r.form_field("audioType").unwrap(), b"audio/wav");
        assert_eq!(r.form_field("audio").unwrap(), std::fs::read(&call.files.wav).unwrap());
    }

    #[test]
    fn a_wrong_key_fails_without_retrying() {
        let (dir, server) = (testing::temp_dir("rdio"), rdio());
        let out = testing::run::<RdioScanner>([
            hello(&dir, server.url(), json!({ "apiKey": "bad", "systemId": 1 })),
            HostMessage::CallConcluded(testing::call(&dir, "sys1", 5)),
        ]);
        let r = out.results();
        assert_eq!(r[0].1, Outcome::Failed);
        assert!(r[0].2.contains("API key"), "{}", r[0].2);
        assert_eq!(server.requests().len(), 1);
    }

    #[test]
    fn talkgroup_filters() {
        let (dir, server) = (testing::temp_dir("rdio"), rdio());
        let out = testing::run::<RdioScanner>([
            hello(&dir, server.url(), json!({ "apiKey": "good", "systemId": 1, "talkgroupAllow": ["1??"], "talkgroupDeny": [102] })),
            HostMessage::CallConcluded(testing::call(&dir, "sys1", 101)),
            HostMessage::CallConcluded(testing::call(&dir, "sys1", 102)),
            HostMessage::CallConcluded(testing::call(&dir, "sys1", 1001)),
        ]);
        let skipped = out.results().iter().filter(|r| r.1 == Outcome::Skipped).count();
        assert_eq!(skipped, 2);
        assert_eq!(server.requests().len(), 1);
    }

    #[test]
    fn a_server_that_is_down_keeps_the_call_for_next_time() {
        let dir = testing::temp_dir("rdio");
        let out = testing::run::<RdioScanner>([
            hello(&dir, "http://127.0.0.1:9", json!({ "apiKey": "good", "systemId": 1 })),
            HostMessage::CallConcluded(testing::call(&dir, "sys1", 5)),
        ]);
        assert!(out.results().is_empty(), "{:?}", out.results());
        assert!(matches!(out.status(), Some((State::Warning, _))));
    }

    #[test]
    fn needs_a_server_and_a_system_id() {
        let dir = testing::temp_dir("rdio");
        let out = testing::run::<RdioScanner>([hello(&dir, "", json!({ "apiKey": "good", "systemId": 1 }))]);
        assert_eq!(out.exit_code, EXIT_CONFIG);
        assert!(out.status().unwrap().1.contains("web address"));
        let out = testing::run::<RdioScanner>([hello(&dir, "http://x", json!({ "apiKey": "good" }))]);
        assert_eq!(out.exit_code, EXIT_CONFIG);
        assert!(out.status().unwrap().1.contains("system ID"));
    }
}
