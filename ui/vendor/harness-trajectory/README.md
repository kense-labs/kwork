Timeline and toolbar source from DeepSeek Harness commit
`39163dd30f703a79079e2d2446545cff3c453738`.

`TrajectoryTimeline.module.css`, `TrajectoryToolbar.module.css`, and `timeline.ts`
are unchanged. The only change
in `TrajectoryTimeline.tsx` redirects the Tooltip import to the identical vendored
primitive. `locales.ts` retains the upstream Chinese dictionary; the host supplies
the translator type. `TrajectoryToolbar.tsx` redirects the search icon import to
the vendored primitive and uses the host translator type; its rendering and
interaction logic are unchanged. `trajectory-record.ts` contains the unchanged metric type
and duration formatter plus the timeline subset of the host record interface.
`layout.ts` contains the timeline subset of the host layout interface.

Integration, measured positioning, and host event bridging live in
`ui/trajectory-timeline.tsx` and `ui/src/web/trajectory/timeline.rs`.
See LICENSE for MIT terms.

`trajectory-preview.ts` redirects only the plain-text extractor import to the
vendored GFM parser. Rust ledger rows call it through `trajectory-json.tsx` so
Markdown punctuation, links, code, images and truncation follow upstream.

`code-program.ts` is from the same pinned revision. Only the imported host
record type is narrowed to its four consumed fields. Source, description and
recorded-language resolution are unchanged.
