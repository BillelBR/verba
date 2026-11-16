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

## Next-word prediction

`LocalEngine::next_words(only, before_caret, max)` is used when the caret is right after a finished word and a space
(`I would like |` -> `to`, `a`, `the`). The code is `crates/verba-local/src/nextword.rs`; the tables are built by
`scripts/fetch-bigrams.ps1` from Tatoeba sentences ([DATA.md](DATA.md)); the numbers come from
`cargo run --release -p verba-local --example nextword_accuracy -- --lang en|fr|ar [--sweep]` (deterministic: the split is by sentence id),
the cases from `crates/verba-local/tests/nextword.rs`.

### How it decides

1. The text must end with a plain word and one or more spaces. A comma, a full stop, a line break, a number, an address or a hyphenated
   word before the caret gives nothing (the text is cut into runs of plain words; no pair is made across a cut).
2. The user's own pairs (learned, seen at least twice) come first, most often first.
3. Then the bundled table of the language: the 6 likeliest followers of the last word. For the 20 000 commonest pairs of words the table also
   holds the followers of the pair (`would like` -> `to`), blended with the one-word table with the weight `n / (n + 20)`, `n` being how
   often the pair was seen.
4. A word is offered only when it follows with a probability of at least 2%. When the last word is not in the table, nothing is offered:
   **no frequent word is ever used as a filler.**
5. English and French both on, no keyboard language: each table is weighted by how well the words before the caret fit the language
   (the same vote as for completions). `i` is written `I`.

### Method

The corpus is split by sentence id: 1 sentence in 40 is held out (50 841 English, 18 242 French, 1 773 Arabic). A table is built from the
rest with the code that builds the shipped tables, then every position of every held-out sentence is a question: the word before is the
context (and the one before it), the next word is the answer. **Overall** counts a position where nothing is offered as a miss, which is what
the user sees; **when shown** counts only the positions where something is offered. The baseline is the strongest dumb rule:
**always offer the 3 most frequent words** of the training part (`the to I` / `de je est` / `في من أن`), not counting corpus-only words such as `Tom`.

Three test sets, because one alone is not honest:

- **novel**: held-out sentences, without exact copies of a training sentence and without sentences that share 60% or more of their word
  triples with the training part. Tatoeba holds many near-copies (`Tom ate an apple` / `Mary ate an apple`), so this drops 72% of the English
  held-out sentences. What stays is the unusual part of the corpus: **pessimistic**.
- **loose**: all held-out sentences except exact copies. **Optimistic** (near-copies of training sentences are in it).
- **hand**: 30 everyday messages per language that I wrote for this measurement (thanks, meetings, questions: 313 / 257 / 217 positions),
  not taken from the corpus. It is the closest to what people type, but it is small and one author's idea of an everyday message, written
  knowing the corpus style, so treat it as a sanity check, not as a number to quote.

### Results (shipped settings)

| | set | positions | baseline top-3 | shown | top-1 overall | top-3 overall | top-1 when shown | top-3 when shown |
|---|---|---|---|---|---|---|---|---|
| English | novel | 89 115 | 6.7% | 84.2% | 10.8% | **18.8%** | 12.8% | 22.4% |
| English | loose | 329 265 | 8.6% | 86.9% | 19.1% | **31.3%** | 22.0% | 36.1% |
| English | hand | 313 | 13.4% | 97.8% | 31.9% | **54.0%** | 32.7% | 55.2% |
| French | novel | 41 171 | 5.8% | 91.5% | 11.6% | **21.3%** | 12.6% | 23.3% |
| French | loose | 99 543 | 6.7% | 94.4% | 18.9% | **32.3%** | 20.1% | 34.2% |
| French | hand | 257 | 5.1% | 99.2% | 28.8% | **51.0%** | 29.0% | 51.4% |
| Arabic | novel | 5 383 | 7.0% | 79.6% | 9.1% | **14.7%** | 11.5% | 18.5% |
| Arabic | loose | 5 914 | 7.5% | 80.0% | 10.0% | **15.9%** | 12.5% | 19.9% |
| Arabic | hand | 217 | 12.4% | 94.9% | 18.0% | **29.0%** | 18.9% | 30.6% |

(Baseline top-1 is 3.0% to 5.4% everywhere.)

