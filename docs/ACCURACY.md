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

## Algerian Darija in Latin letters ("Arabizi")

Measured on 2026-11-16 with `cargo run --release -p verba-local --example dz_accuracy` (add `-- --misses 30` to list misses). Tests:
`crates/verba-local/tests/dz.rs`. How the feature is wired: `crates/verba-local/src/dz.rs`.

### Read this first: the seed list is written by the authors

Algerian Arabizi has no standard spelling and there is no large free frequency list of it. The program ships a **seed list of 2476 words**
(`crates/verba-local/seed/dz-seed.txt`, PolyForm Noncommercial 1.0.0), written from the authors' knowledge, with weights that are estimates. So:

- Every number measured on the seed's own words (sections 1 and 1b) is **optimistic**: the list knows the words it is asked about.
- The honest number is section 3, on real text that nobody wrote the list from (the NArabizi treebank). There the seed knows **55%** of the
  words, and the right word is in the popup for **17%** of the words after 3 characters (44% when the word is one the list knows).
- No Algerian reviewer has checked the list yet. The words the authors are unsure of are in the `UNSURE` section at the end of the file.

### What was checked, and why no corpus is in the program

NArabizi (Seddah et al., "Building a user-generated content North-African Arabizi treebank", ACL 2020; Universal Dependencies release
`UD_Maghrebi_Arabic_French-Arabizi`) is a treebank of 1 287 sentences (18 561 tokens) of Algerian newspaper forum comments and song lyrics,
written in Latin letters, each token labelled Algerian or French. Its license, from its `LICENSE.txt` and the UD metadata:
*"The treebank is licensed under the Creative Commons License Attribution-ShareAlike 4.0 International."* (CC BY-SA 4.0). That allows use,
so it is used **as a separate download only** (`scripts/fetch-dz.ps1` builds `data/words/dz.txt`; nothing is copied into the repository), like the
other lists, and to **measure** the seed. It is too small to be the only source (about 3 900 distinct Algerian words in 1 003 sentences) and it
is forum and lyric text with much Modern Standard Arabic and politics. Other corpora seen and not used: ELNER-DZ (CC BY 4.0, named entities) and a
45 000-comment YouTube sentiment set on Mendeley Data (CC BY 4.0, mostly Arabic script). Neither is a word list.

### How Dz is merged with English and French

Decision, in `dz.rs`. Dz is off until `set_enabled(Lang::Dz, true)`. When it is on, in a Latin context (English or French keyboard, or no hint):

1. A word with digit-letters (`sa7`, `3la`, `9ah`) can only be Darija: Dz alone answers.
2. Otherwise Dz is an extra pool next to English and French. Its candidates are multiplied by `DZ_PRIOR = 0.01` (the seed is small, so its
   words would otherwise outrank real English words), and **a Dz candidate is shown only if it beats the best English or French one**, so it
   never fills the free places of the popup in an English sentence.
3. When the words just before are clearly Darija (words of the Dz list that English and French do not know, and no English or French word nearer),
   Dz counts in full (x1.5) and English and French are held back (x0.3).
4. The Arabic keyboard does not get Dz. `suggest_in(Lang::Dz, ...)` gives Darija only.

Matching has three tiers: exact prefix (`bez` -> `bezzaf`), the same sounds spelled another way (`kifech` for `kifach`, `7bibi` / `hbibi`,
`9ahwa` / `qahwa`, `5ouya` / `khouya`: see `soft_key`), and the consonants only (`bez` -> `bzaf`). Several spellings of one word are one suggestion:
the one that looks like what is being typed.

The spelling correction never writes a Darija word (Dz is not one of its lists) and leaves alone a word Dz knows: listed, another spelling of a
listed word (but not of a plain English or French word: `masster` is still a typo), or a listed word with a negation or pronoun attached
(`makhdemtch`, `choufek`). Words with digit-letters are never corrected anyway.

### 1. The seed's own words (optimistic)

2296 seed words of 4+ characters, typed letter by letter into the engine with English, French and Dz on, 3 suggestions. top-1 / top-3:

| context | 2 characters | 3 characters | 4 characters |
|---|---|---|---|
| nothing before | 5.6% / 12.9% | 23.0% / 39.9% | 49.6% / 70.7% |
| `wach rani` before (Darija) | 9.1% / 23.3% | 33.7% / 62.0% | 56.8% / 81.4% |
| `i think` before (English) | 5.1% / 11.9% | 21.0% / 36.1% | 47.2% / 67.3% |

