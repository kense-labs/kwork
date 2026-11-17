Vendored from DeepSeek Harness commit `39163dd30f703a79079e2d2446545cff3c453738`: ui-settings-plugins and ui-settings-plugin-inventory. MIT license: ../harness-primitives/LICENSE.

Components, styles, search and locale dictionaries are unchanged; only runtime import paths point to the corresponding local upstream primitives and text search. KWork mounting and native ACP transport live in ui/settings-plugins.tsx. KWork bundles its client code, so it has no dynamic client module synchronization; Host phases and optional per-preset composition come from the live Loader.
