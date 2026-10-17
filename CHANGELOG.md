# Changelog

## [0.1.4]

- A system's sites: Trunk Recorder Pro now has systems with several sites, and a
  call heard on more than one is saved once and sent once, to the system's Rdio
  Scanner system.
- Built with trunk-recorder-plugin 0.4.0.

## [0.1.3]

- A system can upload to a server of its own (the recorder's per-system override of the server), and each server has its own upload queue, so one that's down doesn't hold up the others. The dashboard shows each server's status.
- Built with trunk-recorder-plugin 0.3.0.

## [0.1.1]

- The server, and each system's API key and System ID, are marked as needed, so the recorder shows what's left to set up.
- Built with trunk-recorder-plugin 0.1.1.

## [0.1.0]

- Uploads recorded calls to Rdio Scanner with the fields Trunk Recorder's
  uploader sends: M4A when there's an encoder, WAV when there isn't.
- Talkgroup allow and deny lists, with `*` and `?`.
- Retries calls the server couldn't take after 10 s, 1 min, 5 min and 15 min,
  and keeps calls still waiting when recording stops for the next start.
