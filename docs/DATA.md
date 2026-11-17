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
| Algerian Darija in Arabic letters, **built in** (seed) | `crates/verba-local/seed/dz-ar-seed.txt` (embedded in the program) | written by the Verba authors, no corpus | PolyForm Noncommercial 1.0.0 (same as the code) |
| Algerian Darija in Arabic letters, **optional** | `data/words/dz-ar.txt`, built by `scripts/fetch-dz-ar.ps1` | [Algerian Dialect](https://data.mendeley.com/datasets/zzwg3nnhsz/2), 45 000 YouTube comments from Algerian press channels (Benmounah and Boulesnane, Mendeley Data V2, doi 10.17632/zzwg3nnhsz.2): the words the Arabic list lacks, counted | CC BY 4.0 |

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

## Word translation (`data/words/dict.txt`)

Used by `LocalEngine::translate_word`: a selected word is looked up and its translations are offered (English, French, Arabic, in every direction). The format is
documented at the top of `crates/verba-local/src/translate.rs` (`direction<TAB>headword<TAB>translation; translation; ...`, most common first). Like the word lists, the
file is **not stored in this repository**: `scripts/fetch-dict.ps1` downloads the data and builds it (`cargo run --release -p verba-local --example build_dict` does the work).
`scripts/build-installer.ps1` and the release workflow run the script when the file is missing, and fail when it cannot be built.

| What | Source | License |
|---|---|---|
| Translations of English words into French and Arabic (the `translations` of the English entries); the English glosses of French and Arabic entries; the inflected forms of all of them | [Wiktionary](https://en.wiktionary.org) (English edition), as extracted by [wiktextract](https://github.com/tatuylonen/wiktextract) and published by [kaikki.org](https://kaikki.org): `https://kaikki.org/dictionary/{English,French,Arabic}/kaikki.org-dictionary-{English,French,Arabic}.jsonl.gz` (extracted on 2026-10-30 from the dump of 2026-10-01) | **CC BY-SA 4.0** and **GFDL** (dual license), see below |
| Which words come first | the frequency lists `data/words/{en,fr,ar}.txt` (above) | CC-BY-SA-4.0 |

- **License, checked on 2026-11-17.** English Wiktionary's own page [Wiktionary:Copyrights](https://en.wiktionary.org/wiki/Wiktionary:Copyrights) says: "The original texts of Wiktionary
  entries are dual-licensed to the public under both the Creative Commons Attribution-ShareAlike 4.0 International License (CC-BY-SA) and the GNU Free Documentation License (GFDL)."
  and, for people who reuse it: "If you want to use Wiktionary materials in your own books/articles/web sites or other publications, you may do so, but you have to follow CC-BY-SA / the
  GFDL. CC-BY-SA entails the following requirements: your materials have to be licensed under the same, similar, or compatible license, you must attribute the work in the manner specified
  by the author or licensor (but not in any way that suggests that they endorse you or your use of the work)". (That page carries a banner saying that it dates from 2002 and is outdated; its
  licensing statement agrees with the Wikimedia Foundation's [Terms of Use](https://foundation.wikimedia.org/wiki/Policy:Terms_of_Use), which license contributed text under "Creative Commons
  Attribution-ShareAlike 4.0 International License ("CC BY-SA 4.0"), and GNU Free Documentation License ("GFDL") (unversioned, with no invariant sections, front-cover texts, or back-cover
  texts)", say that "Reusers may comply with either license or both", and allow attribution "through hyperlink (where possible) or URL to the article to which you contributed", through a link to
  "an alternative, stable online copy" or through a list of authors.) kaikki.org says of its files: "This data is made available under the same licenses as Wiktionary - both CC-BY-SA and GFDL."
  and asks users of the data to cite Tatu Ylonen, "Wiktextract: Wiktionary as Machine-Readable Structured Data", Proceedings of the 13th Conference on Language Resources and Evaluation (LREC),
  pp. 1317-1325, Marseille, 20-25 June 2022.
- **Attribution (what Verba does).** `dict.txt` opens with a header naming Wiktionary and the license and saying that the entries were changed. `THIRD-PARTY.md` and the installer's
  `WORD-LISTS-LICENSE.txt` carry: "Word translations from Wiktionary (https://en.wiktionary.org), (c) Wiktionary contributors, CC BY-SA 4.0 and GFDL, extracted by wiktextract
  (https://kaikki.org, Tatu Ylonen); changed: reduced to short word translations, ranked, inflected forms added." Wiktionary keeps its authors in the page histories of the entries;
  the link to the project is how the Terms of Use allow the attribution to be given.
- **ShareAlike.** `dict.txt` is a derivative of Wiktionary text and stays under CC BY-SA 4.0 (you may choose the GFDL instead). Keeping it a separate downloaded file keeps this license
  from reaching Verba's code, in the same way as for the word lists. A copy of `dict.txt` that you pass on, or a file built from a modified copy, keeps the header and the same license.
  The file holds words and short phrases only; the extract keeps no definition text, example sentence or etymology.
- **What the build keeps** (details and numbers in [ACCURACY.md](ACCURACY.md#word-translation)):
  - **en>fr, en>ar:** the translations listed in the English entries, plus the English glosses of French and Arabic entries read backwards. **fr>en, ar>en:** the English glosses of French and Arabic entries (only short ones that
    read like a translation: one to three plain words, no definition, no "(...)" note, "to " dropped from verbs), plus the English translation tables read backwards. **fr>ar, ar>fr:** worked out from the other lists for the
    30 000 commonest words of each language (the rest goes through English in the engine).
  - Translations are put in order by the order of the senses in Wiktionary, how many senses and sources have them, and how frequent the translated word is in its own frequency list. At most 5 per headword.
  - **Inflected forms** (plurals, feminine, conjugations, Arabic definite and case forms) are headwords of their own, with the translations of their lemma. They come from the `forms` of the entries and from the entries that are "form of" another entry.
    Only forms of one-word lemmas are written; the verbal nouns and participles that Arabic verb tables list are not forms but words of their own, and are left out. A form that is also a word of its own (`saw`) gets its own translations and its lemma's, in the order of their scores.
  - **Arabic** is written without short vowels, headwords and translations alike. Dialect translations (Egyptian, Gulf, Levantine, Hijazi, ...) are left out; Maghrebi ones (Moroccan, Algerian, Tunisian) are kept at half weight, so they come after
    the Modern Standard Arabic ones. (English Wiktionary holds only 98 translations marked as Algerian Arabic, `arq`; they are not used as such.)
  - **Rude words:** a sense tagged vulgar, offensive, derogatory, slur, ethnic slur, pejorative or obscene is left out, and so is a translation tagged like that or belonging to such a sense of the English word. A headword that is left with
    no ordinary sense is dropped (an English word is dropped when at least two thirds of its senses are of that kind: `fuck`, `slut`); a word that is also an ordinary word (`dog`, `cock`) keeps its ordinary senses. Obsolete,
    archaic, historical, rare, misspelled and dialectal senses are left out; slang and informal ones count half.
  - **Size.** The file is cut to 14 MiB by default (`-MaxMB`): all headwords first (the commonest first), then the inflected forms that are words of the frequency lists, then the other forms of the commonest lemmas. Without the cut it has 944 000 lines and 37.7 MiB.
- **Looked at and not used:** the [FreeDict](https://freedict.org) dictionaries (checked on 2026-11-17 in the TEI header of each source file): `eng-fra` (8 799 headwords) and `fra-eng` (8 505) are small and `eng-ara` (87 424) and
  `ara-eng` (52 996) come from the Arabeyes word list; all four are under the **GNU General Public License, version 2 or later**, which would put the file (and anything combined with it) under the GPL: not used. Apertium's language data
  is also GPL-licensed (not examined further, for the same reason).
- **Known limits:** the data is what Wiktionary editors wrote. The translation tables of the commonest English words are often moved to a subpage that the extract does not contain (`house` has no `maison` in its table; it comes from the French entry's gloss),
  so the order of the first candidates is the weakest point for very common words with several meanings. Arabic entries are fewer (about 19 500 headwords with glosses), and Arabic forms with attached particles (`وبالكتاب`, `سيارتك`) are mostly missing: the engine would have to strip
  the article, prepositions and pronoun endings before the lookup.

## Personal dictionary files (written by the app, not downloaded)

`personal.txt` (one word per line, editable; a line starting with `-` is a word never to learn automatically) and `learned.txt`
(counts of single words that are in no dictionary, `w` lines; of dictionary words the user writes, common ones included, `u` lines, with a count that fades; and of pairs of neighbouring words, `b` lines; tab separated). The engine does not know where they live: the app passes the paths to
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
- Looked at and not used (not word lists): ELNER-DZ (named entities, CC BY 4.0).

### Algerian Darija in Arabic letters

The one "Darija" switch (`Lang::Dz`) covers both scripts.

- **Seed, in the program:** `crates/verba-local/seed/dz-ar-seed.txt`, original work of the Verba authors (PolyForm Noncommercial 1.0.0, like the code), embedded in the binary. Same
  `word weight` format as the Latin seed; no short vowels, no spaces. It holds Algerian vocabulary; what may be Moroccan, Tunisian or regional, or that the authors could not
  confirm, is in its `UNSURE` section for the Algerian reviewer. It also holds everyday words that Modern Standard Arabic shares, because Algerians write them in Darija texts.
- **Optional download:** `scripts/fetch-dz-ar.ps1` downloads the dataset "Algerian Dialect" (Zakaria Benmounah, Abdennour Boulesnane; Mendeley Data, V2, doi 10.17632/zzwg3nnhsz.2) into
  `data/dz/ar-youtube/` and writes `data/words/dz-ar.txt`: the words of the comments that the 50 000-word Arabic list lacks and that occur at least 4 times, with their counts
  (`cargo run --release -p verba-local --example build_dz_ar`). `LocalEngine::load_dz` merges it with the seed. License text, from the dataset's page on Mendeley Data: "Creative Commons
  Attribution 4.0 International (CC BY 4.0). You can share, copy and modify this dataset so long as you give appropriate credit, provide a link to the CC BY license, and indicate if changes
  were made, but you may not do so in a way that suggests the rights holder has endorsed you or your use of the dataset. Note that further permission may be required for any content within
  the dataset that is identified as belonging to a third party." The comments are written by YouTube users: the dataset licence does not settle their rights, so the list stays a
  separate download (it keeps `word count` lines only, no comment) and never enters the repository or a release file.
- **Attribution:** Benmounah, Z. and Boulesnane, A., "Algerian Dialect", Mendeley Data, V2, 2023, doi 10.17632/zzwg3nnhsz.2, CC BY 4.0, changed: words counted, the Modern Standard Arabic words removed.
- Also used for measuring only, never shipped: the same comments (see [ACCURACY.md](ACCURACY.md#algerian-darija-in-arabic-letters)).
- Looked at and not used: PADIC (parallel Algiers and Annaba dialect corpus; its SourceForge project says GPLv3, built from film and sitcom scripts: not an Arabic-script word list under a
  licence fit for this project).

## Known limits of these lists

- **Arabic:** subtitle text mixes Modern Standard Arabic with everyday speech and contains many forms with attached prefixes. Suggestions are usable, not ideal. A list built from news or Wikipedia text would suit Modern Standard Arabic better. Its license must be checked before it is used.
- **All three:** they contain names, interjections and some noise. A cleaning pass and a proper lexicon (for example Hunspell dictionaries, each with its own license) are planned.

## Not used

- NLLB-200 and other models licensed for non-commercial use only.
