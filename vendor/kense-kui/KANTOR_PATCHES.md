# Frontend patches to kense-kui 0.1.0 (MIT)

Vendored from `https://github.com/kense-labs/kui` (`registry/`) at commit
`da08b406d5eb0a97a3f4b035cececd9e8fa58bb8`.

Changes (all kept strictly in this frontend branch):

1. `Cargo.toml`: inline KUI workspace package metadata (`version = "0.1.0"`,
   `edition = "2024"`, `license = "MIT"`) and align `gpui-pre` / `gpui-pre-platform`
   from upstream's `=0.3.10` to `0.3.11` so `kense-kui` (`kui`) and Kantor
   (`gpui-kit` 0.9 / `docs_ui` / `sheets_ui` / `slides_ui`) resolve to the exact
   same `gpui-pre 0.3.11` crate (`Window`, `App`, `Entity<T>`, `AnyElement`).
