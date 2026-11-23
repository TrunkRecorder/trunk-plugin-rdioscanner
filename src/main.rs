//! Rdio Scanner — a Trunk Recorder Pro plugin that uploads recorded calls to
//! an [Rdio Scanner](https://github.com/chuot/rdio-scanner) server, as Trunk
//! Recorder's Rdio Scanner uploader does.

mod upload;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use trunk_recorder_plugin::{
    Attempt, CallQueue, ConcludedCall, Host, Manifest, Plugin, QueueOptions, Setup, System, TalkgroupFilter, Talkgroups, format, topic,
};

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
    // The talkgroups to upload: a menu, and the lists it asks for.
    #[serde(flatten)]
    talkgroups: Talkgroups,
}

/// Where a system's calls go.
struct Target {
    /// The server's host: the queue's lane for it.
    host: String,
    uploader: Arc<Uploader>,
    filter: TalkgroupFilter,
    /// The system, with its settings and its sites' ([`System::targets`]).
    system: System<Config, SystemConfig>,
}

/// A system's (or a site's) Rdio Scanner system ID and API key, when it has both.
fn node(c: &SystemConfig) -> Option<(u32, &str)> {
    let key = c.api_key.trim();
    c.system_id.filter(|_| !key.is_empty()).map(|id| (id, key))
}

struct RdioScanner {
    queue: CallQueue,
}

/// Uploads worked on at once, for each server. Every system sending to the
/// same server shares them: at 4800 calls an hour, the SDK's 2 would leave
/// each upload (its audio included) 1.5 s before calls start to wait.
const UPLOAD_THREADS: usize = 4;

