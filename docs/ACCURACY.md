# Accuracy of the local engine

How well Verba's local engine finishes the word being typed, in English, French and Arabic (Modern Standard Arabic),
measured, not guessed. Measured on 2026-11-16 with the word lists described in [DATA.md](DATA.md).

## How it is measured

Two checks, both reproducible:

1. **The accuracy harness** (`crates/verba-local/examples/accuracy.rs`):

   ```powershell
   powershell -ExecutionPolicy Bypass -File scripts\fetch-data.ps1
   cargo run --release -p verba-local --example accuracy            # add `-- --misses 25` to list misses
   ```

   - **Targets:** the 3000 most frequent words of each list with 4 or more plain letters (one script, no stretched
     sounds, no pieces of contractions, and for English and French no word that is at least 15 times more frequent in the
     other language: `the` in the French list is not a French target). The filter lives in the harness and does not change
     when the engine changes, so before and after use the same words.
   - **Test:** each target is typed letter by letter into the engine, with English, French and Arabic loaded together
     (as in the real program), asking for 3 suggestions, like the popup. A hit means the target is among them.
   - **Reported:** top-1 and top-3 hit rate after 2, 3 and 4 letters; **letters saved per word** (the length of the word
     minus the number of letters typed when it first appears among the 3); and the same on **sloppy typing**:
     Arabic without hamza, taa marbuta dots or yaa dots; French without accents (only prefixes that really differ from
     the correct spelling).
   - **Speed:** the time of one `suggest` call over every prefix of every target (34 450 calls), release build.
2. **The sanity suite** (`crates/verba-local/tests/sanity.rs`): 113 hand-written cases (36 English, 39 French, 38 Arabic)
   of the form "text typed so far -> a word that must be among the 3": mid-sentence, capitals, accents, short vowels,
   hamza and dots left out, the Arabic article and particles, apostrophes and elisions. `cargo test -p verba-local`.

## Results

Same harness, same targets: the engine as it was (commit 520393c) and the engine now.

### Completion of correctly typed words

| Language | | top-1 @3 | top-3 @2 | top-3 @3 | top-3 @4 | letters saved / word |
|---|---|---|---|---|---|---|
| English | before | 25.9% | 8.7% | 53.7% | 85.6% | 2.53 |
| English | after | 25.7% | 8.7% | 53.4% | 85.4% | 2.52 |
| French | before | 22.4% | 9.9% | 47.8% | 78.2% | 2.72 |
| French | after | 22.3% | 10.0% | 47.7% | 78.3% | 2.72 |
| Arabic | before | 45.9% | 29.9% | 70.2% | 90.0% | 1.94 |
| Arabic | after | 43.5% | 29.3% | 69.0% | 89.1% | 1.92 |

**This number did not go up, and it is not going to by cleaning alone.** With correct spelling, the order is decided by how
often each word occurs, and a 3-slot popup is full of equally plausible words after 2 or 3 letters. The new engine gives
up a little here (0.1 to 1.2 points) because it also shows words that match when a letter is left out (see below), and
those take a slot now and then. The gain is elsewhere:

### Typing that is not exact

| Case | words | top-3 @3 before | top-3 @3 after | top-3 @4 before | top-3 @4 after |
|---|---|---|---|---|---|
| Arabic without hamza, taa marbuta dots or yaa dots | 539 | 0.0% | **54.0%** | 0.0% | **74.8%** |
| French without accents | 315 | 0.0% | **31.4%** | 0.0% | **66.0%** |

### The sanity suite

| Language | cases | passed before | passed now |
|---|---|---|---|
| English | 36 | 34 | **36** |
| French | 39 | 35 | **39** |
| Arabic | 38 | 30 | **38** |

Five more cases (`KNOWN_GAPS`, run with `-- --ignored`) are right but cannot pass with these lists; see below.

### Speed of one `suggest` call (release build)

| | p50 | p95 | p99 | max |
|---|---|---|---|---|
| before | 2 µs | 16 µs | 83 µs | 446 µs |
| after | 4 µs | 73 µs | 145 µs | 741 µs |

Slower than before (it looks at more candidates and at the words before the caret), still far under a millisecond.

