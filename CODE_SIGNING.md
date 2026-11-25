# Code signing policy

## Status

KWork is preparing an application to the SignPath Foundation open-source code signing program. **No approval has been granted yet.** Until a signed release is explicitly announced, Windows preview artifacts remain unsigned.

After approval, Windows releases will use the following attribution:

> Free code signing provided by SignPath.io, certificate by SignPath Foundation

The certificate is issued to SignPath Foundation. KWork does not receive or store the private key.

## Team roles

- **Authors:** [KWork maintainers with repository write access](https://github.com/orgs/kense-labs/people) may modify the source and build configuration.
- **Reviewers:** KWork maintainers review every change proposed by external contributors before merge. Build scripts and CI configuration receive the same review as application code.
- **Approvers:** [Kense Labs organization owners](https://github.com/orgs/kense-labs/people?query=role%3Aowner) approve each release signing request. Every release requires explicit manual approval.

Maintainers must use multi-factor authentication for GitHub and SignPath access.

## Build and signing process

1. Windows binaries are built from this public repository on GitHub-hosted runners by `.github/workflows/windows.yml`.
2. For a SignPath release run, a maintainer manually starts `.github/workflows/windows.yml` on `main` with `sign_release` enabled.
3. **Stage 1 (inner executables):** GitHub Actions uploads the unsigned `KWork.exe` and NSIS-generated `uninstall.exe` as a build artifact and submits it to SignPath. SignPath verifies the GitHub build origin and an authorized approver approves the signing request.
4. **Stage 2 (outer installer):** The workflow verifies the inner Authenticode signatures, repacks the NSIS installer around the signed `KWork.exe` and `uninstall.exe`, submits the outer NSIS installer to SignPath for signing, verifies the outer signature, installs the signed package in the Windows smoke test (asserting valid signatures on the installed `KWork.exe` and `uninstall.exe` on disk), and only then uploads the release artifact.
5. **Microsoft Store MSIX:** `.github/workflows/windows.yml` also produces an unsigned `KWork_<version>_x64.msix` (`KWork-Windows-MSIX`) for Microsoft Partner Center submission; upon Store certification, Microsoft automatically re-signs the MSIX package and its bundled binaries with a Microsoft Store certificate.

For SignPath releases, the signed artifacts are `KWork.exe`, `uninstall.exe`, and the outer NSIS installer. Third-party binaries bundled inside the installer retain their upstream signatures where available. The enforced product name is `KWork`; the product version comes from `src-tauri/tauri.conf.json` and must be identical for all SignPath-signed binaries in a release.

## Installation and removal

The NSIS installer installs KWork for the current Windows user. To remove it, open **Windows Settings → Apps → Installed apps → KWork → Uninstall**. User workspaces and Harness data are not deleted automatically.

## Privacy

See the [KWork privacy policy](PRIVACY.md).
