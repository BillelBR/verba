# Data and licenses

Verba's code and the word lists it uses are separate things with separate licenses.
The lists are **not stored in this repository**: `scripts/fetch-data.ps1` downloads them into `data/words/` (ignored by git).

## Word lists (local engine, v0)

| Language | File | Source | License of the list |
|---|---|---|---|
| English | `data/words/en.txt` | [hermitdave/FrequencyWords](https://github.com/hermitdave/FrequencyWords), `content/2018/en/en_50k.txt` | CC-BY-SA-4.0 |
| French | `data/words/fr.txt` | same project, `content/2018/fr/fr_50k.txt` | CC-BY-SA-4.0 |
| Arabic | `data/words/ar.txt` | same project, `content/2018/ar/ar_50k.txt` | CC-BY-SA-4.0 |
| Algerian Darija in Latin letters, **built in** (seed) | `crates/verba-local/seed/dz-seed.txt` (embedded in the program) | written by the Verba authors, no corpus | PolyForm Noncommercial 1.0.0 (same as the code) |
| Algerian Darija in Latin letters, **optional** | `data/words/dz.txt`, built by `scripts/fetch-dz.ps1` | [NArabizi treebank](https://universaldependencies.org/treebanks/qaf_arabizi/) (UD_Maghrebi_Arabic_French-Arabizi): the tokens labelled Algerian, counted | CC BY-SA 4.0 |

- The project's code is MIT; its generated lists are CC-BY-SA-4.0 (checked on 2026-11-16 in its README).
- The lists were built from OpenSubtitles (opus.nlpl.eu/OpenSubtitles2018.php): they are spoken, subtitle-style language.
- **Attribution:** word frequency lists from hermitdave/FrequencyWords, CC-BY-SA-4.0.
- **ShareAlike:** keeping the lists as a separate download keeps this license from reaching Verba's code. If a list is ever copied into the repository or into a release file, that file carries CC-BY-SA-4.0.

## Next-word tables (`data/words/{en,fr,ar}.bigrams.txt`)

Used by `LocalEngine::next_words`: after a finished word and a space, the words that most often follow it.
`scripts/fetch-bigrams.ps1` downloads the sentences and builds the tables (run `scripts/fetch-data.ps1` first). Like the word lists, they are
**not stored in this repository** and are never copied into it.

| Language | Sentences | Source | License |
|---|---|---|---|
| English | `eng`, about 2.0 million sentences, 15.7 million words | [Tatoeba](https://tatoeba.org) per-language export `https://downloads.tatoeba.org/exports/per_language/eng/eng_sentences.tsv.bz2` | CC BY 2.0 FR |
| French | `fra`, about 730 000 sentences, 5.2 million words | same, `.../fra/fra_sentences.tsv.bz2` | CC BY 2.0 FR |
| Arabic | `ara`, about 69 000 sentences, 0.3 million words | same, `.../ara/ara_sentences.tsv.bz2` | CC BY 2.0 FR |

- **License, checked on 2026-11-16** on tatoeba.org (terms of use and downloads page): "These files are released under CC BY 2.0 FR."
  ([Creative Commons Attribution 2.0 France](https://creativecommons.org/licenses/by/2.0/fr/)); "A part of our sentences are also available
  under CC0 1.0." The terms of use say the BY condition is that "the name of the author is cited". There is no ShareAlike clause.
  (Some English and Japanese sentences come from the Tanaka Corpus, public domain.)
- **Attribution:** next-word counts derived from sentences of Tatoeba (https://tatoeba.org), Tatoeba contributors, CC BY 2.0 FR.
  The generated files carry this line in their header, and a build or installer that ships them must carry it in its notices
  (`THIRD-PARTY.md` lists it).
- **What the tables hold:** counts only, never a sentence. For each of the 10 000 commonest words (Arabic: whatever occurs), the 6 words
  that most often follow it and how often, and for the 20 000 commonest pairs of words (English, French) the 5 words that follow
  them. A line is `context<TAB>times seen<TAB>word:count word:count ...`. Plain text, a few hundred kilobytes to 1.4 MB per language (measured sizes:
  [ACCURACY.md](ACCURACY.md#next-word-prediction)). Counts of word pairs from a corpus of 2 million sentences cannot be turned back into the sentences.
- **Words that are a trait of the corpus are never predicted.** Tatoeba is full of the names `Tom` and `Mary`; a word that is at least 8
  times more frequent in the sentences than in the subtitle list is left out (English and French also leave out words the subtitle list does not
  hold). It is the only use of the subtitle lists here.
- **Why Tatoeba.** Candidates checked on the same day: the Leipzig Corpora Collection (a web search of its Terms of Usage reports CC BY for the text corpora but
  CC BY-NC for the project's data in general; the site itself sits behind a bot check and could not be read, the same open point as for the Arabic word
  list, see ACCURACY.md, so it is not used); Common Voice sentences (CC0, but contributors report very few Arabic sentences; not examined further);
  Wikipedia dumps (CC BY-SA; the Arabic dump is 2.2 GB and needs wiki markup stripped; not tried). Tatoeba is the one whose license is clear and whose download is small.
- **Known limits:** Tatoeba is made of short translation exercises (about travel, school, family), not of e-mails and chat. The tables are good at
  the small phrases of everyday language (`would like to`, `thank you for`) and weak at the vocabulary of a profession. The Arabic corpus is
  small (0.3 million words) and holds many informal sentences. Tatoeba's Arabic is `ara`, a macro-language code: dialect sentences are in it.
- **Unpacking `.bz2`:** Windows cannot open them, so the script uses Python 3, `bzip2` or 7-Zip, whichever is installed.

## Personal dictionary files (written by the app, not downloaded)

`personal.txt` (one word per line, editable; a line starting with `-` is a word never to learn automatically) and `learned.txt`
(counts of single words and of pairs of words, tab separated). The engine does not know where they live: the app passes the paths to
`PersonalDict::load` and `save`. Nothing else is stored; see [ACCURACY.md](ACCURACY.md#personal-dictionary) for what learning keeps and refuses.

## Algerian Darija (Arabizi)

- **Seed, in the program:** `crates/verba-local/seed/dz-seed.txt` is original work of the Verba authors, so it carries the project's license
  (PolyForm Noncommercial 1.0.0, like the code) and is embedded in the binary: Darija works with no download. It is an estimate (Algerian Arabizi has no standard spelling and no
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
