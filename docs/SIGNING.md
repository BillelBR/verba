# Code signing and the SmartScreen warning

## What the user sees, and why

An installer that is not signed by a certificate Windows trusts makes SmartScreen say **"Windows protected your PC. Unknown publisher"**.
Nothing in Verba's code can remove that message. Only two things can:

1. **A code-signing certificate** that Windows trusts. This removes "Unknown publisher".
2. **Reputation**: downloads that nobody reported. Without a certificate every new file starts from zero again.

Verba also installs a low-level keyboard hook (to read Tab and Esc while its popup is shown). Some antivirus programs are wary of any program with such a hook. A signed, open-source, automatically built release is the best answer to that too.

## Options (checked on 2026-11-16)

| Option | Cost | Who can use it | Notes |
|---|---|---|---|
| **SignPath Foundation** ([terms](https://signpath.org/terms)) | Free for open-source projects | Any open-source project that meets the conditions below | The certificate belongs to SignPath Foundation, so the publisher shown is theirs. Signing is done by their service from the GitHub Actions run. |
| **Azure Artifact Signing** (formerly Trusted Signing) | 9.99 USD a month (Basic, 5,000 signatures) | Individuals only in the USA and Canada; organisations also in the EU and UK (one blog's report, check Microsoft's current docs) | Needs an identity check. Billing starts when the account is created. |
| **A commercial certificate** (OV/EV) | Roughly 200 to 500 USD a year | Anyone Microsoft's partner authorities can verify | Hardware token or cloud HSM, identity check. |

The conditions SignPath Foundation states: an OSI-approved license with no commercial dual licensing and no proprietary parts; the project is actively maintained and **already released** in the form to be signed; binaries are built from source in a verifiable automated build (GitHub Actions); the signing team owns the repository and signs only its own code; software that sends user data must have a privacy policy; installation comes with uninstallation; **multi-factor authentication** for everyone with access; defined roles (authors, reviewers, approvers); a **code signing policy on the project's home page** that includes "Free code signing provided by SignPath.io, certificate by SignPath Foundation"; product name and version metadata set the same way in every build.

## Where Verba stands against SignPath's conditions

| Condition | State |
|---|---|
| OSI license, no dual commercial license, no proprietary code | Yes: MIT OR Apache-2.0. The word lists are separate downloads (CC-BY-SA-4.0). |
| Already released, maintained | Yes: v0.3.0 is released; the repository must be public first. |
| Verifiable automated build | Yes: `.github/workflows/release.yml` builds on a tag. |
| Privacy | Yes: Verba has no network code (see Privacy in `INSTALL.md`). |
| Uninstallation | Yes: the installer registers an uninstaller. |
| Product name and version metadata | Yes: `build.rs` writes them from Cargo.toml. |
| MFA on GitHub for every maintainer | **To do by the maintainer.** |
| Code signing policy on the home page (README) | **To do** once accepted: roles and the attribution line. |

## Plan

1. Make the repository public (the maintainer decides when). Turn on two-factor authentication for the GitHub account.
2. Apply to SignPath Foundation with the repository link.
3. When accepted, add their GitHub Action to `.github/workflows/release.yml` between "build the installer" and "publish the release": upload the unsigned `Verba-Setup-<version>.exe` as an artifact, let SignPath sign it, download the signed file, then compute the `.sha256` of the **signed** file. Also sign `verba.exe` itself before Inno Setup packs it.
4. Add the code signing policy to the README.

Until then the installer stays unsigned, and `docs/INSTALL.md` tells users how to continue (More info, Run anyway) and how to check the SHA-256.

## What to avoid

- Do not buy a certificate from a reseller that sends the private key by e-mail: certificate keys now live on hardware or in a cloud HSM.
- Do not tell users to turn SmartScreen or the antivirus off.
