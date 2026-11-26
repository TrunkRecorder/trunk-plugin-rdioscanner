# Changelog

## [0.1.10]

- **Short, stable reasons for skipped and failed calls.** Trunk Recorder Pro after 0.5.1 counts a plugin's
  calls by the reason it gives, on its card: a filtered talkgroup is now **talkgroup filter** (no longer with
  its number; the log says which talkgroup, as an info line), and an audio file that can't be read no longer
  names its path (the recorder's log of the failure names the call).

## [0.1.9]

- Calls with no audio are no longer uploaded (the recorder keeps them when **Keep calls with no
  audio** is on). They are marked skipped.
- A busy Rdio Scanner (`429`/`503`) that says how long to wait (`Retry-After`) isn't sent the call again
  sooner: uploads read its answers as every plugin now does (trunk-recorder-plugin's
  `Attempt::from_status`). Built with trunk-recorder-plugin 0.15.0.
- A site can be an Rdio Scanner system of its own: its own **API key** and **System ID**, set under
  the site on its system's card. Calls it heard go there; those only the other sites heard go to the
  system's. A call heard by both goes to each, once.

## [0.1.7]

- Four uploads at once for each server, up from two. Every system sending to the same server shares
  them, and at a few thousand calls an hour two couldn't keep up, so calls waited and reached
  Rdio Scanner late.

## [0.1.6]

- **Talkgroups** is a menu on each system's card: **Every talkgroup**, **Only some** (with
  **Except these**) or **All but some**, each list shown when it applies. Lists take ranges
  (`1000-1099`) as well as numbers and patterns, separated by commas or spaces, and the plugin
  won't start with an entry that's none of them, saying which. **Breaking:** lists saved without a
  menu choice no longer apply: choose the menu on each system's card.
- Built with trunk-recorder-plugin 0.8.0 (its `Talkgroups`), for Trunk Recorder Pro v0.2.10.

## [0.1.5]

- Built with trunk-recorder-plugin 0.5.0, for Trunk Recorder Pro v0.2.0: systems are numbered
  from 0 there, conventional ones too, and the call record names every field the recorder writes.

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