### Lists after cleaning

| Language | lines | kept | dropped |
|---|---|---|---|
| en | 50 000 | 46 257 | 7.5% |
| fr | 50 000 | 45 262 | 9.5% |
| ar | 50 000 | 44 401 | 11.2% |

## What the engine does now

**Cleaning** (`clean.rs`): each rule removes noise seen in the real lists.
- single letters and strings over 24 letters; digits, symbols and hyphens (`1st`, `80s`, `peut-être` cannot be matched
  by the letters typed before the caret); one script and one known alphabet per word (drops a Greek omicron inside
  `you`, broken accents such as `ãªtre`, broken Arabic such as `ظˆط`, Persian letters, Latin stuck to Arabic);
  an apostrophe only inside a word (`aujourd'hui`, `don't`; drops `'s`, `l'`, `c'`, `qu'`); no letter three times in a
  row (`aaah`, `mmm`); a short list of noises (`uh`, `hmm`).
- English contractions arrive split (`don` + `'t`): `don`, `didn`, `isn`... become `don't`, `didn't`, `isn't` with their own
  counts; `can't` and `won't` get what is left of the `'t` count (an estimate, 7 to 3); the fragments `ll`, `ve`, `re`
  go. French fragments of elisions (`qu`, `jusqu`, `aujourd`) go.
- Words of the other language: a word is dropped from the English or French list when the other list uses it at least
  15 times more (`the`, `really` in French; `tuer` in English). Words both languages use stay in both.

**Matching** (`fold.rs`, `lib.rs`):
- Accents and capitals do not matter in the comparison; hamza on alef, taa marbuta dots, alef maqsura and the hamza seats
  do not matter for Arabic. The spelling suggested is the list's own. A letter typed WITH an accent must be right
  (`thé` never gives `the`); only a missing accent is forgiven. Exact spellings win unless the lenient word is about
  2.5 times more frequent.
- One suggestion per word: of two spellings where one is at least 10 times rarer (`الان` next to `الآن`), only the common
  one is shown. Two common spellings (`كتابة`, `كتابه`) are different words and both stay.
- A word you typed is not "corrected" by a completion unless the other spelling is at least 12 times more frequent
  (`tres` -> `très`, `المدرسه` -> `المدرسة`, but not `علي`, a name, -> `على`). Real corrections belong to the correction feature.
- Arabic with an attached article or particle (`ال`, `و`, `ف`, `ب`, `ل`, `ك` and combinations such as `وبال`, `لل`): when the
  list lacks the exact form (`بكتا`), the stem is completed and dressed again with what was typed (`بكتاب`).
  The lists store most such forms (8 720 of the 50 000 Arabic entries start with `ال`), so this is a fallback.
- French elisions are handled by the caret word: after `l'` or `qu'` the word that follows is completed.
- Language: with English and French both on, the 3 words before the caret vote (each shares its vote by how often it occurs in each language; `i` counts
  for English; `l'`, `d'`, `j'`, `qu'` right before the word count for French) and the language ahead is favoured up to 4 times.
  A single word typed alone is never biased.
- Languages can be switched off (`set_enabled`); the others fill the places.

## Known weaknesses

- **Ranking is one word at a time.** `ما اسم` -> `اسمك` is 4th because `اسمع`, `اسمي`, `اسمه` are each more frequent as
  single words; `الحمد لل` -> `لله` needs the previous word. A list of word pairs (bigrams) would fix this class of misses.
- **The Arabic list comes from subtitles.** It mixes Modern Standard Arabic with everyday speech and holds many spellings of
  the same word (`الأن`, `الان`, `الآن`). The 11.2% dropped are mostly broken text, but the rest is still subtitle language
  (no formal or technical vocabulary). Hamza spelling is inconsistent in the data, which is why leniency helps so much.
- **English and French share the Latin script.** Typed alone, a prefix such as `com` gets `comme` and `comment` next to
  `come`. The words before the caret help; the first word of a sentence cannot be helped.