Low at 2 and 3 characters because many words share a prefix in a list of 2 500 words, and English and French hold some places. A hit is this exact
spelling; counting another spelling of the same word as right, top-3 @3 is 46.6% (nothing before) and 70.3% (Darija before).

**Spelling held out (1b):** 400 spellings that have a sibling spelling were removed from the list one at a time and typed to 3 characters (Dz alone):
a sibling is in the top 3 for **61.5%**. This is the part of the matching that does not depend on the list holding the exact word.

### 2. False alarms: normal English and French typed with Dz on

The 3000 most frequent plain words (4+ letters) of each list, typed letter by letter (2, 3 and 4 characters, 3 suggestions). A false alarm is any
Darija word in the popup (it is then always the first suggestion: a Dz word must beat the best English or French one). English or French words before make no difference:
a context only ever helps Dz when it is Darija.

| language | 2 characters | 3 characters | 4 characters |
|---|---|---|---|
| English | 0.13% | **0.90%** | 0.25% |
| French | 0.40% | **0.80%** | 0.15% |

Typical cases: `wel` -> `wellah`, `bee` -> `bezzaf`, `oua` -> `ouach`, `sal` -> `salam`, where the English or French candidates are rare. These are the
commonest words, which is where people type; for rare words the prefix is less often the start of a common word, so more Darija shows up.
Dz loaded but switched off: output identical to an engine without Dz on 2000 prefixes and typos (the tool, and the test `with_darija_off_nothing_changes`).
Guard in the tests: under 1.5% of the prefixes of ordinary words may show a Darija word.

**Spelling correction with Dz on, against no Dz** (5+ letter words of the same lists, synthetic typos, first fix right, `correct(Some(lang))`):

| language | two letters swapped | a letter missing | a letter doubled | correct words wrongly fixed |
|---|---|---|---|---|
| English with / without Dz | 97.2% / 97.3% | 71.7% / 72.2% | 93.9% / 94.5% | 0.00% / 0.00% |
| French with / without Dz | 93.8% / 93.8% | 68.7% / 69.1% | 89.9% / 90.6% | 0.08% / 0.08% |

So Dz on costs 0 to 0.7 points of typo recovery (a typo that sounds like a Darija word is no longer fixed). The English and French cases of
`tests/spell.rs` still pass, and `english_and_french_typos_are_still_fixed_with_darija_on` repeats 25 of them with Dz on.

### 3. Real text: NArabizi (CC BY-SA 4.0, measured on, not copied)

Tokens labelled Algerian (`LangO=ar_dz`), names left out, each typed as a prefix after the words before it in its sentence.
**dev** = the two small files (284 sentences): they were read while the seed was being finished, and the plainly missing frequent words
(`el`, `rana`, `ra7`, `kount`...) were added after looking, so dev is not independent. **sealed** = the large file (1003 sentences), not read while writing.

| set | Darija tokens | the list knows the token | right word in the top 3 after 2 / 3 characters | same, tokens the list knows | any Darija word shown |
|---|---|---|---|---|---|
| dev | 2 324 | 57.0% | 11.2% / 16.6% | 24% / 42% | 51% / 63% |
| **sealed** | 7 855 | **55.4%** | **9.8% / 16.6%** | 22% / 44% | 52% / 60% |

"Right word" counts another spelling of the word as right (same sound key). The exact spelling the writer used is in the top 3 for 10.1% (sealed, 3 characters).
"Knows" counts a listed word, another spelling of one, and a listed word with a negation or pronoun.

Why it is low: about half the words of real forum text are not in a 2 500-word list (verbs with clitics, Modern Standard Arabic words in Latin letters, names,
French); no list of this size has them.

**With the optional list** (`scripts/fetch-dz.ps1` -> `data/words/dz.txt`, merged by `load_dz`; built here from the sealed file only and measured on
the dev files, so not on its own text): the list knows 63.8% of the tokens (+6.8 points) and the right word is in the top 3 for 13.0% / 21.2% (+1.8 / +4.6 points).
A real but modest gain; a bigger corpus would give more.

### Speed

