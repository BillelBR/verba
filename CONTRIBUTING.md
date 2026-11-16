# Contributing to Verba

Thank you for helping. Bug reports, word-list fixes, tests and code are all welcome.

## The one rule about the license

Verba is licensed under the PolyForm Noncommercial License 1.0.0 and the maintainer also sells commercial licenses (see [COMMERCIAL.md](COMMERCIAL.md)).
For that to work, the maintainer must be able to license every line of the code both ways.

So when you send code (a pull request, a patch), you agree that:

1. You wrote it, or you have the right to give it.
2. You license your contribution to the maintainer (BillelBR) and to everyone who receives Verba under the PolyForm Noncommercial License 1.0.0, **and** you give the maintainer the right to also license it under other terms, including commercial licenses, now and in the future.
3. You keep the copyright on your contribution.

Say so in your pull request with this line: `I agree to the contribution terms in CONTRIBUTING.md.`
A pull request without it cannot be merged.

Small things that carry no creative work (a typo, one word in a list) do not need it.

## Practical

- Run `cargo test --workspace` before you send a change. The word lists must be downloaded first (`scripts/fetch-data.ps1`).
- Keep changes small and say what they fix.
- Do not add code or data from a source whose license you have not checked; say where it comes from.