- **Names and subtitle words** (`john`, `mr`) are in the lists.
- **Hyphenated words** (`peut-être`, `est-ce`) are dropped because the letters before the caret stop at the hyphen.
- **`won't` and `can't` counts are estimates**; other contractions (`it's`, `that's`, `i'm`) are not completed.
- **The accuracy harness uses the lists' own words as targets**, so it cannot say how well real writing is covered; a
  held-out text (news, messages) in each language would.

## About a better Arabic list

I looked for a Modern Standard Arabic list built from Wikipedia text. The Leipzig Corpora Collection has one
(`ara_wikipedia_2021_300K`, 470 000 distinct tokens, downloadable). I did **not** adopt it: its license could not be
verified (the site sits behind a bot check and the archive carries no license file), and a data pack is only
added here once its license is read from the source. It is the first thing to try next, after confirming the license
with the Leipzig team.

## Next steps

1. Confirm the Leipzig Wikipedia list's license (or find an MSA list whose license is clear) and compare its coverage of
   news and everyday text against the subtitle list; merge by share.
2. Word pairs (bigrams) per language for the next-word suggestions and for the misses above.
3. A held-out evaluation text per language, so the numbers describe real writing.
4. Hunspell dictionaries as a second source for the right spelling (each with its own license), to settle which
   spelling of a variant is the correct one instead of using frequency.
5. An Arabic stemmer for rare forms with suffix pronouns (`كتابهم`).

## Spelling correction

`LocalEngine::correct(only, before_caret, max) -> Option<Fix>` fixes the word the user has **just finished** (a space or
punctuation follows it). It is quiet by design: a false alarm costs more than a miss, so it only speaks when a word of the
language is clearly what was meant. The code is `crates/verba-local/src/spell.rs`; the numbers below come from
`cargo run --release -p verba-local --example spell_accuracy` (fixed seed, so they repeat exactly), the cases from
`crates/verba-local/tests/spell.rs`.

### Method

1. **The word.** The text must end with a run of letters followed by 1 to 3 spaces or punctuation marks (`. , ; : ! ? ) ] } " » … ، ؛ ؟`).
   Not a word to fix: fewer than 3 letters, digits or `- _ @ / . # & + = < $ %` just before it, an apostrophe before it unless it is a
   French elision (`l'ete`), mixed scripts, letters outside English / French / Arabic, ALL CAPS, camelCase, a capital letter that is
   not at the start of a sentence (a name), a word already written earlier in the visible text, a word the app told the engine to
   ignore (`ignore_word`).
2. **Is it a word?** Latin: the exact lower-case word is in a list. Arabic: its comparison key is in the list, or it is a *common*
   listed stem (rank within 12000) with a particle (`و ف ب ل ك ال ...`) and/or a pronoun or plural ending, because the lists lack
   inflected forms. Any list of the script that is on counts, so French typed on an English keyboard is not a typo.
3. **Candidates.** Everything one edit away (a letter too many or missing, replaced, two letters swapped) is looked up in a hash
   table of comparison keys (so an accent slip costs nothing: `tres` finds `très`, and a missing apostrophe is treated the same way:
   `dont` finds `don't`). Only if nothing is one edit away, and the word has 7 letters or more, two edits are tried, among words
   with the same first letter and rank within 4000.
4. **Ranking.** Frequency share of the candidate times a factor: accent 3.0 or missing apostrophe 2.0, swapped letters 1.5, neighbouring
   key (QWERTY, AZERTY or the Arabic layout) 1.4, doubled letter 1.1 to 1.2, other 1.0, two edits 0.02; times 1.25 when the first letter is
   kept (0.7 when not). With English and French both asked for, the words before the caret lean the choice.
5. **Gates**, from the measurements below:
   - a candidate must rank high enough in its list, by kind of edit and by length of the typed word;
   - two words that differ only by an ending (`habitat` / `habitats`, `catered` / `caterer`, Arabic `وثلاثة` / `ثلاثة`) are different
     words, not slips;
   - a word that **is** in a list is still corrected when a word one edit away is far more frequent (next section).

The rank limits (0 = never; length of the typed word 3, 4, 5, 6 or more letters):