**Verdict.** All three languages beat the baseline clearly: top-3 overall is 2.1 to 10 times the baseline's and top-1 is 2.6 to 7.4 times,
on every set, so all three tables are used. **Arabic is the weakest**: 14.7% top-3 on unusual sentences and 29% on everyday ones, from a corpus 50 times
smaller than the English one (0.3 million words; attached particles such as `و` and `ال` split the counts further). It is clearly better than the
baseline, but the popup will often be wrong; a bigger Arabic corpus is the first thing to improve (DATA.md says what was ruled out). English and French on
unusual sentences (19-21% top-3) are about what a one- or two-word context can give: next-word prediction is hard, and these are hits within three
guesses, not words written for you. The hit rate to expect on real messages lies between the novel and the loose rows, and nearer the loose one for common phrases.

### What the dials do (English; French and Arabic move the same way)

| Setting | novel top-3 | loose top-3 | hand top-3 | size |
|---|---|---|---|---|
| one-word context, 5 000 contexts, 8 followers, no floor | 15.9% | 25.2% | 39.9% | 336 KB |
| same, floor 2% | 15.7% | 24.9% | 39.9% | 336 KB |
| same, floor 5% | 14.1% | 22.6% | 34.8% | 336 KB |
| same, floor 10% | 10.0% | 16.6% | 27.2% | 336 KB |
| 10 000 contexts (floor 2%) | 17.1% | 25.6% | 39.9% | 594 KB |
| 2 000 contexts | 13.6% | 23.4% | 39.3% | 142 KB |
| 5 followers instead of 8 | 15.7% | 24.9% | 39.9% | 233 KB |
| no corpus filter (the names `Tom`, `Mary` kept) | 16.8% | 28.5% | 38.3% | 341 KB |
| + 5 000 pair contexts | 16.9% | 29.0% | 51.8% | 589 KB |
| **+ 20 000 pair contexts, 10 000 contexts, 6 followers (shipped)** | **18.8%** | **31.3%** | **54.0%** | 1.4 MB |
| + 50 000 pair contexts | 18.1% | 31.6% | 55.0% | 2.1 MB |

- **The previous two words help a lot** (about +3 points on unusual sentences, +14 on everyday ones), so the pair contexts are kept; beyond 20 000
  pairs the gain is under 1 point for 50% more size. The weight constant (5, 20 or 60) makes no measurable difference.
- **The 2% floor costs nothing** (it only removes answers that almost never hit); a floor of 5% or more loses hits. Only 3 answers are shown, so 8
  followers per word is no better than 5; 6 are kept so that `max` up to 5 has something to fall back on.
- **The corpus filter** (no `Tom`, `Mary`, nothing the subtitle list does not know) costs 1 point on the novel set and 3.6 on the loose one, which is full of
  them, and gains 1.6 on the everyday set, which is why it is on.
- For French the picture is the same (pairs: +2.6 novel, +16.8 hand). For Arabic the pair contexts add under half a point: the table is mostly one-word
  context. Using the subtitle list as the vocabulary, as for English and French, gives 11.3% on novel Arabic against 14.7% with an open vocabulary and
  lower thresholds (the subtitle list lacks the forms with attached particles), so Arabic is built the second way.

### Cost

| | English | French | Arabic |
|---|---|---|---|
| table on disk | 1.4 MB | 1.1 MB | 0.5 MB |
| contexts | 29 990 | 29 395 | 10 446 |
| load (parse the text) | 27 ms | 23 ms | 7.5 ms |
| memory when loaded | 6.6 MB | 5.6 MB | 1.8 MB |
| `next_words` p50 / p95 / p99 / max | 3.8 / 7.6 / 10.2 / 35 µs | 3.6 / 7.0 / 9.2 / 33 µs | 4.2 / 8.7 / 13 / 30 µs |

Release build on this machine, over every position of the held-out sentences (89 115 / 41 171 / 5 383 calls). The memory is counted by a counting
allocator around the parse; the in-memory form is not compact (a boxed string per word) and could be about 3 times smaller if it matters.

### Known weaknesses

- Everyday phrases that are not translation exercises are often missing: `Happy |` gives `to with here`, not `birthday`; `Good |` gives `at idea time`; `See you |`
  gives `again in at`. `tests/nextword.rs::known_gaps` lists them (`-- --ignored`). A corpus of e-mails and chat would help most.
- Only the last two words are used: `thank you very |` works, `I would like to ask you |` does not.
- After a sentence end or a comma nothing is offered: a table of sentence openers would be a separate feature.
- Followers are lower case (`I` and `I'm` excepted), which suits the middle of a sentence.
- Tatoeba's English mixes American and British, its French is mostly European, its Arabic is a macro-language (`ara`) that includes some dialect.

