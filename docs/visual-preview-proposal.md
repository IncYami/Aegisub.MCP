# Video frame and ASS visual preview proposal

Status: proposal for a future implementation. The current MCP server does not extract video frames or render ASS over video.

## Goal

Let an agent inspect a real video frame with the selected subtitle revision rendered at the correct timestamp. This supports sign translation, font and glyph checks, placement, color, layering, and timing review. It does not replace human review of motion or full-scene continuity.

## Proposed inputs and result

- Accept a video path, subtitle path, exact subtitle revision, timestamp or event ID, and an explicit video/subtitle time offset.
- Extract the frame at the requested video time and render all active ASS events over it using a compatible renderer. Return image content, video and subtitle timestamps, effective resolution, active event IDs, and warnings for unavailable fonts or glyphs.
- Keep the original video and subtitle unchanged. Temporary frames and renders should be confined to a managed temporary directory or streamed in memory and removed after delivery.
- Do not assume one frame proves a sign's complete text, motion, or timing. Allow several timestamps when an event changes across its duration.

## Review workflow

1. Read a relevant event and its context track. Resolve the event's video time using the explicit offset.
2. Request a preview at one or more representative times, including any animated-sign endpoints.
3. Compare the Japanese sign visible in the frame with the Japanese context text. Treat a time match as a candidate association, not as proof of matching wording.
4. If the sign text differs, review that occurrence individually even when the English master text belongs to an exact-text translation group.
5. Recheck the subtitle revision after any edit. A preview is tied to the revision it rendered and must not be reused as evidence after a newer edit.

## Implementation considerations

- Separate frame extraction from ASS rendering so the renderer can be tested against known reference frames. FFmpeg can decode frames; libass or a compatible renderer can composite ASS. The MCP should report the actual tools and versions used.
- Resolve fonts through fontconfig and report missing families, glyphs, or requested bold/italic faces. Compare the rendered result with Aegisub when output differs.
- Handle variable frame rate by using decoded frame timestamps. Do not infer a frame from a nominal FPS alone.
- Cap frame dimensions, output size, and batch size so an agent cannot accidentally return an entire video as MCP images.
- Keep visual assessment advisory. Rendering success does not prove translation accuracy or that every scene was reviewed.

## Acceptance criteria

- Preview a dialogue and an overlapping sign on a real frame with the expected layer order.
- Return the correct frame after applying a nonzero subtitle/video offset.
- Flag missing fonts or glyphs and show a changed sign position after an ASS edit.
- Reject a stale subtitle revision and never write to the video or subtitle.
- Compare representative static and animated signs against known Aegisub or libass renders.