| Kind of edit | Latin | Arabic |
|---|---|---|
| swapped letters | 1500, 8000, 30000, 50000 | 300, 2000, 8000, 30000 |
| neighbouring key | 0, 1000, 4000, 12000 | 0, 600, 2500, 8000 |
| doubled letter | 0, 2000, 8000, 30000 | 0, 1000, 4000, 15000 |
| other letter | 0, 1500, 4000, 10000 | 0, 1000, 3000, 8000 |
| accent or apostrophe | 20000 | (not used) |
| two edits | 4000 | 4000 |

So a 3-letter word is only fixed when two letters were swapped (`teh`, `adn`, `hte`); the other slips need a longer word to be trusted.

### The lists carry slips

Subtitle authors misspell, and the lists count it. Before the ratio rule below, the engine took these for words and never fixed them:

| In the list | count | the right word | count |
|---|---|---|---|
| `wich` | 265 | `which` | 422 483 |
| `seperate` | 166 | `separate` | 18 291 |
| `occured` | 281 | `occurred` | 9 568 |
| `tres` (French list) | 5 418 | `très` | 462 324 |
| `meme`, `pere` (French list) | 5 468, 2 782 | `même`, `père` | 411 247, 257 456 |

So a listed word is corrected when the candidate is at least *ratio* times more frequent. The ratio depends on the kind of edit and
the length of the typed word, because short words have many valid neighbours (`hood` / `good`, `scan` / `can`) and long ones almost
none. Swapped letters: 200, 60, 60, 40 (for 3, 4, 5, 6+ letters). Neighbouring key: never, 2000, 600, 200. Doubled letter: never, 1500,
150, 30. Other edit: never, 1500, 800, 100. Accent or apostrophe: 40. Two edits: 300. All times 0.3 for Arabic. A word that is only
another form of a much more frequent one (`believer`, `doings`) stays alone unless the other is 2000 times more frequent
(`realy` / `really`). The frequency of a word in a language that is not the one asked for weighs a tenth (`dont` is a French word; on an
English keyboard it is a slip).

### Recovery of typos

One typo of each kind in frequent words (ranks 200 to 3000, 5+ letters); a typo that is itself a word of a list is skipped.
"One language" is how the app asks when the keyboard says which language is typed.

| Language | Typo | typos | top-1 | top-3 | nothing offered |
|---|---|---|---|---|---|
| English | swapped letters | 1979 | 94.8% | 99.9% | 0.0% |
| English | letter missing | 1746 | 66.6% | 79.2% | 9.0% |
| English | extra letter | 2055 | 96.2% | 97.0% | 2.6% |
| English | neighbouring key | 2028 | 92.3% | 98.7% | 0.7% |
| English | other letter | 1960 | 91.9% | 98.9% | 0.7% |
| English | two slips | 1968 | 37.7% | 40.1% | 48.5% |
| **English** | **all** | 11736 | **80.4%** | **85.9%** | 10.2% |
| French | swapped letters | 2204 | 88.5% | 95.9% | 0.4% |
| French | letter missing | 1998 | 60.7% | 75.5% | 11.7% |
| French | extra letter | 2309 | 90.7% | 92.2% | 3.6% |
| French | neighbouring key | 2181 | 86.9% | 92.7% | 2.7% |
| French | other letter | 2210 | 86.6% | 93.3% | 2.0% |
| French | accent left out | 117 | 94.0% | 100.0% | 0.0% |
| French | two slips | 2217 | 39.7% | 44.0% | 42.4% |
| **French** | **all** | 13236 | **76.0%** | **82.5%** | 10.3% |
| Arabic | swapped letters | 1315 | 80.5% | 91.3% | 8.2% |
| Arabic | extra letter | 1389 | 86.9% | 87.9% | 11.4% |
| Arabic | neighbouring key | 1215 | 73.6% | 80.9% | 16.0% |
| Arabic | other letter | 1311 | 78.0% | 90.2% | 8.6% |
| Arabic | letter missing | 981 | 21.5% | 34.4% | 44.4% |
| Arabic | two slips | 1303 | 24.2% | 26.6% | 59.0% |
| **Arabic** | **all** | 7514 | **62.6%** | **70.1%** | 23.7% |

