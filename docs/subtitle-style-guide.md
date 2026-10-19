# ASS subtitle style guide

## Project convention

- For a new script created with `create_ass`, keep generated events on `Default` and use local override tags for typesetting. At 1920×1080 this style uses bold Lato 80, near-white primary text, a black 3.75 outline, a partially transparent black 2.25 shadow, bottom-center alignment, and 120/120/60 margins. The generated dimensions scale with script resolution.
- When editing an existing master, preserve its event styles, layers, timing, and override tags unless the requested edit explicitly changes them. Do not normalize an established subtitle to `Default`.
- Treat this as a dialogue baseline, not a universal visual match for signs, credits, or on-screen graphics. Confirm the video resolution and inspect a rendered frame before final delivery.
- For typesetting, keep the event's Style field as `Default` and use local ASS override tags such as `\fn`, `\fs`, `\bord`, `\shad`, `\c`, `\pos`, and `\an` only where the image calls for them. Reset a temporary span with `\rDefault` when later text in the same event should return to the baseline. Avoid creating a separate named style for every sign or effect.
- Choose fonts for the appearance and script of the actual text. A font used for a Japanese-looking Latin sign does not necessarily contain Japanese glyphs. Check glyph coverage and rendering before using a font. A fallback font may change spacing and appearance.
- Keep dialogue readable: use consistent placement, avoid unnecessary effects, preserve timing and line breaks, and check the result against the video. For signs, match the source's approximate shape, weight, color, perspective, and motion when possible.
- Font names in ASS refer to fonts available to the renderer. `create_ass` does not install or embed fonts. Download fonts from their official source and ensure the playback or render environment has them.
- After choosing fonts, use `collect_fonts` to inspect font availability, glyph coverage, and bold/italic variants, then copy installed font files into the project's `fonts/` folder. It organizes filenames, such as `Lato-Bold.ttf`, and returns warnings for unavailable fonts or faces. It copies TTC/OTC and variable fonts unchanged. Review the collected files and their licenses before distribution.

## Suggested Google Fonts for local typesetting

These are starting points for visual matching, not automatic mappings. Font categories and supported scripts are taken from each family's Google Fonts metadata.

