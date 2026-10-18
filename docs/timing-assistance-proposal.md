# Video-aware subtitle timing proposal

Status: proposal for a future implementation. No timing behavior is implemented by this document.

## Goal

Help align dialogue subtitles with nearby shot boundaries and close very short gaps between consecutive dialogue events. The tool should propose frame-accurate changes, explain each change, and apply only reviewed changes to an authorized ASS subtitle after a backup has been created.

## Terminology and inputs

- A **codec keyframe** is a video decoding point. It is a useful hint, but it is not proof of a shot change: encoders can insert keyframes within a continuous shot.
- A **shot boundary** is the first frame of a new visual shot. Reliable boundaries may come from a manually reviewed keyframe list or a video scene detector. Automatically detected boundaries should carry confidence and remain reviewable.
- A **frame boundary** is the timestamp at which a frame begins. Represent an event as a half-open interval `[start, end)`. To keep a subtitle visible through the last frame of a shot, set its end to the start boundary of the first frame in the next shot.
- Inputs should include the prepared ASS subtitle, video or frame-time data, shot boundaries, and any offset between the video and subtitle timelines. For variable frame rate video, use actual frame timestamps rather than a single nominal FPS value.

## Proposed rules

1. **Start at a nearby shot boundary.** If dialogue begins within a configurable window of a confirmed shot boundary, propose moving its start to the first frame of that shot. Do not move the text far ahead of the corresponding audio or reveal a line before its intended moment.
2. **End at a nearby shot boundary.** If dialogue ends near the following shot boundary, propose setting its end to that boundary, making the previous frame its last visible frame. Do not carry a line into another shot unless the dialogue itself continues across the cut or a reviewer accepts the change.
3. **Close short gaps between dialogue events.** If the next dialogue event starts shortly after the current one ends, propose extending the current event to the next event's start. An exact shared boundary produces a continuous subtitle display with no blank frame. Make the maximum gap configurable; do not close a deliberate pause or force continuity across an incompatible shot boundary.
4. **Preserve valid timing.** Never create an end before a start, an unintended overlap, or a duration that violates configured reading-time limits. Skip or flag ambiguous cases involving multiple speakers, simultaneous events, signs, karaoke, animated typesetting, or a dialogue line that crosses a shot boundary.

The three rules interact. Shot alignment should take precedence over gap closing when both target the same endpoint. A small gap alone does not establish that adjacent events belong to one continuous exchange.

## Review and MCP workflow

- Provide an analysis operation that returns proposals without writing files. Each proposal should identify event ID, original times, proposed times, frame positions, detected shot boundary or neighboring event, the rule applied, confidence, and any conflict or skipped reason.
- Accept configurable windows for starts, ends, and short gaps. Keep frame-gap policy explicit: this project may prefer an exact shared boundary for continuous dialogue, while delivery specifications can require a minimum gap such as two frames.
- Apply only selected proposals in one atomic batch to a subtitle prepared in the current MCP session. Require the expected subtitle revision, validate neighboring events together, and reject stale or conflicting proposals before writing.
- Preserve all ASS fields, tags, styles, line endings, and unrelated events. The timestamped backup and video remain unchanged. Do not write persistent intermediate JSON files.
- Allow a dialogue-style filter so signs and special effects are excluded by default.

## Frame and timestamp handling

Keep calculations in frame indices and exact frame timestamps until the final ASS serialization step. ASS event times use centiseconds, so conversion must preserve the intended visible frames and avoid accidental one-frame gaps or overlaps. Validate results against the frame timeline after serialization. Treat video/subtitle offsets and variable frame rate as explicit inputs rather than assuming they are zero or constant.

## Suggested implementation stages

1. Parse a supplied, reviewed shot-boundary list and frame-time map. Implement read-only timing proposals and tests for fixed and variable frame rate videos.
2. Add atomic application to registered ASS subtitles, with revision checks and neighboring-event validation.
3. Add optional video-based scene detection. Cache detection results in memory during the session, expose confidence, and require review of uncertain cuts.
4. Validate the workflow on representative dialogue, scene cuts, intentional pauses, overlapping speakers, and typesetting events. Compare the rendered result with the video and audio before choosing default thresholds.

## References

- [Aegisub Timing Post-Processor](https://aegisubdocs.zahuczky.com/3.1/Timing_Post-Processor/index.htm): adjacent-line continuity and keyframe snapping.
- [Aegisub video manual](https://aegisubdocs.zahuczky.com/manual/Video.html): keyframes are not necessarily scene changes.
- [FFmpeg scene detection filters](https://ffmpeg.org/ffmpeg-filters.html): candidate shot-boundary detection.
- [Netflix subtitle timing guidelines](https://partnerhelp.netflixstudios.com/hc/en-us/articles/360051554394-Timed-Text-Style-Guide-Subtitle-Timing-Guidelines): an example delivery policy for audio sync, shot changes, and frame gaps. Its thresholds should not be assumed to be universal project defaults.