With the three languages on and no keyboard to say which (the harder case): top-1 / top-3 English 77.6% / 85.9%, French 74.2% / 82.8%,
Arabic 62.6% / 70.1%. Typos in **rarer** words (ranks 3000 to 12000, one language): top-1 / top-3 English 60.1% / 64.5%, French 57.5% /
63.1%, Arabic 31.0% / 34.9%: the correction is deliberately cautious where the dictionary thins out.

### False alarms

Valid words the dictionary lacks (an engine built from the top 30 000 words, asked about the words ranked 30 000 to 50 000 of the full
list: names, rare words, inflections) and words that are in the dictionary. A good engine says little; "flagged" means a fix was offered.

| Test | tested | flagged |
|---|---|---|
| English, words missing from the dictionary | 16627 | **22.2%** |
| French, words missing from the dictionary | 16553 | **21.4%** |
| Arabic, words missing from the dictionary | 14355 | **13.8%** |
| English, `-s -ed -ing` added to listed words (some are not words) | 11313 | 25.5% |
| French, `-s -e -es` added to listed words (some are not words) | 11171 | 24.3% |
| Arabic, particles and endings added to listed words | 67523 | 0.0% |
| English, words that are in the dictionary (ranks 2000 to 30000) | 27767 | 1.6% |
| French, words that are in the dictionary | 27829 | 1.6% |
| Arabic, words that are in the dictionary | 27822 | 2.1% |

How to read the first rows: they are lower-case, so every name counts, and a name is capitalised in real text (and left alone). Reading
the flagged words by hand, most are rare valid words with a frequent neighbour (`volley` / `valley`, `juke` / `joke`), names, and
Arabic broken plurals (`تواريخ` / `تاريخ`). If, say, 2% of the words a person types are outside a 50 000-word list (an assumption, not a
measurement), roughly one word in 200 to 300 gets a fix it should not, against the typos fixed. Among the listed words that get a fix, a
hand reading shows mostly real slips in French (`fenetre`, `planete`, `rapelle`, `epoque`, `ecrire`) and mostly rare valid words in
English (`mare`, `hera`, `premise`).

### The dials (a sweep, a fifth of the cases)

Each row changes one setting from the defaults. "Rarer" = recovery of typos in words ranked 3000 to 12000; "OOV" = words missing from
the dictionary; "listed" = words in it.

| Setting | EN top-1 | FR top-1 | AR top-1 | EN rarer | FR rarer | AR rarer | EN false (OOV) | FR false (OOV) | AR false (OOV) | EN false (listed) | FR false (listed) | AR false (listed) |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| default | 79.4% | 76.9% | 62.9% | 60.3% | 57.7% | 31.5% | 22.9% | 21.7% | 13.3% | 1.8% | 2.0% | 2.6% |
| rank limits x0.5 | 73.5% | 72.3% | 55.5% | 29.7% | 28.9% | 15.0% | 14.4% | 13.2% | 7.9% | 1.8% | 2.0% | 2.4% |
| rank limits x2 | 79.9% | 77.3% | 65.2% | 73.7% | 69.1% | 50.8% | 34.2% | 33.1% | 21.4% | 1.8% | 2.0% | 2.6% |
| known-word ratios x0.5 | 79.4% | 76.9% | 62.9% | 60.3% | 57.7% | 31.5% | 22.9% | 21.7% | 13.7% | 3.2% | 3.1% | 4.8% |
| known-word ratios x2 | 79.4% | 76.9% | 62.9% | 60.3% | 57.7% | 31.5% | 22.9% | 21.7% | 13.2% | 1.0% | 1.1% | 0.9% |
| known words never corrected | 79.4% | 76.9% | 62.9% | 60.3% | 57.7% | 31.5% | 22.9% | 21.7% | 13.2% | 0.0% | 0.0% | 0.0% |
| first-letter substitutions x0.25 | 77.1% | 75.0% | 60.8% | 55.1% | 52.1% | 29.8% | 20.3% | 18.7% | 12.6% | 1.8% | 2.0% | 2.6% |
| no distance 2 | 74.9% | 71.2% | 61.3% | 59.8% | 57.1% | 31.2% | 19.3% | 18.1% | 11.3% | 1.6% | 1.9% | 2.5% |
| no "other ending" rule | 80.2% | 79.0% | 66.5% | 60.9% | 58.2% | 32.9% | 27.5% | 33.3% | 17.3% | 1.9% | 2.5% | 4.4% |
| no Arabic stem + ending rule | 79.4% | 76.9% | 65.5% | 60.3% | 57.7% | 32.4% | 22.9% | 21.7% | 18.5% | 1.8% | 2.0% | 2.6% |

