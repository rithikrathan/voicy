# Voicy Agent Rules & Execution Guidelines

These instructions apply whenever a prompt arrives with `/btw` or voice instruction:

## 1. Zero Verifications / Zero Probing
- Never waste turns checking if a tool, binary, daemon, or script exists (e.g. do not run `which`, `whereis`, `ps aux`, or `--help` just to see if a command is there). Assume all mentioned tools exist in `PATH` or `~/.local/bin/`.
- Never verify preconditions unless an operation explicitly fails. Execute the requested action directly.

## 2. Music & Media Playback
- Assume music is already playing; do not check playback status or search for running Spotify processes before acting.
- **Change / Play Song**: Use `play-spotify "<query | uri | url>"` directly (located in `~/.local/bin/play-spotify`). Do not check if the command exists.
- **Playback Control**: Use `playerctl` directly for playback controls:
  - Pause: `playerctl pause`
  - Resume: `playerctl play`
  - Play/Pause toggle: `playerctl play-pause`
  - Next track: `playerctl next`
  - Previous track: `playerctl previous`
  - Volume: `playerctl volume <level>` (e.g. `playerctl volume 0.1+` or `0.1-`)

## 3. Spoken Responses & TTS
- When instructed to respond via voice/speech:
  - Use the `voicy-tts` MCP tool if available.
  - If the MCP tool fails or is unavailable, do NOT attempt to inspect, debug, or fix it. Immediately fall back to `voicy speak "<text>"` (located in `~/.local/bin/voicy`).
- Never print duplicated text to the screen when using TTS.

## 4. Execution Style
- Execute requests directly and stop immediately.
- Zero trailing talk, conversational closers, or sign-offs.