| Visual use | Font | Script note |
| --- | --- | --- |
| Main translated dialogue | [Lato](https://fonts.google.com/specimen/Lato) | Latin, Greek, and Cyrillic; no Japanese subset. Already used by `Default`. |
| Bold marker lettering on a Latin-script sign | [Permanent Marker](https://fonts.google.com/specimen/Permanent+Marker) | Handwriting/marker look; Latin only. It does not provide Japanese glyphs. |
| Casual handwritten notes in Latin text | [Caveat](https://fonts.google.com/specimen/Caveat) | Handwriting family with Latin and Cyrillic subsets. |
| Brush-like handwritten Latin note | [Caveat Brush](https://fonts.google.com/specimen/Caveat+Brush) | Latin subsets; stronger, more informal strokes than Caveat. |
| Formal book or document text | [Tinos](https://fonts.google.com/specimen/Tinos) | Serif family designed to be metrically compatible with Times New Roman; useful when a similar text shape is needed. |
| Elegant editorial or period-style sign | [Playfair Display](https://fonts.google.com/specimen/Playfair+Display) or [Cormorant Garamond](https://fonts.google.com/specimen/Cormorant+Garamond) | High-contrast/display serif choices; Latin and Cyrillic subsets. Check thin strokes at subtitle size. |
| Condensed poster or title lettering | [Oswald](https://fonts.google.com/specimen/Oswald) | Narrow sans-serif with Latin and Cyrillic subsets. |
| Tall all-caps storefront or product label | [Bebas Neue](https://fonts.google.com/specimen/Bebas+Neue) | Narrow display family with Latin subsets; useful when space is tight. |
| Heavy advertising or warning sign | [Anton](https://fonts.google.com/specimen/Anton) or [Alfa Slab One](https://fonts.google.com/specimen/Alfa+Slab+One) | Anton is a heavy advertising sans; Alfa Slab One has dense slab serifs. Latin subsets; match the source's letter structure. |
| Bold decorative shopfront or poster | [Bungee](https://fonts.google.com/specimen/Bungee) | Display lettering for short Latin-script text; inspect width and spacing on the frame. |
| Striped marquee or neon-like title | [Monoton](https://fonts.google.com/specimen/Monoton) | Latin-only display family; suited to short, large lettering rather than dialogue. |
| Terminal, code, or monospaced display | [Roboto Mono](https://fonts.google.com/specimen/Roboto+Mono) | Monospaced family with Latin, Greek, and Cyrillic subsets. |
| Retro CRT terminal or pixel display | [VT323](https://fonts.google.com/specimen/VT323) | Latin subsets; check legibility and scale at the target resolution. |
| Futuristic screen or interface label | [Orbitron](https://fonts.google.com/specimen/Orbitron) | Geometric display family with a Latin subset; good for short labels. |
| Japanese handwritten sign or note | [Yuji Boku](https://fonts.google.com/specimen/Yuji+Boku) or [Zen Kurenaido](https://fonts.google.com/specimen/Zen+Kurenaido) | Both include a Japanese subset; compare their distinct brush/pen character to the source. |
| Heavy Japanese poster or package lettering | [Dela Gothic One](https://fonts.google.com/specimen/Dela+Gothic+One) | Very thick display family with a Japanese subset. |
| Rounded Japanese notice or friendly sign | [M PLUS Rounded 1c](https://fonts.google.com/specimen/M+PLUS+Rounded+1c) | Rounded sans-serif family with a Japanese subset. |
| Neutral Japanese text | [Noto Sans JP](https://fonts.google.com/noto/specimen/Noto+Sans+JP) | Japanese subset; useful for untranslated text or Japanese typesetting. |
| Formal Japanese document or literary sign | [Noto Serif JP](https://fonts.google.com/noto/specimen/Noto+Serif+JP) | Japanese serif family with multiple weights. |

Official metadata: [Lato](https://github.com/google/fonts/blob/main/ofl/lato/METADATA.pb), [Permanent Marker](https://github.com/google/fonts/blob/main/apache/permanentmarker/METADATA.pb), [Caveat](https://github.com/google/fonts/blob/main/ofl/caveat/METADATA.pb), [Caveat Brush](https://github.com/google/fonts/blob/main/ofl/caveatbrush/METADATA.pb), [Tinos](https://github.com/google/fonts/blob/main/ofl/tinos/METADATA.pb), [Playfair Display](https://github.com/google/fonts/blob/main/ofl/playfairdisplay/METADATA.pb), [Cormorant Garamond](https://github.com/google/fonts/blob/main/ofl/cormorantgaramond/METADATA.pb), [Oswald](https://github.com/google/fonts/blob/main/ofl/oswald/METADATA.pb), [Bebas Neue](https://github.com/google/fonts/blob/main/ofl/bebasneue/METADATA.pb), [Anton](https://github.com/google/fonts/blob/main/ofl/anton/METADATA.pb), [Alfa Slab One](https://github.com/google/fonts/blob/main/ofl/alfaslabone/METADATA.pb), [Bungee](https://github.com/google/fonts/blob/main/ofl/bungee/METADATA.pb), [Monoton](https://github.com/google/fonts/blob/main/ofl/monoton/METADATA.pb), [Roboto Mono](https://github.com/google/fonts/blob/main/ofl/robotomono/METADATA.pb), [VT323](https://github.com/google/fonts/blob/main/ofl/vt323/METADATA.pb), [Orbitron](https://github.com/google/fonts/blob/main/ofl/orbitron/METADATA.pb), [Yuji Boku](https://github.com/google/fonts/blob/main/ofl/yujiboku/METADATA.pb), [Zen Kurenaido](https://github.com/google/fonts/blob/main/ofl/zenkurenaido/METADATA.pb), [Dela Gothic One](https://github.com/google/fonts/blob/main/ofl/delagothicone/METADATA.pb), [M PLUS Rounded 1c](https://github.com/google/fonts/blob/main/ofl/mplusrounded1c/METADATA.pb), [Noto Sans JP](https://github.com/google/fonts/blob/main/ofl/notosansjp/METADATA.pb), and [Noto Serif JP](https://github.com/google/fonts/blob/main/ofl/notoserifjp/METADATA.pb). Permanent Marker is listed under Apache 2.0; the other suggested families are listed under the SIL Open Font License in their current Google Fonts metadata. Check each font's license when packaging font files.

## Override-tag examples

An event can remain `Default` while its text uses a local font. Examples of ASS `Text` field content:

```ass
{\fnPermanent Marker\fs90\bord2.5\shad1.5}SALE
{\fnTinos\fs76\b0}Chapter One
{\fnYuji Boku\fs88}日本語の看板
{\fnBebas Neue\fs96}OPEN 24 HOURS
{\fnOrbitron\fs72}SYSTEM ONLINE
{\fnCaveat}Handwritten note{\rDefault} — dialogue text
```

These are syntax examples, not presets. Adjust scale, position, color, outline, and shadow against the actual frame. See the [Aegisub override tag manual](https://aegisub.org/docs/latest/ass_tags/) and the [libass ASS format guide](https://github.com/libass/libass/wiki/ASS-File-Format-Guide) for tag behavior.

## MCP workflow

1. Use `create_ass` for a new ASS file or `prepare_subtitle` to back up an existing ASS before editing it in place.
2. Use `create_ass_events` to author new timed lines, or `insert_events_from_source` to bring cues into a new ASS. Imported events use `Default`; ASS sources can retain existing inline overrides, so inspect `get_event` with `include_raw: true` when needed.
3. Translate dialogue with `replace_text`, `replace_text_batch`, or `replace_text_group`. These tools preserve existing tags and do not add new override blocks. Group application is structural; review meaning against Japanese context as needed.
4. Use `update_ass_events` for deliberate time, field, or raw tag edits to existing events. Fetch the current revision, validate the returned diff, and inspect the result in Aegisub or another renderer. Video frame preview is planned in `docs/visual-preview-proposal.md`.
5. Use `audit_subtitle` against a known backup or reference version before delivery. A report is tied to the exact file hashes it compared.