One `suggest()` call with Dz on: about 50 to 70 microseconds on average (release build) with the seed and the English and French lists. The Dz scan is
linear over the list; a list of 50 000 words would need an index.

### Known weaknesses and what to check

- **The numbers on the seed's own words are optimistic**; only section 3 says something about real writing: about 55% of the words known, the right one offered for 17%.
- **No standard spelling.** A variant the list does not hold is found through the sound key only if its sounds are the same; otherwise only by the consonants after 2 or 3 characters.
  Digit-letters vary a lot (`3` ain; `7` or `h`; `9` or `q`; `5` or `kh`; `8` or `gh`; `2` hamza; `6` is rare).
- **Dz-looking words nobody listed** (a verb with a new combination of clitics) are treated like any word by the spelling correction and may get a Fix to an English or French word.
  Dz protects only what it knows.
- **Mixed Darija and French** is the norm in Algeria; a word known to both counts for no side in the context vote, so a mostly French sentence with a few Darija words stays French.
- **Vulgar words are not in the seed** on purpose.
- Regional and dialect mix: the seed is Algerian; words shared with Moroccan or Tunisian Arabic (`dyal`, `bahi`, `sir`, `kach`) and western-only forms (`dorka`) are in `UNSURE`.

## Usage of dictionary words (learning ranks the words you write most higher)

The learning used to keep only words that are in no dictionary. It now also counts the words the user writes that ARE in a dictionary
(`PersonalDict` "usage", code in `crates/verba-local/src/personal.rs`, tests in `tests/usage.rs`), and `suggest` ranks a word higher the more the user
writes it **beyond what the dictionary leads one to expect**. Nothing changes until `learn_text` has been used.

### What is counted

Every word the user writes with the same refusals as before (digits, URLs, e-mails, paths, identifiers, hex strings, acronyms, under 3 or over 24 letters,
words the user removed), common words included. Counts are keyed by the word without accents or hamza, fade with a half-life of 4 000 `learn_text`
calls (about one per sentence), and are capped at 5 000 words (the least written, then the oldest, go first; counts that faded under 0.2 go too).
The total of words written is kept (faded the same way) to know what the dictionary leads one to expect. `learned.txt` gets two new line kinds,
`utotal` and `u`; the old files load unchanged, damaged lines are skipped, and a program that does not know the lines ignores them.
A word the spelling correction takes for a typo (the subtitle lists hold `wich`, `seperate`) is counted but flagged and never boosted; it is asked once, when
the count reaches 2.

### How the ranking works

For each counted word that starts like what was typed (leniently: accents and hamza may be left out, as for the ordinary completions), its dictionary score
is multiplied by a boost `1 + 3 * ln(r)`, bounded by **12**, where `r = count / (expected + 2)` and `expected = words written * share of the word in the
dictionary`. The word is boosted only when its count exceeds `expected` by 6 standard deviations (`(count - expected) / sqrt(expected + 2) >= 6`), so
chance repeats of a word do not count; a rare word needs about 9 writings, a common one several times what the dictionary predicts. The boosted copy is
added to the candidates next to the ordinary ones (the ordinary list is not touched), so a boosted word is never lost. The same boost applies at every length of
prefix, so two words keep their order while the user types; and the boost is bounded, so a word `x` times rarer than another can never pass it unless
`x <= 12` (a test checks that a word 1 300 times rarer stays behind however often it is written). The engine of a user who never used learning is
**byte for byte the engine of 0.4.0**: `tests/golden_suggest.rs` holds 3 000 questions on made-up lists and 4 100 on the real lists, answered by the code before this
change (commit e80bf34), and compares the answers now.

### Simulation

`cargo run --release -p verba-local --example usage_simulation [-- --lang en|fr|ar] [--tune] [--drift] [--nextword]`. A synthetic user has a core of 300
words drawn from a band of the dictionary's frequency ranks (so the vocabulary is skewed away from the dictionary) and writes a fraction of their words from it with
a Zipf law; the rest are drawn by the dictionary's own frequency. **Specialist**: core from ranks 300-15 000, 35% of the words; **mixed**: ranks 50-3 000, 20%;
**dictionary**: no core, writes as the dictionary says. The user's text goes to `learn_text` one sentence of 12 words at a time; the held-out text is
new draws from the same habits, never learned. Each word is typed 2, 3 and 4 letters deep (the engine shows nothing for 1) and the engine is asked for 3
completions. The dials were tuned on users with seeds 1-4 (the table below) and the results are on users with seeds 101-106.

