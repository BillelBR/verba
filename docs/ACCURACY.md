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

## Word translation

`LocalEngine::translate_word` looks a word up in `data/words/dict.txt` (English, French, Arabic, every direction; `translate.rs` has the engine side). This section measures the DATA: what is in the file, how fast it
loads, how many of the commonest words it knows, and whether the first translation is a good one. Measured on 2026-11-17 on the file built that day from the kaikki.org data of 2026-10-30 (source, license and the rules of the build:
[DATA.md](DATA.md#word-translation-datawordsdicttxt)); `cargo test -p verba-local --test dictionary` checks the well-known cases again on the real file (it skips when the file is missing).

```powershell
powershell -ExecutionPolicy Bypass -File scripts\fetch-dict.ps1        # download + extract + build (add -Keep to keep the downloads and extracts, -MaxMB 24 for a larger file)
cargo run --release -p verba-local --example dict_probe -- --bench --coverage 5000
cargo run --release -p verba-local --example dict_probe -- hello chien كتاب      # what the dictionary says about words
```

### What is in the file

| Direction | Lines written | Of them, lemma headwords | Source of the translations |
|---|---|---|---|
| en>fr | 104 613 | 90 109 | translation tables of the English entries; French glosses read backwards |
| fr>en | 128 846 | 106 487 | English glosses of the French entries (strict); translation tables read backwards |
| en>ar | 46 005 | 32 802 | translation tables; Arabic glosses read backwards |
| ar>en | 63 601 | 32 783 | English glosses of the Arabic entries (strict); translation tables read backwards |
| fr>ar | 11 858 | 11 858 | worked out from fr>en and en>ar, the 30 000 commonest French words |
| ar>fr | 5 986 | 5 986 | worked out from ar>en and en>fr, the 30 000 commonest Arabic words |
| **total** | **360 909** | | |

- The other lines are inflected forms (the plural `dogs`, the conjugated `parlons`, the Arabic `الكتاب` with its article): 14 504 (en>fr), 22 359 (fr>en), 13 203 (en>ar) and 30 818 (ar>en).
- **Size on disk: 14 680 699 bytes (14.0 MiB).** Without the cut the same data would be 943 720 lines and 37.7 MiB (headwords 10.2 MiB, forms that are words of the frequency lists 2.3 MiB, all other forms 25.2 MiB). The cut keeps every headword
  and every form that is a word of the lists (the commonest forms first); what is lost is rare inflections (the Arabic dual, the French literary tenses of rare verbs).
- The English file has 1 499 161 entries, 139 931 of them give a translation into French (104 199 pairs) or Arabic (35 732 pairs). English Wiktionary is much richer in French than in Arabic, and the Arabic glosses are shorter in number (19 571 headwords with a gloss).
- **Gloss-based directions are strict**: a gloss is kept only when it is one to three plain words ("to write" gives `write`, "greetings; hello (general salutation)" gives `greetings`, `hello`; "the act of ...", "a person who ..." give nothing).
  French: of 84 389 headwords that have glosses, 76 979 have at least one short gloss, so **7 410 (8.8%) are left without** (375, or 1.9%, among the 20 000 commonest French words). Arabic: 19 571 headwords, 18 401 with a short gloss, **1 170 (6.0%) without**
  (86, or 0.4%, among the 20 000 commonest). Some of them still get a line through the translation tables read backwards.

### Cost

| | |
|---|---|
| `dict.txt` on disk | 14.0 MiB |
| `Dictionary::parse` of the text (release, text already in memory) | median 298 ms (best 287, worst 302, 5 runs) |
| `LocalEngine::load_dir` with the dictionary, reading all the files (word lists, next-word tables, dictionary) | median 581 ms (3 runs) |
| the same without `dict.txt` | median 293 ms (3 runs) |
| memory of the parsed dictionary (a counting allocator around the parse) | 79 MB (the 14 MB of text is freed after loading) |
| one lookup | a hash lookup per direction (not timed on its own) |

The engine reads the file at start-up on a background thread, so the cost is a delay of about 0.3 s and 80 MB of memory (more with the allocator's overhead). The memory grows with the file at about 5.6 MB per MB of text: on earlier builds of the same data, a 12 MB file
took 67 MB and 254 ms to parse, 18 MB took 103 MB and 369 ms, 24 MB took 143 MB and 514 ms. The in-memory form is not compact (a `String` per headword and per translation); it could be made about 3 times smaller if memory matters.

**Build time** (this PC, Windows 10, release build, 35 MB/s download): 67 to 72 s with a fresh download (21 s download of the three files, 33 s unpacking and extracting, 12 to 14 s joining and writing, the rest compiling the tool), 48 s when the downloads are already on disk.
The 3.4 GB English text is unpacked as a stream into the extractor and never stored; the largest file on disk is the 534 MB English `.gz`, deleted after use. On the GitHub runner the download is the part that may be slower; the release step has a limit of 25 minutes.
Peak memory of the tool (peak working set, polled every 50 ms): the extractor 8 MB for the 3.4 GB English text (it holds one line at a time); the joining step 1.7 GB (it holds every candidate of every direction before cutting; a PC or runner with 4 GB is enough).

### Coverage: the commonest words that have a translation

For the 5 000 commonest words of each frequency list (`data/words/{en,fr,ar}.txt`), the share that has a translation into each of the other two languages, looked up as the engine does (`from` = the list's own language). "Plain" leaves out tokens with a digit,
apostrophe or hyphen and one-letter tokens (the subtitle lists have many: `'s`, `n't`, `ll`).

| From | To | all 5 000 | plain words | looked up directly |
|---|---|---|---|---|
| English | French | 93.7% | 94.7% | 4 684 of 4 684 |
| English | Arabic | 90.1% | 91.1% | 4 505 of 4 505 |
| French | English | 91.2% | 94.1% | 4 562 of 4 562 |
| French | Arabic | 89.9% | 92.8% | 3 238 of 4 496 (the rest through English) |
| Arabic | English | 83.7% | 83.8% | 4 185 of 4 185 |
| Arabic | French | 83.3% | 83.4% | 2 184 of 4 165 (the rest through English) |

The words without a translation are mostly names (`kelly`, `marie`), interjections (`ooh`), contractions and, in Arabic, forms with attached particles and pronoun endings (`مازالت`, `رأيتك`, `سيارتك`), which Wiktionary has no entry for.
Arabic is the weak one: the Arabic subtitle list is full of such forms.

### Is the first translation a good one? A sample judged by hand

**Method.** For each direction, 60 words drawn at random (Python's `random.sample` with seed 11 over the words of 3 or more plain letters among the 3 000 commonest of the list; the words are in the tables below); for the two French-Arabic directions 30 words. The words were looked up as a user would (`dict_probe --from <language> --to <language>`)
and judged by the author of this section, who knows English, French and Arabic but is **not an Algerian or a professional translator**. A first candidate counts when a speaker would accept it as a translation of the word in some common sense (and in the form of the lemma:
`dogs` -> `chien` counts, the plural is the reader's business); "top 3" when one of the first three does. Words that are names missing from the dictionary, interjections and other tokens of the list that are not words of that language are not counted
("not counted"). The draw was made after the rules of the build were fixed; the rules had been tuned on about 40 other words (`house`, `water`, `light`, `bank`, `book`, ...) and on two earlier draws, so the numbers are not blind.
Judging is lenient on purpose where a word has several translations (`fine` -> `fin` counts), and strict where the first candidate is another sense than the common one (`minutes` -> `procès-verbal`, `plus` -> `plus`).

| Direction | words | not counted | judged | first candidate good | one of the first 3 good | of the words that have an entry: first / top 3 |
|---|---|---|---|---|---|---|
| English -> French | 60 | 1 | 59 | **57 (96.6%)** | **59 (100%)** | 96.6% / 100% |
| French -> English | 60 | 4 | 56 | **49 (87.5%)** | **54 (96.4%)** | 87.5% / 96.4% |
| English -> Arabic | 60 | 3 | 57 | **49 (86.0%)** | **54 (94.7%)** | 87.5% / 96.4% |
| Arabic -> English | 60 | 4 | 56 | **45 (80.4%)** | **47 (83.9%)** | 88.2% / 92.2% |
| French -> Arabic | 30 | 2 | 28 | **24 (85.7%)** | **26 (92.9%)** | 85.7% / 92.9% |
| Arabic -> French | 30 | 2 | 28 | **21 (75.0%)** | **22 (78.6%)** | 80.8% / 84.6% |

An earlier draw of 60 English words (seed 7, a slightly earlier build, 57 judged) gave 5 misses of the first candidate and 1 of the first three (91.2% / 98.2%), so the English -> French figure above is on the lucky side of "about 90% first, 98% in the first three".
Words outside the 3 000 commonest are worse: the lists are shorter and the first sense is more often a rare one.

**What goes wrong**

- **Words with several meanings** (the honest limit of a word-by-word dictionary with no context). `bank` gives `banque; banc; rive; grève; bancaire`: the building first, the river bank third, but
  `light` gives `clair; allumer; light; léger; lumière`: four true meanings, and which one is wanted depends on the sentence. `marché` gives `مسيرة; سوق` (walk, market): the French word is both the noun and the participle of `marcher`. `minutes` gives the minutes of a meeting first.
  `love` gives `aimer` before `amour`. The order comes from the senses' order in Wiktionary, the number of sources, and the frequency of the translated word, never from the sentence: show several candidates, as the popup does.
- **Homographs and forms.** A form is looked up without its context: `voyais` (I saw) gives `please; sight; behold`, `aura` gives the noun, `montré` gives `watch` (from `montre`), `pis` gives the udder. Arabic without short vowels merges words: `كرة` is `ball` and `hate`, `اسم` is `name` and `poison`.
- **Arabic verbs and roots.** An Arabic verb headword is the past `he did`, so `كتب` gives `write` (good) but the same letters are also `books` (plural of `كتاب`) and `he made write`; the forms of a verb (`أكتب`, `يكتب`) come from the tables of the entries and are there for the commonest verbs only.
  Derived forms (the second to tenth forms of a verb) are separate entries with their own glosses and are mostly short. Nothing in the data links a word to its root, so root-based lookups do not exist.
- **Gloss-derived directions (fr>en, ar>en) are noisier after the first candidate**: the second and third are often other senses or a neighbour word (`ترتدي` gives `wear; move back quickly; put on`). First candidates are good about 88% of the time when there is an entry.
- **Gaps in the English tables**: the translation tables of very common English words are often on a subpage that the data does not hold. `meant` gives `moyen; méchant` (the adjective `mean`) because the verb's table is missing; `anywhere` has no Arabic translation at all.
  The reverse glosses (French and Arabic entries that say `house`) fill many of these gaps, but with less certain ordering.
- **Rude words are left out by rule** (`fuck`, `slut`, `bitch` have no entry; see [DATA.md](DATA.md#word-translation-datawordsdicttxt)): a user who looks one up gets nothing, not a wrong word.
- **A name that exists as an ordinary word** is read as the word (`كيلي` is `measure`, not Kelly; `ماكس` is `collect taxes`).

### Everyday words an Algerian speaker may look up

Thirty words and phrases, French and English into Arabic, as the engine gives them (the first three candidates shown; the verdict is the author's, **not an Algerian reviewer's**; the dictionary is Modern Standard Arabic with a few Maghrebi words, it knows no Darija
spelling such as `واش`, `شحال` or `برك`).

| Looked up | Gives | Good? |
|---|---|---|
| bonjour | مرحبا; أهلا; آلو | yes (an Algerian would also say صباح الخير / السلام عليكم, which is 4th) |
| merci | شكرا; شكر; شكرا لك | yes |
| s'il vous plaît | أرجوك; رجاء; أعجب | yes, but the 3rd is noise (goes through English: `please` also means "to please") |
| au revoir | وداعا; إلى اللقاء; مع السلامة | yes |
| bienvenue | مرحبا; أهلا وسهلا; أهلا | yes |
| école | مدرسة; كلية; جامعة | yes |
| médecin | دكتور; طبيب; حكيم | yes |
| pharmacie | صيدلية; صيدلة | yes |
| voiture | سيارة; عربة | yes |
| marché | مسيرة; سوق; سار | **no, first is "march"** (market is 2nd) |
| ordonnance | وصفة; أمر; وصفة طبية | yes (the medical prescription, then the order) |
| demain | غدا; غد; الغد | yes |
| maintenant | الآن; حالا; هنا | yes |
| mairie | بلدية | yes (Algerian: البلدية) |
| baccalauréat | (no entry) | **no** |
| carte d'identité | بطاقة تحقيق الشخصية; بطاقة تعريف; بطاقة شخصي | partly: the 2nd is the usual one (بطاقة التعريف) |
| hello | مرحبا; أهلا; آلو | yes |
| thank you | شكرا; شكرا لك; الله يبارك فيك | yes (the 3rd is a Maghrebi-flavoured phrase) |
| please | أرجوك; رجاء; أعجب | yes (3rd is noise) |
| good morning | صباح الخير; صباح النور | yes |
| goodbye | وداعا; إلى اللقاء; مع السلامة | yes |
| welcome | مرحبا; أهلا وسهلا; أهلا | yes |
| bread | خبز; قرص | yes |
| market | سوق | yes |
| prescription | وصفة; وصفة طبية; سن | yes |
| tomorrow | غدا; غد; الغد | yes |
| a lot | كثير; كثيرا | yes |
| money | مال; نقود; أموال | yes (colloquial `دراهم`/`فلوس` not there) |
| how much | كم; بكم | yes |
| good night | تصبح على خير; ليلة سعيدة | yes |

27 of 30 are good first (28 with `carte d'identité`); the failures are a word with two meanings (`marché`) and a word the data does not have (`baccalauréat`). Other Algerian terms tried and not found: `wilaya`, `chorba`, `kamis`. `casse-croûte` gives
`وجبة خفيفة; لمجة; تصبيرة` (the last two are Maghrebi words from Wiktionary), `semoule` gives `سميذ; سميد`.

### Known weaknesses of the dictionary

- No context: a word is translated alone, in the order of the commonest senses, and the user picks. Show at least 3 candidates.
- Arabic with attached particles and pronoun endings (`وبالكتاب`, `سيارتك`, `رأيتك`) is mostly not found; about one Arabic word in six of the 5 000 commonest is in that class. The engine could strip the article, a one-letter preposition or conjunction and a pronoun ending before the lookup.
- The Arabic side is Modern Standard Arabic with a little Maghrebi: no Darija spelling, no Latin-letter (Arabizi) input. English Wiktionary has 98 translations marked as Algerian Arabic.
- Multi-word headwords (up to 3 words) exist, but a selection of a whole phrase finds them only if the phrase is in Wiktionary as an entry.
- Proper nouns are few (names are mostly absent); abbreviations are treated like other words.
- Judging is by one person and the samples are small (60 words per direction): the percentages have a margin of about plus or minus 8 points.

### Where this goes next

- A frequency of senses in real text (a parallel corpus such as Tatoeba, CC BY 2.0 FR) would put the commonest sense of a polysemous word first better than Wiktionary's order.
- The French edition of Wiktionary (`kaikki.org/dictionary/` has a "French Wiktionary edition" extract) has French definitions and translations of its own and might fill the Arabic side.
- Arabic clitic stripping in the engine, as said above.

<details>
<summary>The six samples, word by word (how the numbers above were counted)</summary>

**en>fr**

| word | the dictionary gives (first 3) | 1st | top 3 | a good translation |
|---|---|---|---|---|
| ruin | ruine; ruiner; gâcher | yes | yes | ruine |
| hung | pendu; pendre; monté | yes | yes | pendu |
| kelly | Kelly | yes | yes | Kelly |
| genius | génie; bel esprit; aigle | yes | yes | génie |
| advantage | avantage; favoriser; atout | yes | yes | avantage |
| lily | lys; lis | yes | yes | lys |
| anywhere | n'importe où | yes | yes | n'importe où |
| thinks | penser; trouver; pensée | yes | yes | pense |
| vacation | vacances; aller en vacances; vacance | yes | yes | vacances |
| eventually | finalement; à terme; finir par | yes | yes | finalement |
| settled | régler; réglé; arrêté | yes | yes | réglé, installé |
| education | éducation; enseignement; formation | yes | yes | éducation |
| across | à travers; en travers; outre | yes | yes | à travers |
| chance | chance; hasard; risque | yes | yes | chance |
| murderer | meurtrier; assassin; meurtrière | yes | yes | meurtrier |
| sarah | Sarah; Sara | yes | yes | Sarah |
| street | rue | yes | yes | rue |
| minutes | procès-verbal; minute; minuscule | **no** | yes | minutes (also: procès-verbal) |
| remains | restes; dépouille; cendres | yes | yes | restes |
| blast | rafale; explosion; souffle | yes | yes | explosion, rafale |
| tiger | tigre; tigresse | yes | yes | tigre |
| fine | fin; bien; amende | yes | yes | bien, fin |
| sucks | sucer; super; téter | yes | yes | suce |
| distance | distance; froid; éloigner | yes | yes | distance |
| memories | mémoire; souvenir; stockage | yes | yes | souvenirs |
| billion | milliard; millions; mille milliards | yes | yes | milliard |
| tank | tank; réservoir; cuve | yes | yes | réservoir, char |
| marie | -- | - | - | (name, no entry) |
| clean | propre; net; nettoyer | yes | yes | propre |
| sand | sable; plage; chevalier | yes | yes | sable |
| had | avoir; tenir; prendre | yes | yes | avait, eu |
| louis | Louis; Ludovic | yes | yes | Louis |
| door | porte; portière; huis | yes | yes | porte |
| miss | manquer; rater; raté | yes | yes | manquer |
| years | année; an; millésime | yes | yes | années, ans |
| bill | bill; note; bec | **no** | yes | facture |
| sleeping | dormir; sommeil; couchage | yes | yes | dormant |
| writer | écrivain; auteur; écrivaine | yes | yes | écrivain |
| wait | attente; attendre; patienter | yes | yes | attendre |
| avoid | éviter; fuir; soustraire | yes | yes | éviter |
| rose | rose; rosier; rosé | yes | yes | rose |
| growing | grandir; grandissant; croître | yes | yes | grandissant |
| learning | apprentissage; apprendre; étudier | yes | yes | apprentissage |
| join | joindre; rejoindre; appareiller | yes | yes | joindre, rejoindre |
| hearts | cœur; cardiaque; âme | yes | yes | cœurs |
| carry | porter; entraîner; charrier | yes | yes | porter |
| maid | bonne; demoiselle; domestique | yes | yes | bonne, femme de chambre |
| hiding | cacher; dissimuler; peau | yes | yes | cachant |
| terms | terme; limite; condition | yes | yes | termes, conditions |
| just | juste; que; équitable | yes | yes | juste |
| article | article; chef; papier | yes | yes | article |
| crazy | fou; insensé; taré | yes | yes | fou |
| victims | victime | yes | yes | victimes |
| agents | agent; agente; procureur | yes | yes | agents |
| gay | gay; homo; gai | yes | yes | gay |
| split | fendre; diviser; couper | yes | yes | fendre, diviser |
| tear | déchirer; déchirure; larme | yes | yes | déchirer, larme |
| kids | gamin; gosse; môme | yes | yes | enfants, gamins |
| bus | bus; autobus; car | yes | yes | bus |
| local | local; du coin; section | yes | yes | local |

**fr>en**

| word | the dictionary gives (first 3) | 1st | top 3 | a good translation |
|---|---|---|---|---|
| mariés | married; marry; wed | yes | yes | married |
| scientifique | scientific; scientist | yes | yes | scientific, scientist |
| célèbre | famous; celebrate; celebrated | yes | yes | famous |
| queue | tail; queue; line | yes | yes | tail, queue |
| arthur | Arthur | yes | yes | Arthur |
| pis | udder; and; trouble | **no** | **no** | worse (also: udder) |
| plusieurs | several; many; lot | yes | yes | several |
| porter | porter; carry; wear | **no** | yes | carry, wear |
| minuit | midnight | yes | yes | midnight |
| enlevé | remove; take off; kidnap | yes | yes | removed, kidnapped |
| vengeance | revenge; vengeance | yes | yes | revenge |
| voyais | please; sight; behold | **no** | **no** | saw, was seeing |
| balle | ball; bullet; chaff | yes | yes | ball, bullet |
| affaires | business; belongings; busy | yes | yes | business, things |
| style | style; elegant; stylish | yes | yes | style |
| retrouvé | find; remember; rediscover | yes | yes | found (again) |
| suppose | suppose; alleged; think | yes | yes | suppose |
| vérité | truth; fact; reality | yes | yes | truth |
| différente | different; dissimilar; assorted | yes | yes | different |
| king | -- | - | - | (English word in the French list) |
| problème | problem; trouble; conundrum | yes | yes | problem |
| lucy | -- | - | - | (name, no entry) |
| également | equally; too; also | yes | yes | also |
| dira | say; tell; saying | yes | yes | will say |
| étoile | star; starry; starred | yes | yes | star |
| sauve | save; safe; except | yes | yes | saves |
| objets | object; thing; aim | yes | yes | objects |
| secret | secret; secrecy; undercover | yes | yes | secret |
| écran | screen; monitor; display | yes | yes | screen |
| vraiment | really; truly; very | yes | yes | really |
| posé | pose; installation; extension | yes | yes | placed, put down |
| dernière | last; latter; latest | yes | yes | last |
| sang | blood | yes | yes | blood |
| jour | day; daylight; light | yes | yes | day |
| ferait | make; do; go | yes | yes | would do, would make |
| prise | hold; socket; take | yes | yes | taken, socket, grip |
| stan | -- | - | - | (name, no entry) |
| mort | death; dead; die | yes | yes | death, dead |
| feras | make; do; go | yes | yes | will do |
| remarqué | remark; notice; observe | **no** | yes | noticed |
| cousin | cousin; mosquito; cranefly | yes | yes | cousin |
| reviendrai | return; come back; grow back | yes | yes | will come back |
| espèce | species; type; sort | yes | yes | species, kind |
| tir | shooting; shot; pitch | yes | yes | shot, shooting |
| appartement | apartment; flat; tenement | yes | yes | apartment |
| retournez | turn over; return; flip | yes | yes | return, turn over |
| prouver | prove; evidence | yes | yes | prove |
| premiers | first; premier; prime | yes | yes | first |
| plus | plus; more; most | **no** | yes | more |
| créature | creature; being | yes | yes | creature |
| aura | aura; have; vibe | **no** | yes | will have |
| casse | broken; case; break | yes | yes | breaks |
| ooh | -- | - | - | (interjection, no entry) |
| comprenez | understand; include; comprehend | yes | yes | understand |
| montré | watch; wristwatch; show | **no** | yes | shown |
| rentrez | go back; go home; tuck | yes | yes | go back, go home |
| espère | hope; wait; trust | yes | yes | hope |
| vol | flight; theft; stealing | yes | yes | flight, theft |
| arranger | arrange; suit; fix | yes | yes | arrange |
| aille | go; attend; be | yes | yes | go |

**en>ar**

| word | the dictionary gives (first 3) | 1st | top 3 | a good translation |
|---|---|---|---|---|
| ruin | خراب; دمر; طلل | yes | yes | خراب، دمار |
| hung | علق; شنق; تعلق | yes | yes | علّق |
| kelly | -- | - | - | (name, no entry) |
| genius | عبقري; عبقرية | yes | yes | عبقري |
| advantage | فائدة; أفضلية; ميزة | **no** | yes | ميزة |
| lily | زنبق; سوسن | yes | yes | زنبق |
| anywhere | -- | **no** | **no** | في أي مكان |
| thinks | فكر; تفكر; ظن | yes | yes | يفكّر |
| vacation | عطلة; إجازة | yes | yes | عطلة |
| eventually | أخيرا; في الآخر | yes | yes | أخيرا |
| settled | تحضر; حاضر; سوى | **no** | **no** | مستقر |
| education | تربية; تعليم; ثقافة | yes | yes | تعليم |
| across | عبر | yes | yes | عبر |
| chance | فرصة; صدفة; باب | yes | yes | فرصة |
| murderer | قاتل; قاتلة; سفاح | yes | yes | قاتل |
| sarah | سارة | yes | yes | سارة |
| street | شارع; طريق; حارة | yes | yes | شارع |
| minutes | دقيقة; دقيق; تافه | yes | yes | دقائق |
| remains | تبقى; باقي; بقي | yes | yes | يبقى، بقايا |
| blast | لفح; عصف | **no** | yes | انفجار، عصف |
| tiger | نمر; ببر | yes | yes | نمر |
| fine | حسنا; غرامة; بخير | yes | yes | حسنا، جيد |
| sucks | مص; رشف; مصة | yes | yes | يمصّ |
| distance | بعد; مسافة; بين | yes | yes | مسافة |
| memories | ذاكرة; ذكرى; حافظة | yes | yes | ذكريات |
| billion | مليار | yes | yes | مليار |
| tank | دبابة; خزان; فنطاس | yes | yes | دبابة، خزان |
| marie | متزوج; متزوجة; تزوج | - | - | (name; the engine found it in French) |
| clean | نظيف; نظف; نقي | yes | yes | نظيف |
| sand | رمل; دريجة | yes | yes | رمل |
| had | ل; عند; ملك | **no** | **no** | كان لديه |
| louis | -- | - | - | (name, no entry) |
| door | باب; أبواب; مدخل | yes | yes | باب |
| miss | أخطأ; آنسة; افتقد | yes | yes | أخطأ، اشتاق |
| years | عام; سنة; سنوات | yes | yes | سنوات |
| bill | فاتورة; لائحة; بيل | yes | yes | فاتورة |
| sleeping | نام; نوم; سنة | yes | yes | نائم |
| writer | كاتب; مؤلف; كاتبة | yes | yes | كاتب |
| wait | انتظر | yes | yes | انتظر |
| avoid | تجنب; تفادى; ابتعد | yes | yes | تجنّب |
| rose | وردة; شخص; نهض | yes | yes | وردة |
| growing | كبر; زائد; أمر | yes | yes | نامٍ |
| learning | تعلم; درس; أستاذ | yes | yes | تعلّم |
| join | وصل; اتصل; أوصل | yes | yes | انضمّ |
| hearts | قلب; عند; جوهر | yes | yes | قلوب |
| carry | حمل; قل; احتمل | yes | yes | حمل |
| maid | فتاة; جارية; خادمة | **no** | yes | خادمة |
| hiding | أخفى; خبأ; جلد | yes | yes | اختباء |
| terms | شرط; مصطلح; مهلة | yes | yes | شروط |
| just | فقط; عادل; عدل | yes | yes | فقط، عادل |
| article | مقالة; مقال; مادة | yes | yes | مقال |
| crazy | أحمق; مجنون; مسعور | **no** | yes | مجنون |
| victims | ضحية; أضحى | yes | yes | ضحايا |
| agents | فاعل; وكيل; عميل | **no** | yes | وكلاء، عملاء |
| gay | مثلي; مثلية | yes | yes | مثلي |
| split | قسم; شعب; فرق | yes | yes | قسّم، انقسم |
| tear | شق; مزق; خرق | yes | yes | مزّق، دمعة |
| kids | طفل; جدي | yes | yes | أطفال |
| bus | حافلة; باص; ناقل | yes | yes | حافلة |
| local | محلي; أهلي | yes | yes | محلي |

**ar>en**

| word | the dictionary gives (first 3) | 1st | top 3 | a good translation |
|---|---|---|---|---|
| ببساطة | basically | yes | yes | simply |
| أنهُ | that; to; if | yes | yes | that he |
| آخذ | take; get; blame | yes | yes | taking |
| تلعب | play; game; trick | yes | yes | play |
| مارتن | Martin | yes | yes | Martin |
| ولدي | child; bear; produce | yes | yes | my son |
| الجيد | good; neck; beautiful | yes | yes | the good |
| حفلة | party; gathering; meeting | yes | yes | party |
| ترتدي | wear; move back quickly; put on | yes | yes | wear |
| تحديد | definition; sharpen; confine | yes | yes | determination |
| القوات | power; might; force | yes | yes | the forces |
| كيلي | measure; mete | - | - | (name Kelly, read as a verb) |
| كهذا | like this | yes | yes | like this |
| القيام | rebellion; vertical; right | **no** | **no** | performing |
| النجدة | help; crisis; trouble | yes | yes | help, rescue |
| القيادة | leadership; driving; control | yes | yes | leadership, driving |
| غداً | tomorrow; leave; come | yes | yes | tomorrow |
| نفسي | myself; self; breath | yes | yes | myself |
| إياه | him; it; he | yes | yes | him, it |
| الأمير | prince; commander; Amir | yes | yes | the prince |
| كثيرًا | lot; often; many | yes | yes | a lot |
| فعلت | do; make; activate | yes | yes | I did |
| جلبت | bring; fetch; get | yes | yes | I brought |
| بسيط | simple; wide; easy | yes | yes | simple |
| نوعا | type; kind; diversify | yes | yes | kind (of) |
| فيكتور | -- | - | - | (name, no entry) |
| إليهم | to; automatic; promise | yes | yes | to them |
| مبكراً | early | yes | yes | early |
| كثيرا | lot; often; many | yes | yes | a lot |
| مازالت | -- | **no** | **no** | still |
| أحد | one; someone; somebody | yes | yes | one, someone |
| رأيتك | -- | **no** | **no** | I saw you |
| قتل | kill; murder; slay | yes | yes | kill |
| الأشياء | thing; something; little | yes | yes | the things |
| لهذا | therefore | yes | yes | therefore |
| بأني | part; separate; builder | **no** | **no** | that I |
| ماكس | collect taxes; customs; tax | - | - | (name Max, read as a noun) |
| سقط | fall; down; defective | yes | yes | fell |
| بذلك | whereby | **no** | **no** | thereby |
| قيمة | value; worth; amount | yes | yes | value |
| قطع | cut; stop; break | yes | yes | cut |
| سعيداً | happy; Said; lucky | yes | yes | happy |
| مسبقاً | -- | **no** | **no** | in advance |
| تأخذ | take; get; blame | yes | yes | take |
| فلم | film; movie; motion picture | yes | yes | film |
| لويس | -- | - | - | (name, no entry) |
| البيانات | statement; piano; facts | yes | yes | data, statements |
| منزلي | house; domestic; homemade | yes | yes | my house, domestic |
| أنحاء | side; like; towards | **no** | **no** | parts, regions |
| الآن | now; that; to | yes | yes | now |
| الحي | insist; alive; experience | **no** | yes | the neighbourhood |
| لحظة | moment; instant; jiffy | yes | yes | moment |
| للقيام | -- | **no** | **no** | to do |
| الذهب | go; gold; leave | **no** | yes | gold |
| مهمة | job; mission; task | yes | yes | task, mission |
| فكر | think; thought; thinking | yes | yes | think, thought |
| سيارتك | -- | **no** | **no** | your car |
| دعنا | let's | yes | yes | let us |
| أنتظر | wait; await; wait for | yes | yes | wait |
| تموت | die; death; kill | yes | yes | die |

**fr>ar**

| word | the dictionary gives (first 3) | 1st | top 3 | a good translation |
|---|---|---|---|---|
| mariés | متزوج; متزوجة; تزوج | yes | yes | متزوجون |
| scientifique | عالم; علمي; عالمة | yes | yes | علمي، عالم |
| célèbre | مشهور; شهير; مجيد | yes | yes | مشهور |
| queue | ذنب; ذيل; وقف في الصف | yes | yes | ذيل |
| arthur | آرثر | yes | yes | آرثر |
| pis | و; إذا; ضرع | **no** | **no** | أسوأ |
| plusieurs | كثير; عدة; بعض | **no** | yes | عدة |
| porter | حمل; قل; احتمل | yes | yes | حمل، ارتدى |
| minuit | منتصف الليل; منتصف ليل | yes | yes | منتصف الليل |
| enlevé | أخرج; نقل; أزال | yes | yes | أُزيل، خُطف |
| vengeance | انتقام; ثأر; انتقم | yes | yes | انتقام |
| voyais | أرجوك; رجاء; أعجب | **no** | **no** | كنت أرى |
| balle | كرة; رصاصة; رصاص | yes | yes | كرة، رصاصة |
| affaires | عمل; شركة; مشروع | yes | yes | أعمال |
| style | أسلوب; نمط; موضة | yes | yes | أسلوب |
| retrouvé | وجد; لقي; عثر على | yes | yes | وجد |
| suppose | افترض; حسب; ظن | yes | yes | افترض |
| vérité | حقيقة; حق | yes | yes | حقيقة |
| différente | مختلف; آخر; مشكل | yes | yes | مختلفة |
| king | ملك; سلطان; شاه | - | - | (English word) |
| problème | مشكلة; مسألة; معضلة | yes | yes | مشكلة |
| lucy | -- | - | - | (name, no entry) |
| également | كذلك; أيضا; بالتساوي | yes | yes | أيضا |
| dira | قال; ترى; يقول | yes | yes | سيقول |
| étoile | نجم; كوكب; نجمة | yes | yes | نجمة |
| sauve | أنقذ; نجى; أنجى | yes | yes | ينقذ |
| objets | شيء; عرضة; هدف | yes | yes | أشياء، أغراض |
| secret | خفية; سر; سري | **no** | yes | سرّ |
| écran | شاشة; ستارة; حاجز | yes | yes | شاشة |
| vraiment | حقا; والله; حقيقة | yes | yes | حقا |

**ar>fr**

| word | the dictionary gives (first 3) | 1st | top 3 | a good translation |
|---|---|---|---|---|
| ببساطة | foncièrement; à la base; basiquement | **no** | **no** | simplement |
| أنهُ | que; cela; ce | yes | yes | que |
| آخذ | prendre; prise; emporter | yes | yes | prendre |
| تلعب | jouer; jeu; lire | yes | yes | jouer |
| مارتن | martinet; Martin | **no** | yes | Martin |
| ولدي | enfant; drôle; petit | yes | yes | mon fils |
| الجيد | bon; bien; sage | yes | yes | le bon |
| حفلة | partie; rassemblement; party | **no** | **no** | fête |
| ترتدي | porter; user; usure | yes | yes | porter |
| تحديد | définition; signification | yes | yes | définition |
| القوات | pouvoir; puissance; alimenter | **no** | **no** | les forces |
| كيلي | mesurer; mesure; pige | - | - | (name Kelly) |
| كهذا | -- | **no** | **no** | comme ça |
| القيام | rébellion; révolte; vertical | **no** | **no** | le fait de faire |
| النجدة | aide; aider; secours | yes | yes | aide, secours |
| القيادة | leadership; autorité; charisme | yes | yes | direction, conduite |
| غداً | demain | yes | yes | demain |
| نفسي | moi-même; je; moi | yes | yes | moi-même |
| إياه | lui; le; il | yes | yes | lui |
| الأمير | prince; garde; commandant | yes | yes | prince |
| كثيرًا | souvent; lot; ensemble | yes | yes | beaucoup |
| فعلت | faire; do; aller | yes | yes | faire |
| جلبت | apporter; amener; traîner | yes | yes | apporter |
| بسيط | simple; large; facile | yes | yes | simple |
| نوعا | type; taper; espèce | yes | yes | type |
| فيكتور | -- | - | - | (name, no entry) |
| إليهم | à; pour; chez | yes | yes | à |
| مبكراً | tôt; en avance; de bonne heure | yes | yes | tôt |
| كثيرا | souvent; lot; ensemble | yes | yes | beaucoup |
| مازالت | -- | **no** | **no** | toujours |

</details>

### Arabic correction lists (word translation)

`crates/verba-local/seed/dict-fixes.txt` holds hand-written first candidates for the Arabic words that the Wiktionary data gets wrong. Two rounds, both judged by the maintainer's AI assistant (not an Algerian reviewer): about 140 everyday words, then the 560 commonest words of the Arabic frequency list (mostly small words and words with a pronoun attached, such as `لدي`, `معي`, `عليك`). Before the second round about one in five of those small words had a wrong first candidate in English or French (`يجب` gave "respond", `إذن` "ear", `تم` "swan", `كم` "your"). The fixes put the right word first; the dictionary's own candidates follow. Words further down the list (the 600th word and later) have not been checked one by one. Letters that look alike once folded (`على`/`علي`, `لدى`/`لدي`) share one entry, so a fix for one is a fix for both.

Third round: the words from rank 560 to 1000 of the Arabic list were read the same way (about 400 more words), and about 140 Algerian Darija words written in Arabic letters were added to the same file (`واش`, `كيفاش`, `بزاف`, `قداش` / `ڨداش`, `نقدر`, `راني`, `كاين`, `ماكاينش` ...), because the Wiktionary data has no Darija spellings. The Darija part was written from general knowledge and has **not** been checked by an Algerian: it is marked as such in the file, and the reviewer should read it. Darija written in Latin letters (`wach`, `bezzaf`, `3andi`, `hamdoullah`) is translated from a separate glossary of about 400 spellings (`crates/verba-local/seed/dz-latin-glossary.txt`, into English, French and Arabic). A Latin word is looked up as English or French first, and only a word they do not know is looked up there (a few spellings such as `wesh`, `ana`, `dima` are marked to go first). A spelling that is not listed is still found when it differs only in doubled letters or vowels (`bzaf`, `bezaf`, `bzzaf`), unless two entries would share the same consonants. Like the Arabic-letter Darija above it was written from general knowledge and has not been checked by an Algerian reviewer. Words beyond the thousand commonest of the list have not been checked one by one.

## Sentence translation (Arabic ↔ English)

Select a sentence, press the translate shortcut: `verba-translate.exe` runs an OPUS-MT network (Helsinki-NLP/opus-mt-ar-en or -en-ar, int8, CTranslate2 with oneDNN) on the CPU. Where the models come from: [DATA.md](DATA.md#sentence-translation-models-modelspair).

### Quality

Measured in a one-day spike (2026-11-17) on 20 Arabic sentences (14 Modern Standard Arabic, 6 Algerian Darija) for Arabic → English, and on the first 20 of 50 English sentences for English → Arabic. Every output was read and given 0 (wrong or invented), 1 (understandable, with an error) or 2 (correct and natural) **by the AI agent that ran the spike, not by a bilingual person**. There is no BLEU score and no independent test set, and 14 or 20 sentences are few: read these as "what to expect", not as a measurement of accuracy.

| Direction | Average of 2 | Sentences |
|---|---|---|
| Arabic (MSA) → English | **1.93** | 14 |
| Algerian Darija → English | **0.00** | 6 |
| English → Arabic | 1.60 | 20 |

- **Modern Standard Arabic into English is good.** `هل يمكنك أن ترسل لي الملف قبل الساعة الخامسة؟` gives "Can you send me the file before 5:00?" (2); `الأطفال يلعبون في الحديقة بينما نحضر العشاء` gives "The kids are playing in the park while we're at dinner." (1: "while we prepare dinner" is meant).
- **Algerian Darija does not work, and it fails by inventing words**: `حبيت نروح للدار دروك` became "We liked to go to Darhawk.", `ما فهمتش والو` became "I don't get it. Wallace, please come back.", `واش راك؟ ماكاينش مشكل` became "McCainch is a problem." The model never saw Darija; it reads the words as foreign names. Verba says so in the settings and in the setup, and does not try to hide it: use the word translation (which knows the commonest Darija words) for Darija, not this.
- **English into Arabic** gives correct, readable Modern Standard Arabic for plain sentences (`Please find attached the report you asked for.` gives `أرفق طيه التقرير الذي طلبته.`); idioms come out literally (`Break a leg` gives "break your leg"). The input starts with the model's own language token `>>ara<<`, passed as one piece (through the sentence splitter it would be cut into letters and copied into the answer): the program checks that it never comes back.
- Seen in the test run of the finished program: English → Arabic dropped the final full stop of one sentence and put a tanween on `شكراً`. Small, but the text is not copied as it is: read it.
- Idioms, business phrases and names are the weak points of this size of model in both directions. Check a translation before you send it.

### Base or big model (measured 2026-11-18)

The first spike used the base models (78 MB). The Hub also has bigger ones (`opus-mt-tc-big-*`, 245 MB converted). A second comparison converted both for both directions and ran the same sentences through Verba's own program (beam 4, `>>ara<<` for English into Arabic), 50 Modern Standard Arabic sentences into English (the 14 of the first spike and 36 new ones, 10 of them chat style) and 65 English sentences into Arabic (the 50 of the first spike and 15 gaming and chat sentences, written out the way Verba writes them). Every output was read and scored 0/1/2 by the AI agent that ran it, **not blind, not by a native speaker, with no BLEU**: a difference of 0.1 means "probably a small gain".

| Direction | Model | Size | Average of 2 | Scored 2 / 1 / 0 | 6 / 16 / 28 words |
|---|---|---|---|---|---|
| Arabic → English | base | 78 MB | 1.84 | 42 / 8 / 0 | 72 / 273 / 385 ms |
| Arabic → English | **big (used)** | 245 MB | **1.94** | 47 / 3 / 0 | 152 / 441 / 697 ms |
| English → Arabic | base **(used)** | 78 MB | 1.55 | 39 / 23 / 3 | 87 / 171 / 315 ms |
| English → Arabic | big | 245 MB | 1.65 | 45 / 17 / 3 | 158 / 312 / 772 ms |

(The speeds are the agent's, a busy PC, up to 2x between runs; the figures of the finished program are in the table below.)

- **Arabic → English: the big model.** It was better on 6 sentences, worse on 1, equal on 43. It removes visible flaws of the base model: `&apos;` inside the text (`New Year &apos; s plan`), "I've had a very headache", "credit card" for a bank card ("Do you accept bank card payments?" now), "succeeded in class" for "passed the class". The cost is 245 MB instead of 78 MB, about twice the time, and about 160 MB more memory while loaded.
- **English → Arabic: the base model stays.** The big one averaged 1.70 against 1.58 on the 50 sentences of the first spike, 1.65 against 1.55 on all 65, better on 9 sentences, worse on 5. On the 15 gaming and chat sentences the two tie exactly (1.47). The big one fixes two bad word errors of the base ("peanuts" as "pistachio", "bad play" as "theatre play") and adds two of its own ("lagging" as `متخلفة`, "backward", also an insult; "my mom is calling me" as "will call me"). Three times the size for a small and uneven gain was not taken; the big English → Arabic model is converted and tested (`opus-mt-tc-big-en-ar`, CC-BY-4.0), and switching is one more entry in the `$models` table of `scripts/fetch-models.ps1`.
- **The checkpoint of `opus-mt-tc-big-ar-en` on the Hub's main branch is broken** (garbage in, garbage out); the corrected one is in an unmerged pull request and is pinned (see [DATA.md](DATA.md#sentence-translation-models-modelspair)).
- **Gaming words** are wrong in both models ("carry" as الحمل, "squad" as فرقة "band", "nice shot" as رمية "a throw"): that is the glossary's job, not a bigger model's. **Darija** does not work in either (the preparation step below helps a little).

### Chat abbreviations in a sentence

English chat abbreviations are written out before the translation (`seed/chat-slang.txt`, see [DATA.md](DATA.md#game-and-chat-abbreviations-cratesverba-localseedchat-slangtxt)). One check on the finished program, English into Arabic, with and without it (not a measurement, six sentences, read by the author of the change):

| Sentence | Without | With |
|---|---|---|
| `gg guys, gn, see u tomorrow` | `يا رفاق، أراك غداً` ("gg" and "gn" are lost) | `مباراة جيدة، ليلة سعيدة، أراكم غداً` |
| `brb, my mom is calling me` | `(برب)، أمي تتصل بي` ("brb" copied as sounds) | `سأعود حالاً، أمي تتصل بي` |
| `thx for the carry, that was ez` | `هذا كان من أجل الحمل` | `شكراً على الحمل، كان ذلك سهلاً` ("carry" becomes الحمل, "the load": a game word with an ordinary meaning is not fixed) |

### Spoken Arabic in a sentence (Arabic into English)

`seed/ar-colloquial.txt` replaces spoken words by their standard form before the sentence goes to the network (see [DATA.md](DATA.md#spoken-arabic-made-standard-cratesverba-localseedar-colloquialtxt)). Measured on the finished program, before and after, scored 0/1/2 by the author of the change reading each English output (**the same person wrote the list and the sentences, and is not a native speaker: the numbers are optimistic and are one reader's judgement**):

| Set | Sentences | Average before | Average after |
|---|---|---|---|
| 17 sentences the list was written looking at (Algerian Darija, Levantine, Egyptian) | 17 | 0.59 | 1.65 |
| 15 sentences written afterwards, scored **before** adding the words they showed missing | 15 | 0.47 | 1.40 |

Examples (before, after): `نتا فين راك دروك؟` "Nata Vin Rack Drake?" then "Hey, where are you now?"; `راني تعبان بزاف اليوم` "Rani's tired of faking it today." then "I'm so tired today."; `هاد الطوموبيل غالية بزاف` "Fad al-Tomobil is precious to Zav." then "This car is too expensive."; `يا زلمة شو بدك؟` "Hey, Zhou Buddy?" then "Man, what do you want?"; `شحال عمرك؟` "You're a little old?" then "How old are you?". Still wrong after: `ما نبغيش نروح للسبيطار` "I don't want to go to spitar." (since added to the list), `شو هالحكي؟ ما بفهم شي` "What's wrong with you?" (the meaning is "what is this talk? I don't understand anything"), `غدوة نمشي للخدمة بكري` "Tomorrow I go to service early." (since added: "work"). A word the list does not have is still left to the network, and a sentence with several such words is still translated badly: this is a list of the commonest words, not Darija translation. The person is not recognised either: `ما جيتش` is "I did not come" or "you did not come", and the list takes the first.

### Speed and memory

The finished program, measured on a Ryzen 7 PRO 4750G (8 cores, AVX2), Windows 10, 4 threads, beam 4, median of 10 runs, model already loaded:

| Direction | Words | Time |
|---|---|---|
| Arabic → English (big model) | 6 | 145 ms |
| Arabic → English (big model) | 16 | 460 ms |
| Arabic → English (big model) | 28 | 572 ms |
| English → Arabic | 8 | 88 ms |
| English → Arabic | 22 | 222 ms |
| English → Arabic | 29 | 282 ms |

- **First sentence:** starting the program and loading a model took 0.84 to 0.98 s for the base English → Arabic model and 1.16 s for the big Arabic → English model (files in the disk cache). A model is loaded when a direction is used first; at most two stay in memory; the program ends itself after ten minutes without work.
- **Threads and beam** (measured with the base Arabic → English model, before the big one replaced it): 8 threads gave no gain over 4 (251 ms against 259 ms for 16 words); beam 2 instead of 4 took 221 ms for the same sentence. Verba uses 4 and beam 4, the setting the quality was read at.
- **The OpenMP runtime matters:** the first Rust build, without it, took 295 ms for 16 words and 595 ms for 29 on English → French; with `openmp-runtime-comp` the 28-word sentence takes 325 ms. The Python build of CTranslate2 is still about 1.5 to 2 times faster (135 ms for the 16-word Arabic sentence); the cause is not found.
- **Longer text** is cut into sentences first (and a sentence of more than 60 words at a comma), and the sentences go to the model as one batch.
- **Not measured:** other CPUs (a laptop with half the speed would take twice these times), the memory of the Rust program (the Python process peaked at 140 to 149 MB with one model, interpreter included, so expect about 90 MB for a base model; the big model took 252 MB in the same measurement), loading from a cold disk, and the popup on screens other than the author's.

## Wrong keyboard (Ctrl+Alt+K)

What it was measured on: this PC's own keyboards (English US, French AZERTY, Arabic 102 AZERTY, read from Windows) and the real word lists. Arabic sentences are made of words of the Arabic list, "typed" with the keys of the Arabic layout on each of the two other keyboards (as the text would have come out), and given to the same function the shortcut uses. No key is sent and no window is touched.

```
cargo test -p verba-app fixkeys -- --ignored --nocapture
```

| Text | Cases | Offered first | Offered among the readings |
|---|---|---|---|
| Words of the list, 1 to 20 words (7 lengths, 300 sentences each, both keyboards) | 4200 | 100 % | 100 % |
| 18 hand-written sentences (Arabic, French, English) on every wrong keyboard | 18 | 100 % | 100 % |
| 3 sentences in Algerian Darija with Arabic letters, on both keyboards | 6 | 100 % | 100 % |
| One word that the lists lack (random Arabic letters), the keyboard it was typed on still active | 600 | 98.7 % | 98.7 % |
| Two such words | 600 | 98.8 % | 98.8 % |
| One such word among two or five real words (typed keyboard active) | 600 + 600 | 99.3 % and 100 % | 100 % |
| Two such words among four real ones (typed keyboard active) | 600 | 99.7 % | 100 % |
| The same cases, but the Arabic keyboard is the active one | 600 each | 85 % (one word), 91 % (two), 98 to 100 % (with real words) | 85 % to 100 % |
| Text that is **right** (English and French sentences from the lists, 1 to 12 words) | 2400 | 0.5 % of the one-word texts (words of three letters) are offered a change; nothing else | |

How it decides, and what it learned from the failures:

- It traces each character back to its key and writes what the same key gives on the other layout; every pair of layouts is tried.
- A reading needs **evidence of its own**: letters in real words, at least three for Arabic and four for English and French (words of two letters never count there: the lists hold strings like `og`, `lf` and `ho` that wrong letters make by chance). Letters in a word that has something stuck inside it that its language does not have (`]`, `$`, a digit or a Latin letter in the middle of an Arabic word) are not evidence, and that symbol counts as one more character that is not a word.
- A text is taken as right only with the same evidence, so `tor vq\e]q` (a real English word `tor` by chance) is not "right".
- Of two readings that are equal, the layout the text is easiest to have typed on comes first (an English text has `[`, `]`, `;` that the French layout gives only with AltGr or not at all; a French text has `ù`, `é` that the English layout lacks), then the keyboard that was active, then the order of Windows. A reading from another layout than the active one needs two more letters of evidence.
- When nothing is made of real words and the text is not either (a name), the Arabic reading is given anyway, from the layout that explains the text with the fewest odd keys.

Known weaknesses, measured:

- **Letters only, no word of the lists:** English and French share most keys, so a text of unknown words with no symbol and no accent can be read from the wrong one of the two when the Arabic keyboard is the active one (85 % to 91 % right, above). With the typed keyboard active it is 98 to 99 %.
- **A rare word of the lists that wrong letters spell by chance** (`shia`, `imf`, `heya`) makes a short text look right: nothing is offered for it (about 1 case in 100 of the texts of one or two unknown words above).
- **A text that is only symbols** (an Arabic word typed entirely on punctuation keys) has no letter: nothing is offered.
- **Words of three letters typed right** (`cat`) have too little evidence to count as right, so a one-word selection of this kind is offered an Arabic reading (0.5 % of the one-word texts above).
- Not measured: other keyboards than these three, the dead keys of the French layout, Arabic text with diacritics, text of several scripts mixed, the long texts in real apps (the typing over the selection).

## Size of the lists

Does a longer word list help the suggestions? Measured on real text, not on the lists' own words (the harness above uses those). Reproduce:

```
# the full lists (FrequencyWords _full.txt) cut to N lines, and the text
cargo run --release -p verba-local --example realtext_accuracy -- --dir <folder with en.txt fr.txt ar.txt> --max-words N --lang ar --tsv <ara_sentences.tsv> [--typos --base-dir <folder>]
```

- **Text:** Tatoeba sentences (20 000 of the 2 million English ones, the 69 000 Arabic ones; short everyday sentences), and the introductions of random Wikipedia articles (1 351 English and 980 Arabic sentences; written, formal language; the sample is small because the site limited the requests).
- **Method:** every word of 4 letters or more is typed letter by letter after the words before it (the last 48 characters, as the program sees them), 3 suggestions asked after each letter; names (a capital inside a sentence), words with an apostrophe, hyphen or digit, and other scripts are left out. Lists only (no next-word table), so nothing built from the same sentences flatters the numbers.

| Text | List | Words the list has | Hit within 3 letters | Letters saved |
|---|---|---|---|---|
| Arabic, Tatoeba | 50 000 (44 401 after cleaning) | 82.2 % | 50.3 % | 28.6 % |
| | 100 000 | 87.4 % | 51.1 % | 29.7 % |
| | **150 000** | 90.2 % | 51.2 % | 30.0 % |
| | 300 000 | 92.6 % | 51.4 % | 30.5 % |
| Arabic, Wikipedia | 50 000 | 70.4 % | 31.4 % | 24.4 % |
| | 100 000 | 79.9 % | 32.3 % | 26.3 % |
| | **150 000** | 83.6 % | 32.5 % | 27.0 % |
| | 300 000 | 87.9 % | 32.6 % | 27.9 % |
| English, Tatoeba | 50 000 (46 257) | 98.2 % | 74.4 % | 45.7 % |
| | **100 000** | 98.9 % | 74.4 % | 46.0 % |
| | 300 000 | 99.2 % | 74.4 % | 46.2 % |
| English, Wikipedia | 50 000 | 93.4 % | 47.8 % | 39.4 % |
| | **100 000** | 95.7 % | 47.9 % | 40.5 % |
| | 300 000 | 96.8 % | 47.9 % | 40.9 % |

- **What a longer list gives:** more of the rare words, mostly after 4 letters; the hits within 2 and 3 letters hardly move (the frequent words win those places either way). Arabic gains most: +10 points of covered words on formal text at 100 000, +13 at 150 000, with 1.9 to 2.6 points of letters saved on formal text (1.1 to 1.4 on everyday sentences). English gains little: 1.1 points of letters saved on formal text at 100 000 and 0.3 on everyday sentences. **Shipped: English 100 000, Arabic 150 000, French 50 000 (not measured).** More than that gains less each time and costs more.
- **Cost** (the three languages at the same size): memory 11 MB at 50 000, 21 MB at 100 000, 31 MB at 150 000, 59 MB at 300 000; loading 0.26 s, 0.5 s, 0.8 s, 1.7 s; one suggestion 6 to 25 µs at 50 000 and 10 to 160 µs at 100 000 to 150 000 (the old harness: p99 137 µs before, 674 µs after, worst 1.4 ms).
- **The spelling correction is unchanged.** A longer list would hide more typos (a typo that is a rare word of the list is left alone: with the whole list, the share of typos that are a word went from 4.5 % to 10.3 % in Arabic and from 1.6 % to 5.3 % in English at 300 000). So the spelling keeps the dictionary it always had, the first 50 000 lines of the list (`WordList::spell_min_count`). On the same 8 240 Arabic and 7 810 English typos, the correction offered, the right word first and among three are the same to a tenth of a point at 50 000, 100 000, 150 000, 200 000 and 300 000.
- **The tail does not fill places with junk.** A word beyond the first 50 000 lines is offered only when its frequency is at least 1/200 of the best word offered (`TAIL_FLOOR`): without it, `eit` offered `either`, `eitan`, `eitaro`. Values of 1/50, 1/200, 1/1000 and none give the same hit rates to 0.1 point; a strict 1/1 loses about one point in Arabic. With the floor, 16 of the 2 267 answers of the "answers as before" check (`golden_real.txt`) changed (0 in English alone, 11 in Arabic, 3 first suggestions), and the check was written again.
- **The old harness** (3 000 commonest words of each list) gives the same numbers as before to a tenth of a point, except Arabic top-1 after 3 and 4 letters (+0.3 and +0.7).
- **Not measured:** French; text written as messages or in Algerian Darija (the texts above are Tatoeba and Wikipedia); the effect on the next-word suggestions (their tables are separate: 10 467 Arabic pairs, 29 993 English); typos made by people (the ones above are made by a program: a letter left out, swapped, doubled or replaced).

## Words written for Verba

`seed/ar-common.txt` (see [DATA.md](DATA.md#common-modern-standard-arabic-words-written-for-verba-cratesverba-localseedar-commontxt)): 1 392 lines of common Modern Standard Arabic words with English translations, written from general knowledge, checked against what the program already had:

```
cargo run --release -p verba-local --example seed_coverage -- --show 500
```

- **The dictionary already had most of them.** The lookup (particles and pronoun endings stripped, as in the program) found nothing for **73 lines (5.2 %)**: those add a headword (`كلمة المرور`, `تسجيل الدخول`, `الذكاء الاصطناعي`, `نظام التشغيل`, plurals such as `مستخدمون`). With the corrections below, for 82.0 % the translations share a word with the dictionary's (plural and tense are compared by a rough stem: `accounts` and `account` agree); for 12.7 % they share none, which is mostly a different inflection written here (`قال` said, the dictionary says), and sometimes the dictionary's own noise.
- **Wiktionary's data has real noise in the first translation of common words**, found by reading the disagreements: `قروض` die; gnaw; loan, `كوارث` heir; successor, `لولا` Lula, `قرآن` conjunction; century, `بنى` brown, `سلطات` salad, `أسنان` mold; sharpen, `علوم` know. About 80 of them were put right in `dict-fixes.txt` (the corrected word goes in front; the rest stays). The lines whose first translation is none of the written ones went from 231 to 188, and the lines that share nothing from 206 to 177.
- **The suggestions gain almost nothing:** the Arabic list (150 000 lines) already had all but **16 of the 1 308 one-word headwords (1.2 %)**. Measured on the same real Arabic text as above (Tatoeba, Wikipedia): the same 30.0 % and 27.0 % of letters saved with and without them. The words written here are not where the Arabic list is short: it is short of rare words (see the sizes above).
- **Not measured:** that the 1 392 lines are right (they were written, not verified by a native speaker, and the dictionary's rare senses are not all wrong: a line that agrees with the dictionary only confirms one sense); the effect on a sentence (the sentence translator is a neural model and does not read the dictionary).

## Arabic next-word with Wikipedia text

The next-word table of Arabic was built from Tatoeba only (69 000 short everyday sentences, 0.3 million words). Measured on sentences it was NOT built from, with the tool of the next-word section (`--train` can be given several files, `file@6` counts each sentence of a file 6 times, `--eval` names the file whose held-out sentences are the questions):

```
cargo run --release -p verba-local --example nextword_accuracy -- --lang ar --train ara_sentences.tsv@6 --train ara_wikipedia_100K.tsv --eval ara_wikipedia_100K.tsv
```

The questions come from three kinds of text: **Wikipedia** (written Modern Standard Arabic: 2 504 held-out sentences, 33 017 positions, near copies of the training part dropped), **Tatoeba** (1 773 sentences, 5 383 positions) and **everyday messages written by hand** (217 positions, the set the tool always uses). "Top-3" is the share of positions where the next word is among the 3 offered, counting a position where nothing is offered as a miss.

| The table is built from | Wikipedia top-1 / top-3 | Tatoeba top-1 / top-3 | Everyday top-1 / top-3 |
|---|---|---|---|
| nothing (always the 3 commonest words of the training text) | 4.1 % / 8.4 % (Tatoeba's) and 4.1 % / 9.0 % (Tatoeba's and Wikipedia's) | 3.0 % / 7.0 % | |
| **Tatoeba (what was shipped until 0.10.5)** | **3.4 % / 5.2 %** | 9.2 % / 14.8 % | 18.0 % / 29.0 % |
| Wikipedia alone | 7.1 % / 10.7 % | | 8.8 % / 12.4 % |
| Tatoeba x1 + Wikipedia | 7.3 % / 11.1 % | 7.8 % / 12.8 % | 13.8 % / 23.0 % |
| Tatoeba x3 + Wikipedia | 7.3 % / 11.1 % | 9.2 % / 14.6 % | 17.1 % / 27.2 % |
| **Tatoeba x6 + Wikipedia (shipped from 0.10.6)** | **6.9 % / 10.7 %** | **9.7 % / 15.4 %** | **18.0 % / 28.6 %** |
| Tatoeba x12 + Wikipedia | 6.6 % / 10.2 % | 9.9 % / 15.7 % | 18.9 % / 30.4 % |

- **The Tatoeba table did not suit written Arabic:** on Wikipedia sentences it was worse than the dumb rule (5.2 % against 8.4 % for the 3 commonest words of the same Tatoeba text). With Wikipedia text added the right word is among the 3 offered 10.7 % of the time: twice as often.
- **Without a weight, Wikipedia drowns the everyday phrases** (everyday 29.0 % down to 23.0 %): its 1.5 million words outweigh the 0.3 million of Tatoeba. Counting each Tatoeba sentence 6 times keeps the everyday and Tatoeba numbers where they were (28.6 % against 29.0 %, 15.4 % against 14.8 %) and keeps nearly all of the gain on Wikipedia (10.7 % of 11.1 %). 6 was chosen from the sweep above (1, 3, 6, 12); 12 gives a little more everyday and a little less Wikipedia.
- **Cost:** the table goes from 534 KB (10 446 contexts) to 1 433 KB (22 663 contexts), as big as the English one. Memory and load time of this table were not measured (the 889 KB table of the 1 + 1 experiment took 2.75 MB and 11 ms to load).
- **Limits:** the Wikipedia questions are encyclopedic text, not letters or chat; the weights were chosen on the same three sets they are reported on (no fourth set was kept apart); French and English are unchanged; the license of the Leipzig collection is an open point (DATA.md).
