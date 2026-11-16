# Data and licenses

Verba's code and the word lists it uses are separate things with separate licenses.
The lists are **not stored in this repository**: `scripts/fetch-data.ps1` downloads them into `data/words/` (ignored by git).

## Word lists (local engine, v0)

| Language | File | Source | License of the list |
|---|---|---|---|
| English | `data/words/en.txt` | [hermitdave/FrequencyWords](https://github.com/hermitdave/FrequencyWords), `content/2018/en/en_50k.txt` | CC-BY-SA-4.0 |
| French | `data/words/fr.txt` | same project, `content/2018/fr/fr_50k.txt` | CC-BY-SA-4.0 |
| Arabic | `data/words/ar.txt` | same project, `content/2018/ar/ar_50k.txt` | CC-BY-SA-4.0 |
| Algerian Darija in Latin letters, **built in** (seed) | `crates/verba-local/seed/dz-seed.txt` (embedded in the program) | written by the Verba authors, no corpus | MIT OR Apache-2.0 |
| Algerian Darija in Latin letters, **optional** | `data/words/dz.txt`, built by `scripts/fetch-dz.ps1` | [NArabizi treebank](https://universaldependencies.org/treebanks/qaf_arabizi/) (UD_Maghrebi_Arabic_French-Arabizi): the tokens labelled Algerian, counted | CC BY-SA 4.0 |

- The project's code is MIT; its generated lists are CC-BY-SA-4.0 (checked on 2026-11-16 in its README).
- The lists were built from OpenSubtitles (opus.nlpl.eu/OpenSubtitles2018.php): they are spoken, subtitle-style language.
- **Attribution:** word frequency lists from hermitdave/FrequencyWords, CC-BY-SA-4.0.
- **ShareAlike:** keeping the lists as a separate download keeps this license from reaching Verba's code. If a list is ever copied into the repository or into a release file, that file carries CC-BY-SA-4.0.

## Algerian Darija (Arabizi)

- **Seed, in the program:** `crates/verba-local/seed/dz-seed.txt` is original work of the Verba authors, so it carries the project's license
  (MIT OR Apache-2.0) and is embedded in the binary: Darija works with no download. It is an estimate (Algerian Arabizi has no standard spelling and no
  free frequency list): [ACCURACY.md](ACCURACY.md) says how far to trust it. It holds Algerian vocabulary; what is shared with Moroccan or Tunisian, or unsure, is in its `UNSURE` section.
- **Optional download:** `scripts/fetch-dz.ps1` downloads the NArabizi treebank (Seddah et al., ACL 2020; Riabi et al., 2023; the Universal Dependencies release) into
  `data/dz/narabizi/` and writes `data/words/dz.txt` (Algerian-labelled words seen at least twice). `LocalEngine::load_dz` merges it with the seed. License text,
  from the treebank's `LICENSE.txt`: "The treebank is licensed under the Creative Commons License Attribution-ShareAlike 4.0 International." (CC BY-SA 4.0).
  Its text is comments from an Algerian newspaper's web forum (Cotterell et al., 2014) and song lyrics; whatever rights the original writers have in
  it are not something this project can vouch for. Both reasons keep it a separate download that never enters the repository or a release file.
- **Attribution:** NArabizi corpus, Seddah et al. 2020 and Riabi et al. 2023, CC BY-SA 4.0.
- Looked at and not used (not word lists): ELNER-DZ (named entities, CC BY 4.0) and a 45 000-comment YouTube sentiment dataset (Mendeley Data, CC BY 4.0, mostly Arabic script).

## Known limits of these lists

- **Arabic:** subtitle text mixes Modern Standard Arabic with everyday speech and contains many forms with attached prefixes. Suggestions are usable, not ideal. A list built from news or Wikipedia text would suit Modern Standard Arabic better. Its license must be checked before it is used.
- **All three:** they contain names, interjections and some noise. A cleaning pass and a proper lexicon (for example Hunspell dictionaries, each with its own license) are planned.

## Not used

- NLLB-200 and other models licensed for non-commercial use only.
