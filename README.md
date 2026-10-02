# Rdio Scanner for Trunk Recorder Pro

Uploads the calls [Trunk Recorder Pro](https://github.com/TrunkRecorder/trunk-recorder-pro)
records to an [Rdio Scanner](https://github.com/chuot/rdio-scanner) server,
so they can be listened to there. It does what Trunk Recorder's built-in Rdio
Scanner uploader does.

## What it needs

- **An Rdio Scanner server**, with an API key (from the API keys section of its
  administration page) and a system set up for each system you upload.
- **Optionally, an M4A encoder.** Calls are sent as M4A when there's one, and
  as WAV when there isn't. On macOS one is built in; on Linux and Windows,
  install [ffmpeg](https://ffmpeg.org).

## Settings

| Setting | |
|---|---|
| **Server** | Your Rdio Scanner's web address, the one you open it with (like `http://192.168.1.20:3000`). |

For each system:

| Setting | |
|---|---|
| **API key** | An Rdio Scanner API key. Leave it empty to not upload that system. |
| **System ID** | The system's ID in Rdio Scanner. |
| **Only these talkgroups** | Upload these talkgroups and no others. Leave it empty to upload all of them. |
| **Not these talkgroups** | Never upload these talkgroups. |

Talkgroups can be numbers or patterns: `*` stands for any digits and `?` for
one, so `507*` is every talkgroup starting with 507. When both lists are set,
a talkgroup has to be in the first and not in the second.

## What it does with calls

For each recorded call of a system with an API key, it sends Rdio Scanner the
call's audio and the fields Trunk Recorder sends: talkgroup and its names,
frequency, start time, the radios heard, the talkgroups patched with it, and
error counts. Encrypted calls are never sent.

If the server can't be reached, the plugin tries the call again after 10
seconds, 1 minute, 5 minutes and 15 minutes, and shows how many calls are
waiting. Calls still waiting when recording stops are kept, and sent when it
starts again. A call the server refuses (a wrong API key, missing details)
is marked failed and not tried again.

## Coming from Trunk Recorder

Trunk Recorder's plugin settings can be pasted in as they are: `server`, and
each system's `apiKey`, `systemId`, `talkgroupAllow` and `talkgroupDeny`.

## Building

```sh
cargo build --release
trunk-pro plugin run ./target/release/rdioscanner ~/TrunkRecorderPro --settings examples/settings.json
```

## License

GPL-3.0-or-later, like Trunk Recorder.
