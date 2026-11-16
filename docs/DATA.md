# Data and licenses

Verba's code and the word lists it uses are separate things with separate licenses.
The lists are **not stored in this repository**: `scripts/fetch-data.ps1` downloads them into `data/words/` (ignored by git).

## Word lists (local engine, v0)

| Language | File | Source | License of the list |
|---|---|---|---|
| English | `data/words/en.txt` | [hermitdave/FrequencyWords](https://github.com/hermitdave/FrequencyWords), `content/2018/en/en_50k.txt` | CC-BY-SA-4.0 |
| French | `data/words/fr.txt` | same project, `content/2018/fr/fr_50k.txt` | CC-BY-SA-4.0 |
| Arabic | `data/words/ar.txt` | same project, `content/2018/ar/ar_50k.txt` | CC-BY-SA-4.0 |

- The project's code is MIT; its generated lists are CC-BY-SA-4.0 (checked on 2026-11-16 in its README).
- The lists were built from OpenSubtitles (opus.nlpl.eu/OpenSubtitles2018.php): they are spoken, subtitle-style language.
- **Attribution:** word frequency lists from hermitdave/FrequencyWords, CC-BY-SA-4.0.
- **ShareAlike:** keeping the lists as a separate download keeps this license from reaching Verba's code. If a list is ever copied into the repository or into a release file, that file carries CC-BY-SA-4.0.

## Known limits of these lists

- **Arabic:** subtitle text mixes Modern Standard Arabic with everyday speech and contains many forms with attached prefixes. Suggestions are usable, not ideal. A list built from news or Wikipedia text would suit Modern Standard Arabic better. Its license must be checked before it is used.
- **All three:** they contain names, interjections and some noise. A cleaning pass and a proper lexicon (for example Hunspell dictionaries, each with its own license) are planned.

## Not used

- NLLB-200 and other models licensed for non-commercial use only.