Why these defaults: rank limits x1 sit in the middle (x0.5 cuts the false alarms by about 40% but halves the recovery of rarer words, x2 buys
11 to 19 points of rarer-word recovery for 8 to 11 more points of false alarms). Two edits cost 2 to 3.6 points of false alarms and are
kept because they bring two-slip words from about 10% to about 40% recovered (top-3, one language: English 10.1% to 40.1%, French 11.3% to
44.0%, Arabic 11.0% to 26.6%; false alarms 18.9% to 22.2%, 17.7% to 21.4%, 12.0% to 13.8%). The "other ending" rule removes 4 to 12 points
of false alarms for 1 to 4 points of recovery. Changing the first letter is rare in real typos and costs more false alarms than it
recovers. The known-word ratios are the price of the slips in the lists: x2 halves the listed words wrongly touched, but would miss
`seperate` and `occured`.

### Speed

| p50 | p95 | p99 | max |
|---|---|---|---|
| 173 µs | 522 µs | 807 µs | 1811 µs |

11253 calls, release build, on this PC. `correct` is asked after every change of the text, but it only reads the end of the string
until a word is finished, so the cost above is paid once per word. The first call builds the lookup tables of a language once
(English and French 61 ms together, Arabic 37 ms).

### Cases in `tests/spell.rs`

30 English, 29 French and 26 Arabic hand-written typos (all fixed with the language asked for; the English ones also with no keyboard to
say which, except `dont`, a French word too), and checks that a correct word, a name, an acronym, `camelCase`, an address, a number, a word
still being typed, a word already written, a dismissed word, a word of the other language, a disabled language and the ending-only forms
(`habitats`, `admires`, `fraternités`) are left alone; that `only` decides where the fixes come from; and that the fix keeps the
punctuation typed after the word and the capital letter. Seven right fixes cannot be given and are kept visible in `KNOWN_GAPS`
(`-- --ignored`):
- `Je l'ai trouve` -> `trouvé` and `I want to go their` -> `there`: both are words of the language, and only the sentence says they are wrong;
- Arabic `كنتت`, `الوقتت`, `ذلكك`: an extra `ت ك ه ي ن` at the end of a word looks like a pronoun or plural ending, and such forms are words;
- Arabic `قللت` -> `قلت`: the list holds the slip 308 times less often than the right word, not enough for a 4-letter word;
- Arabic `السيراة` -> `السيارة`: `سيرا` + the ending `ة` looks like a word to the endings rule.

### Known weaknesses

- **Names and rare words** are the main false alarms (a fifth of the words missing from a 50 000-word list): there is no knowledge of
  the sentence. The app can call `ignore_word` for what the user dismisses.
- **Real-word errors** (`their` / `there`, `form` / `from`, `trouve` / `trouvé`) need the neighbouring words; this is a spelling
  check, not a grammar check.
- **Arabic morphology** is approximated with particles and endings; broken plurals (`تواريخ` / `تاريخ`), verb forms with a prefix and
  pronoun at once, and a final extra `ت ك ه ي ن` are not understood. Recovery of Arabic typos in rarer words is low (31% top-1) for
  that reason; a stemmer or an Arabic dictionary with its license is the next step.
- **Two letters wrong** are fixed for 7+ letter words only, and 40% of the time at best.
- **The typo sets are synthetic** (random positions, one kind at a time) and drawn from the lists' own frequent words, so they say how the
  rules behave, not how often real people slip; a collection of real misspellings per language would.

### Next steps

1. A held-out collection of real misspellings per language to replace the synthetic typos.
2. Word pairs (bigrams) to tell `their` from `there` and to rank candidates by the sentence.
3. Hunspell dictionaries (each with its license) as a second opinion on what is a word, so that a rare valid word stops being flagged.
4. An Arabic stemmer or dictionary, then retune the Arabic ratios.
