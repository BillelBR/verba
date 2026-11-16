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

## Where Verba stands (license: PolyForm Noncommercial 1.0.0)

SignPath Foundation only signs projects under an OSI-approved open-source license with no commercial dual licensing. Verba is source-available (PolyForm Noncommercial) and sells commercial licenses, so **the free SignPath route is closed** while that license stays.
If the license ever goes back to an OSI one (MIT, Apache-2.0, AGPL without a commercial option), the route reopens: see the conditions above.

Left for Verba:

1. **A paid certificate** in the maintainer's own name (the publisher then shows that name). Roughly 200 to 500 USD a year, with an identity check. Check what the issuer accepts from your country before paying.
2. **Azure Artifact Signing**, 9.99 USD a month, only if the maintainer is in a supported country (USA, Canada for individuals).
3. **Stay unsigned** and explain the warning in `INSTALL.md` (done). Reputation then builds slowly with downloads.

Whichever is chosen, the release workflow signs `verba.exe` before Inno Setup packs it, and the installer after; the `.sha256` is computed on the signed file.

## What to avoid

- Do not buy a certificate from a reseller that sends the private key by e-mail: certificate keys now live on hardware or in a cloud HSM.
- Do not tell users to turn SmartScreen or the antivirus off.