**Result: top-3 hit rate, mean of prefixes of 2, 3 and 4 letters (top-1 in the last column), learning off, then after 2 000 and 10 000 words learned**

| | user | off | 2 000 words | 10 000 words | gain at prefix 2 / 3 / 4 (10 000) | top-1 off -> 10 000 |
|---|---|---|---|---|---|---|
| English | specialist | 60.1% | 67.6% (+7.5) | 70.8% (**+10.8**) | +7.9 / +14.3 / +10.0 | 38.6% -> 50.4% (+11.8) |
| English | mixed | 73.4% | 75.8% (+2.4) | 77.1% (+3.6) | +5.3 / +5.0 / +0.6 | 49.9% -> 56.3% (+6.3) |
| English | dictionary | 76.4% | 76.4% (0.0) | 76.4% (**0.0**) | 0.0 / 0.0 / 0.0 | 56.0% -> 56.0% (0.0) |
| French | specialist | 56.0% | 61.6% (+5.6) | 64.5% (**+8.5**) | +7.1 / +10.6 / +7.9 | 37.4% -> 44.0% (+6.6) |
| French | mixed | 66.8% | 69.6% (+2.8) | 71.2% (+4.4) | +5.6 / +5.8 / +1.8 | 44.5% -> 49.9% (+5.4) |
| French | dictionary | 70.5% | 70.5% (0.0) | 70.5% (**0.0**) | 0.0 / 0.0 / 0.0 | 48.3% -> 48.3% (0.0) |
| Arabic | specialist | 58.4% | 64.8% (+6.4) | 67.1% (**+8.8**) | +10.7 / +8.2 / +7.4 | 37.8% -> 48.0% (+10.2) |
| Arabic | mixed | 65.1% | 66.7% (+1.7) | 67.6% (+2.6) | +2.9 / +2.9 / +1.9 | 44.4% -> 47.9% (+3.6) |
| Arabic | dictionary | 66.3% | 66.3% (0.0) | 66.2% (**-0.1**) | -0.3 / 0.0 / 0.0 | 45.8% -> 45.7% (-0.1) |

- **The regression is nil to negligible**: a user who writes exactly as the dictionary says loses nothing in English and French, and 0.1 point (one hit in
  a thousand) in Arabic, on 6 users each. The dial that buys this is the significance gate (below); without it the same users lose 0.1 to 0.4 point.
- **The gain grows with what has been written** (2 000 words: about 2/3 of the 10 000 gain), is largest where the first letters leave many candidates (3 letters), and is
  small at 4 letters, where the dictionary already finds the word. A user whose habits are moderate (`mixed`) gains 2.6-4.4 points; one with a strong personal vocabulary 8.5-10.8.
- These are synthetic users: real people have habits of this kind (a trade, a project, names) but the sizes of the gains are those of the model, not a promise.
  The held-out words are drawn from the same unigram habits as the training text, which is what the ranking can learn; topics that change from day to day gain less.

**The dials, tuned on seeds 1-4 (change in top-3 after 10 000 words: specialist / mixed / dictionary)**

| Setting | English | French | Arabic |
|---|---|---|---|
| boost `1 + 0.9 ln(count)`, bound 8 | +8.6 / +3.0 / -0.3 | +5.8 / +2.5 / -0.2 | +5.8 / +2.2 / -0.5 |
| `r` = count over the expected count, `k` 0.9, bound 8 | +8.6 / +3.4 / -0.2 | +5.9 / +3.2 / -0.1 | +5.9 / +2.4 / -0.2 |
| same, `k` 1.8, bound 12, from 2 writings | +11.5 / +4.4 / -0.2 | +9.5 / +4.2 / -0.3 | +8.7 / +3.3 / -0.4 |
| same, padded expectation (2), `k` 2.4, no gate | +11.4 / +4.3 / -0.1 | +9.4 / +4.7 / -0.2 | +8.6 / +3.3 / -0.2 |
| gate at 4 standard deviations, `k` 3 | +11.7 / +4.7 / -0.1 | +9.5 / +5.1 / 0.0 | +8.5 / +3.4 / -0.1 |
| **gate at 6 standard deviations, `k` 3, bound 12 (shipped)** | **+11.1 / +4.5 / 0.0** | **+9.2 / +4.8 / 0.0** | **+8.0 / +3.3 / -0.1** |