/// A server's host (`rdio.example.com:3000`), to name it by.
fn host_of(server: &str) -> String {
    server.split_once("://").map_or(server, |(_, r)| r).split('/').next().unwrap_or("").to_string()
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
            // A system can upload to another server.
            system_overrides: vec!["server".into()],
            // A site that's an Rdio Scanner system of its own.
            site_overrides: vec!["apiKey".into(), "systemId".into()],
            ..trunk_recorder_plugin::manifest!()
        }
    }

    fn start(host: Host, setup: Setup<Config, SystemConfig>) -> Result<Self, String> {
        // One uploader for each server (a system can name a server of its own).
        let mut uploaders: HashMap<String, Arc<Uploader>> = HashMap::new();
        let mut targets = HashMap::new();
        let has_m4a = setup.has_format(format::M4A);
        for s in setup.systems {
            let Some(c) = &s.config else { continue };
            if !c.api_key.trim().is_empty() && c.system_id.is_none() {
                return Err(format!("Add {}'s Rdio Scanner system ID (or clear its API key).", s.short_name));
            }
            for site in s.sites.iter().filter(|t| t.config.is_some()) {
                if site.config.as_ref().and_then(node).is_none() {
                    return Err(format!("Add the Rdio Scanner system ID and API key of {}'s site {}.", s.short_name, site.name));
                }
            }
            let sites: Vec<(&str, u32)> = s.sites.iter().filter_map(|t| Some((t.name.as_str(), t.config.as_ref().and_then(node)?.0))).collect();
            if node(c).is_none() && sites.is_empty() {
                continue;
            }
            let server = s.settings.server.trim().trim_end_matches('/').to_string();
            if server.is_empty() {
                return Err(format!("Add the web address of the Rdio Scanner to upload {} to.", s.short_name));
            }
            if !(server.starts_with("https://") || server.starts_with("http://")) {
                return Err(format!("The server has to be a web address (http://…), not \"{server}\""));
            }
            let uploader = uploaders.entry(server.clone()).or_insert_with(|| Arc::new(Uploader::new(&server))).clone();
            let filter = c.talkgroups.filter_for(&s.short_name)?;
            let filtered = if filter.is_empty() { String::new() } else { format!(", talkgroups: {}", filter.describe()) };
            let to = host_of(&server);
            match node(c) {
                Some((id, _)) => host.info(format!("uploading {} to {to} as system {id}{filtered}", s.short_name)),
                None => host.info(format!("uploading {} to {to} only from its sites with an Rdio Scanner system of their own{filtered}", s.short_name)),
            }
            for (site, id) in &sites {
                host.info(format!("uploading {}'s site {site} as system {id}", s.short_name));
            }
            targets.insert(s.short_name.clone(), Target { host: to, uploader, filter, system: s });
        }
        if targets.is_empty() {
            return Err("Add an Rdio Scanner API key and system ID to the systems you want to upload.".into());
        }
        if !has_m4a {
            host.info("no M4A encoder: uploading WAV");
        }
        // Each server is a lane of the queue: one that's down holds up no other.
        // The dashboard names each by its host.
        let targets = Arc::new(targets);
        let lanes = targets.clone();
        let opts = QueueOptions { threads: UPLOAD_THREADS, noun: "upload", endpoint: Some("Rdio Scanner".into()), ..QueueOptions::saved_in(&setup.data_dir) };
        let lane = move |call: &ConcludedCall| lanes.get(&call.call.short_name).map(|t| t.host.clone()).unwrap_or_default();
        let queue = CallQueue::start_by(host, opts, lane, move |call: &ConcludedCall| {
            // By short name, a system's identity: a call saved for a later run still finds its system.
            let Some(t) = targets.get(&call.call.short_name) else {
                return Attempt::Skip("no Rdio Scanner API key for this system".into());
            };
            if call.call.encrypted {
                return Attempt::Skip("encrypted".into());
            }
            // Kept with none by "Keep calls with no audio": nothing to play.
            if !call.call.has_audio() {
                return Attempt::Skip("no audio".into());
            }
            if !t.filter.passes(call.call.talkgroup) {
                return Attempt::Skip(format!("talkgroup {} isn't uploaded (talkgroup filter)", call.call.talkgroup));
            }
            let audio = call.files.m4a.as_ref().filter(|p| p.is_file()).unwrap_or(&call.files.wav);
            // Each Rdio Scanner system the sites that heard it send to, once.
            let mut nodes: Vec<(u32, &str)> = Vec::new();
            for d in t.system.targets(&call.call) {
                if let Some(n) = node(d.config).filter(|n| !nodes.contains(n)) {
                    nodes.push(n);
                }
            }
            if nodes.is_empty() {
                return Attempt::Skip("none of the sites that heard it has an Rdio Scanner system".into());
            }
            Attempt::together(nodes.into_iter().map(|(system_id, api_key)| t.uploader.upload(&Upload { system_id, api_key, call, audio })))
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
    use trunk_recorder_plugin::{EXIT_CONFIG, HostMessage, Outcome, SiteInfo, SiteRef, State, SystemInfo};

    /// What a system can set again is one of its settings.
    #[test]
    fn a_system_can_set_its_server() {
        let m = trunk_recorder_plugin::describe::<RdioScanner>();
        assert_eq!(m.system_overrides, ["server"]);
        assert_eq!(m.site_overrides, ["apiKey", "systemId"], "a site can be a system of its own");
        assert!(m.system_overrides.iter().all(|k| m.config.as_ref().unwrap()["properties"].get(k).is_some()));
    }

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

    /// A call kept with no audio ("Keep calls with no audio"); a 0.4 s one, whose `call_length` says 0, has some.
    #[test]
    fn calls_with_no_audio_are_skipped() {
        let (dir, server) = (testing::temp_dir("rdio-silent"), rdio());
        let mut silent = testing::call(&dir, "sys1", 101);
        (silent.call.call_length, silent.call.call_length_ms) = (0.0, 0);
        let out = testing::run::<RdioScanner>([hello(&dir, server.url(), json!({ "apiKey": "good", "systemId": 1 })), HostMessage::CallConcluded(silent)]);
        assert_eq!(out.results()[0].1, Outcome::Skipped);
        assert!(server.requests().is_empty());
        let mut short = testing::call(&dir, "sys1", 102);
        (short.call.call_length, short.call.call_length_ms) = (0.0, 400);
        let out = testing::run::<RdioScanner>([hello(&dir, server.url(), json!({ "apiKey": "good", "systemId": 1 })), HostMessage::CallConcluded(short)]);
        assert_eq!(out.results()[0].1, Outcome::Ok, "{:?}", out.results());
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
            hello(&dir, server.url(), json!({ "apiKey": "good", "systemId": 1, "talkgroupMode": "only", "talkgroupAllow": ["1??"], "talkgroupDeny": [102] })),
            HostMessage::CallConcluded(testing::call(&dir, "sys1", 101)),
            HostMessage::CallConcluded(testing::call(&dir, "sys1", 102)),
            HostMessage::CallConcluded(testing::call(&dir, "sys1", 1001)),
        ]);
        let skipped = out.results().iter().filter(|r| r.1 == Outcome::Skipped).count();
        assert_eq!(skipped, 2);
        assert_eq!(server.requests().len(), 1);
    }

    /// The menu: all but a range; and an entry that isn't a talkgroup stops it starting.
    #[test]
    fn talkgroups_chosen_in_the_menu() {
        let (dir, server) = (testing::temp_dir("rdio"), rdio());
        let out = testing::run::<RdioScanner>([
            hello(&dir, server.url(), json!({ "apiKey": "good", "systemId": 1, "talkgroupMode": "allBut", "talkgroupDeny": ["100-199"] })),
            HostMessage::CallConcluded(testing::call(&dir, "sys1", 150)),
            HostMessage::CallConcluded(testing::call(&dir, "sys1", 250)),
        ]);
        assert_eq!(out.results().iter().filter(|r| r.1 == Outcome::Ok).count(), 1);
        assert_eq!(String::from_utf8(server.requests()[0].form_field("talkgroup").unwrap()).unwrap(), "250");
        let out = testing::run::<RdioScanner>([hello(&dir, server.url(), json!({ "apiKey": "good", "systemId": 1, "talkgroupMode": "only" }))]);
        assert_eq!(out.exit_code, EXIT_CONFIG);
        assert!(out.status().unwrap().1.contains("sys1's talkgroups: list the talkgroups"), "{:?}", out.status());
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

    /// A site with an Rdio Scanner system of its own: the calls it heard go there, whichever site's
    /// copy was kept; those only the others heard, to the system's.
    #[test]
    fn a_site_of_its_own() {
        let dir = testing::temp_dir("rdio");
        // (The recorder sends each site's settings as the system's with what it sets again.)
        let run = |system: Value, south: Value, heard: &[(u32, &str)]| {
            let server = rdio();
            let mut h = testing::hello(&dir, json!({ "server": server.url() }));
            h.systems[0].config = system;
            h.systems[0].sites = vec![SiteInfo { id: 7, name: "north".into(), config: Value::Null }, SiteInfo { id: 8, name: "south".into(), config: south }];
            let mut call = testing::call(&dir, "sys1", 103);
            call.call.site = heard[0].1.into();
            call.call.sites = heard.iter().map(|&(id, name)| SiteRef { id, name: name.into() }).collect();
            let out = testing::run::<RdioScanner>([HostMessage::Hello(h), HostMessage::CallConcluded(call)]);
            let mut ids: Vec<String> = server.requests().iter().filter_map(|r| r.form_field("system")).map(|v| String::from_utf8(v).unwrap()).collect();
            ids.sort();
            (ids, out.results().into_iter().map(|r| r.1).collect::<Vec<_>>(), format!("{out:?}"))
        };
        let system = json!({ "apiKey": "good", "systemId": 42 });
        let south = json!({ "apiKey": "good", "systemId": 77 });
        assert_eq!(run(system.clone(), south.clone(), &[(7, "north"), (8, "south")]).0, ["42", "77"], "north has none of its own: the system's; south's own");
        assert_eq!(run(system.clone(), south.clone(), &[(8, "south")]).0, ["77"]);
        assert_eq!(run(system.clone(), south.clone(), &[(7, "north")]).0, ["42"]);
        // No system of the whole system's: only what south heard goes.
        let only = json!({ "systemId": 42 });
        assert_eq!(run(only.clone(), south.clone(), &[(7, "north"), (8, "south")]).0, ["77"]);
        let (ids, results, _) = run(only, south, &[(7, "north")]);
        assert_eq!((ids, results), (Vec::<String>::new(), vec![Outcome::Skipped]));
        // A site of its own needs its system ID and key.
        assert!(run(system, json!({ "apiKey": "good" }), &[(8, "south")]).2.contains("site south"));
    }

    /// A system can send its calls to a server of its own.
    #[test]
    fn a_system_overrides_the_server() {
        let (dir, main, club) = (testing::temp_dir("rdio"), rdio(), rdio());
        let mut h = testing::hello(&dir, json!({ "server": main.url() }));
        h.systems[0].config = json!({ "apiKey": "good", "systemId": 1 });
        h.systems.push(SystemInfo {
            index: 1,
            short_name: "sys2".into(),
            kind: "p25".into(),
            config: json!({ "apiKey": "good", "systemId": 2 }),
            settings: json!({ "server": club.url() }),
            ..Default::default()
        });
        let out = testing::run::<RdioScanner>([
            HostMessage::Hello(h),
            HostMessage::CallConcluded(testing::call(&dir, "sys1", 101)),
            HostMessage::CallConcluded(testing::call(&dir, "sys2", 202)),
        ]);
        assert!(out.results().iter().all(|r| r.1 == Outcome::Ok), "{:?}", out.results());
        let system = |s: &MockServer| s.requests().iter().map(|r| String::from_utf8(r.form_field("system").unwrap()).unwrap()).collect::<Vec<_>>();
        assert_eq!(system(&main), ["1"]);
        assert_eq!(system(&club), ["2"]);
        // Each server has its own endpoint status.
        let mut names: Vec<String> = out.metrics().last().unwrap().endpoints.iter().map(|e| e.name.clone()).collect();
        let mut want = [main.url(), club.url()].map(|u| format!("Rdio Scanner at {}", host_of(u)));
        names.sort();
        want.sort();
        assert_eq!(names, want);
    }
}
