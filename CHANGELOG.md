# Changelog

## [0.1.0]

- Uploads recorded calls to Rdio Scanner with the fields Trunk Recorder's
  uploader sends: M4A when there's an encoder, WAV when there isn't.
- Talkgroup allow and deny lists, with `*` and `?`.
- Retries calls the server couldn't take after 10 s, 1 min, 5 min and 15 min,
  and keeps calls still waiting when recording stops for the next start.
