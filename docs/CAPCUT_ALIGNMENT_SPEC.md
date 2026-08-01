# RustCut Studio CapCut-inspired alignment

## Intent

Improve RustCut Studio's browser workspace for creators who expect the clarity and speed of a modern online video editor. Use CapCut.com only as a public product-pattern reference; keep RustCut's own name, implementation, APIs, and visual identity. Do not copy CapCut logos, text, images, CSS, or proprietary assets.

## Reference patterns

- Clear editor entry point with a content-first preview and timeline.
- Discoverable AI assistant, captions, text/audio tools, effects, and templates as editing surfaces.
- Dark creator workspace with one obvious primary action: export.
- Progressive disclosure for dense effect controls.

## In-scope implementation

1. Preserve all existing RustCut behavior and API payloads.
2. Rework the web shell to feel like a focused creator workspace: stronger hierarchy, clearer tool grouping, compact metadata, and an obvious export action.
3. Make the UI responsive at 375px, 768px, 1024px, and desktop widths. Mobile must not horizontally scroll; prioritize preview, timeline, and the active tool, with secondary panels collapsible or stacked.
4. Keep every existing form usable by keyboard and touch. Interactive controls need visible focus/pressed states and at least 44px effective hit areas.
5. Keep the existing dark theme, but use semantic CSS tokens, restrained surfaces, consistent 8px spacing, and accessible contrast. Use CSS/SVG only; do not add external assets or emoji icons.
6. Keep motion purposeful and under 400ms; honor `prefers-reduced-motion`.

## Acceptance

- Desktop 1440x1000: media, preview, timeline, AI assistant, caption/effect forms, undo/redo, and export remain visible and usable.
- Tablet 768x1024: no clipped primary controls; panels stack or collapse predictably.
- Mobile 390x844 and 375x812: `document.documentElement.scrollWidth <= window.innerWidth`, no horizontal page scroll, preview and active controls remain usable, and no control is smaller than 44px high.
- `node --check web/app.js` passes; existing Rust/static checks remain unaffected.
- Existing selectors and event handlers continue to work, or are updated consistently in the same change.
- Do not add a fake CapCut brand, remote tracking, or unverified backend features.