## Personal dictionary

`PersonalDict` (code: `crates/verba-local/src/personal.rs`; tests: `crates/verba-local/tests/personal.rs`, 25 cases, which need no downloaded data except
two checks on the real lists).

### Words the user keeps

`add_word` takes a single word of 2 to 40 letters of one script (an apostrophe or hyphen inside is fine) and keeps its capitals (`Snow`, `iPhone`).
Kept words are never flagged by the correction, and are offered as completions, per script, whatever the language of the keyboard.

**Ranking.** A kept word counts as `3.0e-4` of the list's total frequency (a word learned by the engine: `1.5e-4`, up to 1.6 times more when
seen often). On the real English list that is about the 330th most frequent word (`police`, 3.05e-4). Measured on the real lists with words added by hand:

| typed | offered |
|---|---|
| `po` | police, **Positiv**, point |
| `pos` | **Positiv**, possible, position |
| `posi` | **Positiv**, position, positive |
| `sn` | **Snow**, snow, snake |
| `ve` | very, **Verba**, vehicle |
| `be` | been, because, before (`Benchmark` was added: it needs `ben`) |

A very common word (`people`, `very`, `been`, `because`: ranks under 150) beats the user's word on two letters; with three letters or more the user's word is
among the first. A word the user kept beats every dictionary word rarer than the 330th, which is how a name or a term of the trade gets through; if that
feels too eager, the constants `ADDED_SCORE` and `LEARNED_SCORE` in `personal.rs` are the dials. The capitals of the typing are respected (`POS` -> `POSITIV`).
The ranking is tested (`a_very_common_word_still_wins_on_a_short_prefix_but_a_rare_one_does_not`).

### Learning (off unless the app calls `learn_text`)

- **Counted:** words of 3 to 24 letters of one script that are in no dictionary that is on (exact word for Latin; for Arabic also with a
  particle attached), once per occurrence. Promoted to personal at 3 sightings.
- **Refused:** numbers and anything with a digit, URLs, e-mail addresses, paths and slashes, identifiers (`snake_case`, `getUserName`: two or more
  lower-to-upper switches), hex strings of 6 letters or more, acronyms and shouting (`NASA`, `OK`), hyphenated words, stretched sounds (`aaah`),
  words under 3 or over 24 letters, words the user removed (they stay removed; adding one by hand lifts that), and **words the correction takes for a
  typo** (checked when a word is about to be promoted: `recieve` typed three times is not learned; the user can still keep it on purpose).
- **Capitals:** the form kept is the one typed in the middle of a sentence (`Quillon`), not the one at its start.
- **Caps:** 5 000 words (candidates and promoted; the least seen, then the oldest, are forgotten first, and a promoted word goes last) and 20 000 pairs.
  Only the last 20 000 characters of a text are read.
- **Pairs** (for next-word prediction): two neighbouring plain words, not across punctuation, a line break, a number or an address; used once seen twice.
- **Cost:** `learn_text` on 200 words: 340 µs. `suggest` with 1 900 learned words that all match the typed prefix: 410 µs (the worst case; a normal prefix
  matches a few words: 1-6 µs). `save` with 4 500 words and 14 000 pairs: 7 ms, 330 KB.

### Privacy

Stored: single words (lower case, or as typed in mid-sentence), how often each was seen, a counter of when (a number that goes up by one per
`learn_text`, **not a date or a time**), and pairs of two neighbouring words with their counts. **Never stored:** a sentence, three words in a row,
the app, the place, the date. Whoever reads `learned.txt` sees the words and pairs the user uses most (`thank you`): that is the data, and
that is why it is a plain, editable file that `clear_learned` and deleting the file both wipe. The test `what_is_stored_never_holds_a_sentence` checks the
shape of every line written. The app must never call `learn_text` for a password field, an excluded app, or text the user did not type: the engine cannot tell.

### Files

`personal.txt` and `learned.txt` are plain UTF-8 text; a byte order mark, Windows line endings, comments, blank lines and invalid lines are accepted
and skipped. Writing goes to `<name>.tmp` then renames over the file, creates the folder if needed, and writes nothing when nothing changed
(`is_dirty`). A missing, damaged or non-UTF-8 file gives a smaller dictionary, never an error. Tested with garbage bytes, half-written lines, absurd
numbers, a folder where a file should be, and a rename that fails.
