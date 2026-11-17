Vendored without source changes from DeepSeek Harness commit
`39163dd30f703a79079e2d2446545cff3c453738`.

The JSON tree and its transitive UI primitives retain their upstream CSS,
keyboard navigation, menus, copy semantics, and long-string layout. Theme
token sheets come from `packages/client/ui-theme/src/styles`.
KWork integration is in `ui/trajectory-json.tsx`. See LICENSE for MIT terms.

The Markdown plain-text extractor and parser also come from the same commit.
Their code is unchanged except that `local-image-syntax.ts` imports the extracted
classifier from `file-type-classifier.ts` instead of the React icon component.
The classifier and its maps are copied from `FileTypeIcon.tsx`; the independent
`code-file-types.ts` is unchanged. Glyph rendering is not included in that adapter.

The program inspector also includes the upstream CodeBlock, CodeToolbar,
clipboard helper, viewport highlighting, diff fence parser, language table,
and Shiki palette. `highlight.ts` imports the installed `@shikijs/core` and
`@shikijs/engine-javascript` packages in place of the upstream `shiki` re-exports;
`code-highlighting.ts` points to the unchanged local `code-language.ts` table.
