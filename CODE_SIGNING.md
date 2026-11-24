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
2. For a signing run, the maintainer manually starts that workflow on `main` with `sign_release` enabled.
3. GitHub Actions uploads the unsigned `KWork.exe` and NSIS installer as a build artifact and submits that artifact to SignPath.
4. SignPath verifies the GitHub build origin. An authorized approver approves the signing request.
5. The workflow downloads the signed binaries, verifies their Authenticode signatures, runs the Windows installation smoke test, and only then uploads the release artifact.

The signed artifacts are the KWork main executable and its NSIS installer. Third-party binaries bundled inside the installer retain their upstream signatures where available. The enforced product name is `KWork`; the product version comes from `src-tauri/tauri.conf.json` and must be identical for all signed binaries in a release.

## Installation and removal

The NSIS installer installs KWork for the current Windows user. To remove it, open **Windows Settings → Apps → Installed apps → KWork → Uninstall**. User workspaces and Harness data are not deleted automatically.

## Privacy

See the [KWork privacy policy](PRIVACY.md).