- The bound: 2 loses about half of the gain and 4 loses 15-20%; 8 to 16 gain the same (a boost above 8 is rare), so 12 leaves room without letting a rare word
  pass a clearly better one. With the bound, "clearly better" means a word 12 times more frequent in the dictionary.
- Logarithm of the count alone and the count over the expected count differ little in the gain; the second is kept because it also protects the common words
  (a word the user writes as often as expected gets no boost). The significance gate is what removes the regression.
- The minimum count (2 or 3) made no difference (0.1 point or less) in the runs without the gate, and with the gate a rare word needs about 9 writings anyway; 2 is used, so the typo check is made early. (The shipped combination was only run on seeds 101-106, the table above.)
- **Fading.** A user who changes vocabulary after 10 000 words (new job, new project) and writes 4 000 words of a new one: on the new words the gain is +6.3 / +6.9 / +7.5
  (English / French / Arabic) with a half-life of 4 000 sentences and +6.3 / +6.9 / +7.6 without any fading; with a half-life of 100 sentences it is +4.6 / +5.2 / +6.4. **The
  simulation cannot show that fading helps**: old habits cost nothing here, because the new words overtake them quickly; a short half-life only forgets habits that are still real
  (the first vocabulary drops from 69.0% to 65.3% right after its 10 000 words with a half-life of 100). The 4 000 (about a month of writing) is kept as gentle, so that counts
  and the file do not grow for ever; it is a safety, not a measured gain.

### Cost

On the real English list, 2 500 random sentences of 12 words from a 20 000-word vocabulary (the worst case for the typo check, which runs when a word reaches 2 writings):
`learn_text` 400 µs per sentence; `suggest_in` with 4 700 counted words 19 µs per call (the 2 to 4 letter prefixes of the 3 000 commonest words); `save` 14 ms, 576 KB for 4 700 words
and their pairs; `load` 42 ms.

### Next words

