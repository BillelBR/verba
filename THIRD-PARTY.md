# Third-party software and data

Verba's own code is under the **PolyForm Noncommercial License 1.0.0** (see `LICENSE`; commercial use: `COMMERCIAL.md`).
It builds on the following, which keep their own licenses.

## Rust libraries (checked on 2026-11-16 with `cargo metadata`)

| License | Libraries |
|---|---|
| MIT OR Apache-2.0 | `windows` and its support crates (Microsoft), `serde` family, `syn`, `quote`, `proc-macro2`, `hashbrown`, `toml` family, `version_check` and others |
| MIT | `winresource` (build time only), `winnow` |
| (MIT OR Apache-2.0) AND Unicode-3.0 | `unicode-ident` |

All of them allow free use, modification and redistribution. Only `windows` and `windows-numerics` end up in the program;
`winresource` and the others are used while building.

## Word lists (data, downloaded separately)

| What | License |
|---|---|
| Word-frequency lists (English, French, Arabic) from [hermitdave/FrequencyWords](https://github.com/hermitdave/FrequencyWords), built from OpenSubtitles | **CC-BY-SA-4.0** |
| Optional list of Algerian Arabizi words (`scripts/fetch-dz.ps1`) built from the [NArabizi treebank](https://universaldependencies.org/treebanks/qaf_arabizi/) (Seddah et al. 2020; Riabi et al. 2023) | **CC BY-SA 4.0** |
| Optional list of Algerian words in Arabic letters (`scripts/fetch-dz-ar.ps1`) built from the Mendeley Data set [Algerian Dialect](https://data.mendeley.com/datasets/zzwg3nnhsz/2) (Benmounah and Boulesnane, doi 10.17632/zzwg3nnhsz.2) | **CC BY 4.0** |

They are **not part of the source repository** (`scripts/fetch-data.ps1` downloads them) and they do not change the license of Verba's code.
The Windows installer ships them as separate files in `data\words\` with a notice (`WORD-LISTS-LICENSE.txt`).
If you copy a list into another project, its CC-BY-SA-4.0 terms (attribution, same license for the data) apply to that copy.
See [docs/DATA.md](docs/DATA.md).

## Next-word tables (data, built from a download)

| What | License |
|---|---|
| Next-word counts (English, French, Arabic) built by `scripts/fetch-bigrams.ps1` from sentences of [Tatoeba](https://tatoeba.org), (c) Tatoeba contributors | **CC BY 2.0 FR** (attribution) |

The tables hold counts only, no sentence. They are not part of the source repository. A copy of a table must keep the attribution above.
See [docs/DATA.md](docs/DATA.md).

## Tools used to build the installer (not shipped)

Inno Setup (its own license; the installer it makes is not covered by it).