The user's own pairs already come first once seen twice (`MIN_PAIR_USE`, now the dial `pair_min` of `UsageParams`; `tests/usage.rs::pairs_the_user_repeats_come_first`
and `tests/nextword.rs`). Simulation (`--nextword`; users with 300 core words, each with 2 favourite followers used 45% of the time; text followed the bundled
table otherwise, which makes the table's own predictions the "off" reference and flatters the table side; 10 000 words learned; top-3 of `next_words`, after a core word):

| pair used at | habits, English | habits, French | habits, Arabic | dictionary-like, English | French | Arabic |
|---|---|---|---|---|---|---|
| 1 | +18.1 | +11.1 | +9.5 | +7.4 | -0.1 | -2.8 |
| **2 (kept)** | +13.7 | +5.2 | +5.7 | +10.0 | +2.9 | +0.1 |
| 3 | +13.2 | +4.0 | +4.6 | +9.8 | +3.0 | +0.7 |
| 4 | +12.7 | +3.7 | +3.6 | +9.8 | +3.0 | +0.9 |

Using a pair after one sighting helps people with real habits by 5 points more but costs people without habits up to 3 points (a pair seen once is often chance and pushes out the
table's answer). The threshold of 2 is the compromise and **should stay**; if habits must be caught faster, a pair seen once could be offered after the table's first
answer instead of before it (not done: it needs a change of the ordering in `next_words`, measured only here). Raising it to 3 gains almost nothing for either.
Over all positions, learning gains 13-15 points in English and French and 7 in Arabic in this simulation, mostly from sentence openings and pairs of the user's core words.

### Privacy: what is stored now

`learned.txt` holds: single words that are in no dictionary with a count (`w`), single words that are in a dictionary, common ones included, with a faded count (`u`), the number of
words written in all (`utotal`), a counter that goes up by one per `learn_text` (not a date), and pairs of two neighbouring words with a count (`b`). **Never**: a sentence, three words in a
row, a date, the program, the place. At most 5 000 words of each kind and 20 000 pairs. Someone who reads the file learns which words and pairs the user writes most, and roughly how much they wrote
(the total), which is why it is a plain file that *Forget what it learned* or deleting it erases; `docs/INSTALL.md` says the same in the user's words.

## Algerian Darija in Arabic letters

Measured on 2026-11-16 with `cargo run --release -p verba-local --example dz_ar_accuracy` (`-- --misses 20` lists misses, `-- --quick` stops before the real-text part, which takes
a few minutes) and `spell_accuracy` with and without `--dz`. Tests: `crates/verba-local/tests/dz_arabic.rs`. Code: `dz.rs` (the same `DzList` as the Latin script, with Arabic keys).
The one `Lang::Dz` switch covers both scripts.

### Read this first: the seed list is written by the authors

The program ships a **seed of 3 573 words** in Arabic letters (`crates/verba-local/seed/dz-ar-seed.txt`, PolyForm Noncommercial 1.0.0), written from the authors' knowledge, with estimated weights.
2 054 of them are not in the 50 000-word Arabic list; the others are everyday words that Modern Standard Arabic shares. 95 are in the `UNSURE` section (may be Moroccan or Tunisian, or regional).
No Algerian reviewer has checked it yet. So the numbers measured on the seed's own words (sections 1 and 1b) are **optimistic**; only the real-text part (3) is not, and even there the first
5 000 comments were looked at to add about 190 plainly missing words (`راهي`, `تع`, `نتاع`, `يشفيك`...), which is why they are reported apart from the sealed part.

### A real corpus exists for the Arabic script, and it is used for measuring

"Algerian Dialect" (Zakaria Benmounah, Abdennour Boulesnane; Mendeley Data V2, doi 10.17632/zzwg3nnhsz.2): 45 000 YouTube comments from more than 30 Algerian press channels, mostly in Arabic letters,
labelled for sentiment (the labels are not used). Licence, from its Mendeley Data record: **Creative Commons Attribution 4.0 International (CC BY 4.0)**, "You can share, copy and modify this dataset so long as
you give appropriate credit, provide a link to the CC BY license, and indicate if changes were made [...] Note that further permission may be required for any content within the dataset that is identified as belonging
to a third party." It allows use, so: the comments are a **separate optional download** (`scripts/fetch-dz-ar.ps1` builds `data/words/dz-ar.txt`, `word count` lines only, from the words the Arabic list lacks), never copied
into the repository, and used to measure. It has no word-level dialect labels, so "Darija" below is operationally "words the Arabic list does not have" and "words the Darija list knows"; those two sets include misspellings,
stretched letters (`بزاااف`), names and Latin-script slang written in Arabic letters. About 37% of the words of the comments are outside the Arabic list: mostly dialect, but not only.
PADIC (Algiers and Annaba dialects, film and sitcom scripts) was looked at and not used: its SourceForge project says GPLv3.

### How it works

- A word typed in Arabic letters is looked up in the Arabic list as before; Darija is an extra pool next to it (`dz_ar_merge`), used for the Arabic keyboard (`suggest_in(Lang::Ar | Lang::Dz)`, or no keyboard hint).
- **Keys:** short vowels, hamza on alef, ta marbuta, alef maqsura (the existing `fold`), `ڨ گ` = `ق`, `ڤ` = `ف`, `پ` = `ب`, `چ` = `ش`, `ث` = `ت`, `ذ` = `د`, `ظ` = `ض`, a lone hamza dropped. Three tiers like the Latin
  script: exact prefix, same key (`قا` finds `ڨاع`), the consonants without `ا و ي` (`بزف` finds `بزاف`). Several spellings of one word are one suggestion, the one that looks like what is typed.
  The attached article and particles (`وكيفاش`, `بالطوموبيل`) are handled with the Arabic list's own particle logic.
- **Score:** `DZ_AR_PRIOR = 0.05` times the share in the Darija list, and a Darija word is offered only if it beats the best Arabic candidate (it never fills the free places of the popup). When the words just before are
  Darija (a word of the Darija list that the Arabic list holds 10 times less often votes for it, an Arabic-list word that Darija does not know votes against), Darija counts x1.5 and the Arabic list x0.3.
- **The suggestions carry `Lang::Ar`, not `Lang::Dz`**: they read right to left and the popup lays text out by the language of the suggestion (`Lang::Dz` is left to right). `Lang::Dz` stays for the Latin script.
- **Spelling correction:** it never writes a Darija word (the Darija pool is not one of its lists), and leaves alone a word Dz knows: listed, another spelling with the same strict key (a word that is also in the Arabic list
  is left to the Arabic list), or a listed word with a particle, `ما`/`م`, `ن ت ي`, `ش` and a pronoun around it (`ماعرفتش`, `نشوفك`, `وكيفاش`). The strict key does not drop a hamza or collapse doubled letters, so that
  `النساء` and its typos are not taken for `نسا`.
- Dz off: nothing changes (test `with_darija_off_nothing_changes_in_arabic`, and the tool, on 1 500 prefixes and typos).

### 1. The seed's own words (optimistic)

3 118 seed words of 4+ letters typed letter by letter with the Arabic list and Dz on, 3 suggestions (top-1 / top-3):

| context | 2 letters | 3 letters | 4 letters |
|---|---|---|---|
| nothing before, or Modern Standard Arabic before (`ذهبت الى`) | 2.4% / 6.1% | 20.5% / 34.8% | 43.2% / 62.6% |
| `واش راني` before (Darija) | 7.5% / 22.5% | 31.9% / 61.4% | 54.8% / 83.5% |

Low without a Darija context because the Arabic list (50 000 words, subtitles, with some everyday speech) holds many words with the same start, and a Darija word has to beat them: `كيف` gives `كيفية` first, `علا`
gives `علاقة` first. (Modern Standard before gives the same as nothing: an Arabic context never helps Darija.) **Spelling held out (1b):** 83 spellings that have a sibling spelling were removed and typed to 3 letters
(Darija alone): a sibling is in the top 3 for 33.7%. Weaker than for the Latin script (61.5%), because the Arabic spellings differ mostly by letters that the key already merges.

### 2. False alarms and spelling correction on Modern Standard Arabic

The 3000 most frequent words (4+ letters) of the Arabic list typed letter by letter, 3 suggestions, Dz on against the Arabic list alone:

| letters | any new suggestion in the top 3 | a Darija-only word (not in the Arabic list) in the top 3 | it is the first suggestion |
|---|---|---|---|
| 2 | 1.00% | 0.63% | 0.63% |
| 3 | 0.57% | 0.33% | 0.33% |
| 4 | 1.26% | 0.66% | 0.66% |

Typical: `علا` -> `علاش` first, `كيف` -> `كيفاش` first, `هاد` -> `هادا`. They are Darija words an Algerian might mean, not wrong words. `DZ_AR_PRIOR` was chosen from this: 0.02 gives 0.0 to 0.2% but finds
Darija words only after a Darija context; 0.1 gives 1.3 to 1.9% Darija-only; 0.3 gives 7 to 9%. Guard in the tests: under 2% of the prefixes of ordinary words may show a new suggestion.

**Typo recovery with Dz on, against off** (`spell_accuracy`, the 3000-rank test, Arabic, `correct(Some(Ar))`):

| | top-1 | top-3 | nothing offered |
|---|---|---|---|
| Dz off (as before) | 62.6% | 70.1% | 23.7% |
| **Dz on** | **62.1%** | **69.4%** | 24.5% |

By kind (top-1, off to on): transposition 80.5 to 79.4, extra letter 86.9 to 86.7, neighbour key 73.6 to 72.9, other letter 78.0 to 77.2, two slips 24.2 to 24.2, letter missing 21.5 to 20.7.
Rarer words (ranks 3000 to 12000): top-1 31.0% to 30.7%. **Words wrongly fixed:** dictionary words ranked 2000 to 30000: 2.1% (582 of 27 822) to 2.0% (552); the 14 355 words of ranks 30000 to 50000 that this
dictionary lacks (names, rare words, inflections) get a fix 13.8% of the time, then 13.3%. So Dz on costs 0.5 to 0.8 points of recovery (typos that happen to look like a Darija word, such as `لدنيا` read as `ل` + `دنيا`,
`الحومة`, `تعيطني`, are no longer fixed) and removes some wrong fixes. The tool's own typos (`dz_ar_accuracy`, 5+ letter words, swapped / doubled / dropped letters, top-1): 83.7 / 86.7 / 29.3% with Dz against 84.5 / 86.7 / 29.9% without.
The English and French rows of `spell_accuracy --dz` move by 0.1 to 0.5 points with both scripts on (the Latin script alone was measured in its own section).

### 3. Real text: the YouTube comments (CC BY 4.0, measured on, not copied)

Each word typed as a prefix after the (up to 3) words before it in its comment; every 4th word, 3 suggestions. "Exact word" = the spelling the writer used.
**dev** = comments 1 to 5 000 (read while finishing the seed). **sealed** = comments 25 001 to 45 000 (never read). Comments 5 001 to 25 000 are used only to build the optional list.

| set | words (3+ letters) | not in the Arabic list | the Darija list knows | of those not in the Arabic list |
|---|---|---|---|---|
| dev | 49 427 | 37.2% | 40.7% | 33.9% |
| **sealed** | 201 751 | 37.7% | **40.4%** | **33.0%** |

Exact word in the top 3 / first, Dz on against without Dz (sealed; dev is within 0.6 points):

| words | letters | words typed | top-3 on | top-3 off | top-1 on | top-1 off |
|---|---|---|---|---|---|---|
| all | 2 | 40 371 | 12.3% | 7.9% | 6.3% | 3.7% |
| all | 3 | 25 834 | 17.5% | 13.8% | 8.6% | 6.5% |
| the Arabic list lacks | 2 | 18 010 | 4.7% | 0.0% | 2.9% | 0.0% |
| the Arabic list lacks | 3 | 13 640 | 7.8% | 0.5% | 3.5% | 0.1% |
| the Darija list knows | 3 | 7 109 | 32.8% | 14.7% | 17.5% | 6.8% |

The Arabic list alone finds almost none of the words it lacks, as expected; with Darija on, 8% of them after 3 letters (the seed knows a third of them). Completion of all the words rises from 13.8% to 17.5%
(top-3, 3 letters). Some hits are lost where the Darija context pushes a Modern Standard word down (`--misses` lists them); the net is positive.

**Spelling correction on the real words** (sealed): of 6 000 words the Arabic list lacks, **10.4% get a Fix with Dz on, 16.1% without**; of 6 000 words the Arabic list has, 0.8% against 1.5%.
A Fix on a word the dictionary lacks is not always wrong (some are typos), so this says "fewer words rewritten", not "fewer mistakes": the real words are not labelled.

**With the optional list** (`scripts/fetch-dz-ar.ps1`; here built from comments 5 001 to 25 000 only, 3 005 words, and measured on the sealed part): the list knows 50.3% of all words and 55.3% of those the Arabic list lacks; exact word
in the top 3 for all words 14.3% / 21.4% (2 / 3 letters), for words the Arabic list lacks 10.1% / 16.3%; Fixes on words the Arabic list lacks 5.9%. Real, but it is the same kind of text (YouTube comments from the same channels) as the sealed part,
so it says what a corpus of this kind brings, not what any Algerian text would get.

### Speed

One `suggest()` call in an Arabic sentence (release build): about 80 microseconds with Darija on, 8 without; the Darija scan is linear over the 3 600-word list. `dz_knows` tries about 4 000 prefix and suffix combinations on an unknown
Arabic word in the worst case, and only when a word has just been finished.

### Known weaknesses and what to check

- **The numbers on the seed's own words are optimistic**, and the dev part of the real text was used to add words. The sealed part is the number to read: about a third of the dialect words known, 8% of them found after 3 letters (17.5% of all words, against 13.8% without Darija).
- **Without a Darija context Darija is deliberately timid** (it must beat the Arabic list): one word like `واش` before is enough to switch it on, a first word is not.
- **No standard spelling.** Spellings that differ by more than the merged letters (`كيفاش` / `كفاش` / `كيفاه`, vowel letters dropped; `ڨ` for `ق` is merged but `ك` for `ق` is not) are found only through the consonant tier, after 2 or 3 letters.
- **The seed mixes Algerian with what is shared**: `UNSURE` holds the Moroccan-looking (`ديال`, `شنو`, `فاش`), Tunisian-looking (`توا`, `باهي`, `هكا`) and regional forms (`دوك` / `دورك`, `كيفاه`, `علاه`). Politics, football and religion dominate the comments, so the seed
  is thinner for everyday speech than for those.
- **The correction protects a Darija word only if Dz knows it**; an unlisted word is still checked like any Arabic word. Politically charged and vulgar vocabulary is left out of the seed on purpose.
- **Popup direction:** Darija suggestions are tagged `Lang::Ar` so the popup draws them right to left; code that wants to tell them from Arabic-list suggestions has to look them up in `DzList::seed_ar()`.
