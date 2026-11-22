//! Measures next-word prediction on sentences the tables were NOT built from.
//!
//!   powershell -ExecutionPolicy Bypass -File scripts\fetch-bigrams.ps1      # once: downloads the sentences, builds the tables
//!   cargo run --release -p verba-local --example nextword_accuracy -- --lang en [--sweep] [--tsv path]
//!   ... --lang ar --train a.tsv --train b.tsv --eval b.tsv      (train on the sentences of several files, measure on the held-out ones of one)
//!
//! Method: the corpus (Tatoeba sentences) is split by sentence id: 1 in 40 sentences is held out. A table is built from the rest
//! with exactly the code that builds the shipped tables, then every position of every held-out sentence is a question: the words before
//! it are the context, the next word is the answer. A hit means the answer is among the 1 (top-1) or 3 (top-3) words offered.
//! Held-out sentences that also are in the training part (same words) are dropped, and so are those that share most of their word
//! triples with it (Tatoeba holds many near-copies: `Tom ate an apple` / `Mary ate an apple`), which would flatter the numbers.
//! A second, small set of sentences written by hand as everyday messages (not translation exercises) checks that the result is not
//! only a trait of the corpus.
//!
//! The baseline is the strongest dumb rule: always offer the 3 most frequent words of the training part.
//! "Overall" counts a position where nothing is offered as a miss (what the user would see); "when shown" counts only the positions
//! where something is offered.

use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use verba_core::Lang;
use verba_local::{BuildParams, LocalEngine, NextBuilder, NextParams, NextTable, WordList, word_runs};

/// Counts the bytes that are alive, to report the memory of a table.
struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        LIVE.fetch_add(l.size(), Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size(), Ordering::Relaxed);
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        LIVE.fetch_add(n, Ordering::Relaxed);
        LIVE.fetch_sub(l.size(), Ordering::Relaxed);
        unsafe { System.realloc(p, l, n) }
    }
}
#[global_allocator]
static A: Counting = Counting;

fn hash(s: &str) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

#[derive(Default, Clone, Copy)]
struct Score {
    n: usize,
    shown: usize,
    top1: usize,
    top3: usize,
    top1_when_shown: usize,
    top3_when_shown: usize,
}

impl Score {
    fn add(&mut self, offered: &[String], gold: &str) {
        self.n += 1;
        let top1 = offered.first().is_some_and(|w| w == gold);
        let top3 = offered.iter().take(3).any(|w| w == gold);
        if !offered.is_empty() {
            self.shown += 1;
            self.top1_when_shown += usize::from(top1);
            self.top3_when_shown += usize::from(top3);
        }
        self.top1 += usize::from(top1);
        self.top3 += usize::from(top3);
    }

    fn line(&self, name: &str) -> String {
        let p = |a: usize, b: usize| if b == 0 { 0.0 } else { 100.0 * a as f64 / b as f64 };
        format!(
            "{name:<38} shown {:5.1}% | overall top-1 {:5.1}%  top-3 {:5.1}% | when shown top-1 {:5.1}%  top-3 {:5.1}%",
            p(self.shown, self.n),
            p(self.top1, self.n),
            p(self.top3, self.n),
            p(self.top1_when_shown, self.shown),
            p(self.top3_when_shown, self.shown)
        )
    }
}

/// Asks `predict(word two back, word before)` at every position of the held-out runs.
fn evaluate(runs: &[Vec<String>], mut predict: impl FnMut(Option<&str>, &str) -> Vec<String>) -> Score {
    let mut s = Score::default();
    for run in runs {
        for i in 1..run.len() {
            let w1 = if i >= 2 { Some(run[i - 2].as_str()) } else { None };
            s.add(&predict(w1, &run[i - 1]), &run[i]);
        }
    }
    s
}

fn table_predictor<'a>(t: &'a NextTable, p: &'a NextParams) -> impl FnMut(Option<&str>, &str) -> Vec<String> + 'a {
    move |w1, w2| t.predict(w1, w2, p).into_iter().take(3).map(|x| x.0.to_string()).collect()
}

/// Everyday messages, written by hand for this measurement (not from the corpus).
fn everyday(lang: Lang) -> Vec<&'static str> {
    if lang == Lang::En {
        vec![
            "Thanks for your email, I will get back to you as soon as possible",
            "Hi everyone, I just wanted to let you know that the meeting has been moved to Thursday",
            "Can you please send me the report before the end of the day",
            "I would like to ask you a question about the project",
            "Let me know if you have any questions or need more information",
            "I am going to be late for the meeting because of the traffic",
            "What time do you want to meet tomorrow",
            "Sorry for the late reply, I was out of the office last week",
            "We are looking forward to working with you on this project",
            "Please find attached the document that you asked for",
            "I think that we should talk about it before we make a decision",
            "Do you want to grab some lunch with us today",
            "I have been trying to reach you all morning",
            "It was great to see you at the party last night",
            "Could you tell me how to get to the station from here",
            "I don't know what to do about this problem",
            "He said that he would call me back in a few minutes",
            "The weather is going to be nice this weekend so we can go to the beach",
            "I really appreciate everything that you have done for us",
            "Happy birthday, I hope you have a wonderful day with your family",
            "There is a lot of work to do before the end of the month",
            "I will be there in about ten minutes",
            "Thank you very much for your help with the application",
            "She asked me if I wanted to come with them to the cinema",
            "If you need anything else, please do not hesitate to contact me",
            "We need to find a better way to solve this issue",
            "I have not had time to look at it yet but I will do it today",
            "How was your trip, did you have a good time",
            "Do you know where I can find a good restaurant around here",
            "It is not as easy as it looks",
        ]
    } else if lang == Lang::Fr {
        vec![
            "Merci pour votre message, je reviens vers vous dès que possible",
            "Bonjour à tous, je voulais vous informer que la réunion a été déplacée à jeudi",
            "Pouvez-vous m'envoyer le rapport avant la fin de la journée",
            "Je voudrais vous poser une question à propos du projet",
            "N'hésitez pas à me contacter si vous avez des questions",
            "Je vais être en retard à la réunion à cause de la circulation",
            "À quelle heure voulez-vous qu'on se retrouve demain",
            "Désolé pour ma réponse tardive, j'étais absent la semaine dernière",
            "Nous sommes impatients de travailler avec vous sur ce projet",
            "Veuillez trouver ci-joint le document que vous avez demandé",
            "Je pense qu'on devrait en parler avant de prendre une décision",
            "Tu veux manger avec nous à midi",
            "J'ai essayé de te joindre toute la matinée",
            "C'était super de te voir à la fête hier soir",
            "Pourriez-vous me dire comment aller à la gare d'ici",
            "Je ne sais pas quoi faire avec ce problème",
            "Il a dit qu'il me rappellerait dans quelques minutes",
            "Il va faire beau ce week-end donc on pourra aller à la plage",
            "Je vous remercie pour tout ce que vous avez fait pour nous",
            "Joyeux anniversaire, j'espère que tu passes une belle journée en famille",
            "Il y a beaucoup de travail à faire avant la fin du mois",
            "Je serai là dans une dizaine de minutes",
            "Merci beaucoup pour votre aide avec le dossier",
            "Elle m'a demandé si je voulais venir avec eux au cinéma",
            "Si vous avez besoin d'autre chose, n'hésitez pas à me contacter",
            "Nous devons trouver une meilleure façon de résoudre ce problème",
            "Je n'ai pas encore eu le temps de regarder mais je le ferai aujourd'hui",
            "Comment s'est passé ton voyage, tu as passé un bon moment",
            "Savez-vous où je peux trouver un bon restaurant par ici",
            "Ce n'est pas aussi facile que ça en a l'air",
        ]
    } else if lang == Lang::Ar {
        vec![
            "شكرا لك على رسالتك سأعود إليك في أقرب وقت ممكن",
            "مرحبا بالجميع أردت أن أخبركم أن الاجتماع تم نقله إلى يوم الخميس",
            "هل يمكنك أن ترسل لي التقرير قبل نهاية اليوم",
            "أريد أن أسألك سؤالا عن المشروع",
            "أخبرني إذا كان لديك أي أسئلة أو تحتاج إلى مزيد من المعلومات",
            "سأتأخر عن الاجتماع بسبب الازدحام",
            "في أي ساعة تريد أن نلتقي غدا",
            "آسف على التأخر في الرد كنت خارج المكتب الأسبوع الماضي",
            "نحن نتطلع إلى العمل معكم في هذا المشروع",
            "أرفقت لكم الوثيقة التي طلبتموها",
            "أعتقد أنه يجب أن نتحدث عن ذلك قبل أن نتخذ قرارا",
            "هل تريد أن تتناول الغداء معنا اليوم",
            "حاولت الاتصال بك طوال الصباح",
            "كان من الرائع أن أراك في الحفلة ليلة أمس",
            "هل يمكن أن تخبرني كيف أصل إلى المحطة من هنا",
            "لا أعرف ماذا أفعل في هذه المشكلة",
            "قال إنه سيتصل بي بعد بضع دقائق",
            "سيكون الطقس جميلا في نهاية الأسبوع لذلك يمكننا الذهاب إلى الشاطئ",
            "أقدر كل ما فعلته من أجلنا",
            "عيد ميلاد سعيد أتمنى لك يوما رائعا مع عائلتك",
            "هناك الكثير من العمل قبل نهاية الشهر",
            "سأكون هناك بعد حوالي عشر دقائق",
            "شكرا جزيلا لك على مساعدتك في الطلب",
            "سألتني إذا كنت أريد أن أذهب معهم إلى السينما",
            "إذا كنت تحتاج إلى أي شيء آخر فلا تتردد في الاتصال بي",
            "يجب أن نجد طريقة أفضل لحل هذه المشكلة",
            "لم يكن لدي وقت لأنظر في الأمر لكنني سأفعل ذلك اليوم",
            "كيف كانت رحلتك هل قضيت وقتا ممتعا",
            "هل تعرف أين يمكنني أن أجد مطعما جيدا هنا",
            "ليس الأمر سهلا كما يبدو",
        ]
    } else {
        Vec::new()
    }
}

struct Args {
    lang: Lang,
    /// The files the table is built from (all but the held-out sentences), and the one whose held-out sentences are the questions.
    train: Vec<(PathBuf, usize)>,
    eval: PathBuf,
    words: PathBuf,
    sweep: bool,
}

fn args() -> Args {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/words");
    let mut lang = Lang::En;
    let (mut tsv, mut sweep) = (None, false);
    let (mut train, mut eval): (Vec<(PathBuf, usize)>, Option<PathBuf>) = (Vec::new(), None);
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--lang" => {
                lang = match it.next().as_deref() {
                    Some("fr") => Lang::Fr,
                    Some("ar") => Lang::Ar,
                    _ => Lang::En,
                }
            }
            "--tsv" => tsv = it.next().map(PathBuf::from),
            // a file, and how many times its sentences count when they are the weighted kind: `--train everyday.tsv@4`
            "--train" => {
                if let Some(spec) = it.next() {
                    match spec.rsplit_once('@').and_then(|(p, w)| w.parse::<usize>().ok().map(|w| (p.to_string(), w))) {
                        Some((p, w)) => train.push((PathBuf::from(p), w)),
                        None => train.push((PathBuf::from(spec), 1)),
                    }
                }
            }
            "--eval" => eval = it.next().map(PathBuf::from),
            "--sweep" => sweep = true,
            _ => {}
        }
    }
    let name = if lang == Lang::Fr {
        "fra"
    } else if lang == Lang::Ar {
        "ara"
    } else {
        "eng"
    };
    // without --train and --eval: the one corpus (Tatoeba, or --tsv) is both
    let default = tsv.unwrap_or_else(|| root.join(format!("src/{name}_sentences.tsv")));
    if train.is_empty() {
        train.push((eval.clone().unwrap_or_else(|| default.clone()), 1));
    }
    let eval = eval.unwrap_or_else(|| train[0].0.clone());
    Args { lang, train, eval, words: root, sweep }.with_sweep(sweep)
}

impl Args {
    fn with_sweep(mut self, sweep: bool) -> Self {
        self.sweep = sweep;
        self
    }
}

fn is_held_out(id: &str) -> Option<bool> {
    id.parse::<u64>().ok().map(|id| hash(&id.to_string()) % 40 == 0)
}

fn main() {
    let a = args();
    let list_text = std::fs::read_to_string(a.words.join(format!("{}.txt", a.lang.code()))).expect("run scripts/fetch-data.ps1 first");
    let reference = WordList::parse_for(a.lang, &list_text, 50_000);
    let corpora: Vec<(String, usize)> = a.train.iter().map(|(p, w)| (std::fs::read_to_string(p).unwrap_or_else(|_| panic!("{} is missing: run scripts/fetch-bigrams.ps1 first", p.display())), *w)).collect();
    let eval_corpus = std::fs::read_to_string(&a.eval).unwrap_or_else(|_| panic!("{} is missing", a.eval.display()));

    let t0 = Instant::now();
    let mut builder = NextBuilder::new(a.lang);
    let mut held: Vec<&str> = Vec::new();
    let mut train: Vec<(&str, usize)> = Vec::new();
    // the table is built from every sentence of the training files that is not held out; the questions are the held-out sentences of one file
    for (corpus, weight) in &corpora {
        for line in corpus.lines() {
            let mut f = line.splitn(3, '\t');
            let (Some(id), Some(second)) = (f.next(), f.next()) else { continue };
            let text = f.next().unwrap_or(second);
            if is_held_out(id) == Some(false) {
                train.push((text, *weight));
            }
        }
    }
    for line in eval_corpus.lines() {
        let mut f = line.splitn(3, '\t');
        let (Some(id), Some(second)) = (f.next(), f.next()) else { continue };
        let text = f.next().unwrap_or(second);
        if is_held_out(id) == Some(true) {
            held.push(text);
        }
    }
    let mut train_sentences = 0usize;
    for (text, weight) in &train {
        train_sentences += usize::from(builder.add_sentence_times(text, *weight));
    }
    println!(
        "{}: {} training sentences ({} distinct), {} words; built in {:.1}s",
        a.lang.code(),
        train.len(),
        train_sentences,
        builder.token_count(),
        t0.elapsed().as_secs_f32()
    );

    // held-out runs, without exact copies and near copies of the training part
    let mut triples: HashSet<u64> = HashSet::new();
    for (text, _) in &train {
        for run in word_runs(a.lang, text) {
            for w in run.windows(3) {
                triples.insert(hash(&w.join(" ")));
            }
        }
    }
    let (mut kept, mut loose, mut dropped_exact, mut dropped_near) = (Vec::new(), Vec::new(), 0, 0);
    for text in &held {
        if builder.has_sentence(text) {
            dropped_exact += 1;
            continue;
        }
        let runs = word_runs(a.lang, text);
        loose.extend(runs.iter().cloned());
        let (mut total, mut seen) = (0usize, 0usize);
        for run in &runs {
            for w in run.windows(3) {
                total += 1;
                seen += usize::from(triples.contains(&hash(&w.join(" "))));
            }
        }
        if total >= 2 && seen * 10 >= total * 6 {
            dropped_near += 1;
            continue;
        }
        kept.extend(runs);
    }
    drop(triples);
    println!("held out: {} sentences; {} exact copies and {} near copies dropped", held.len(), dropped_exact, dropped_near);
    let hand_runs: Vec<Vec<String>> = everyday(a.lang).iter().flat_map(|s| word_runs(a.lang, s)).collect();

    // baseline: the 3 most frequent words of the training part
    let top: Vec<String> = builder.top_allowed(&reference, &BuildParams::for_lang(a.lang), 3);
    println!("baseline words: {top:?}");

    let configs: Vec<(String, BuildParams, NextParams)> = if a.sweep {
        let base = BuildParams::for_lang(a.lang);
        let np = NextParams::default();
        let bi = |min_prob: f64| NextParams { min_prob, trigram: false, ..np.clone() };
        let mut v = vec![
            ("bigram, floor 0".to_string(), base.clone(), bi(0.0)),
            ("open vocab (ratio 8)".to_string(), BuildParams { need_in_reference: false, ..base.clone() }, np.clone()),
            ("open vocab (ratio 8), min word 2".to_string(), BuildParams { need_in_reference: false, min_word_count: 2, ..base.clone() }, np.clone()),
            ("open vocab, min word 2, min count 1".to_string(), BuildParams { need_in_reference: false, min_word_count: 2, min_count: 1, ..base.clone() }, np.clone()),
            ("open vocab, ratio 30, min word 3".to_string(), BuildParams { need_in_reference: false, corpus_ratio: 30.0, min_word_count: 3, ..base.clone() }, np.clone()),
            ("pairs 20k + 10k ctx".to_string(), BuildParams { contexts: 10_000, pair_contexts: 20_000, pair_min_total: 15, ..base.clone() }, np.clone()),
            ("pairs 20k + 20k ctx".to_string(), BuildParams { contexts: 20_000, pair_contexts: 20_000, pair_min_total: 15, ..base.clone() }, np.clone()),
            ("pairs 20k + 10k ctx, 6 per".to_string(), BuildParams { contexts: 10_000, per_context: 6, pair_contexts: 20_000, pair_min_total: 15, ..base.clone() }, np.clone()),
            ("bigram, floor .02".to_string(), base.clone(), bi(0.02)),
            ("bigram, floor .05".to_string(), base.clone(), bi(0.05)),
            ("bigram, floor .10".to_string(), base.clone(), bi(0.10)),
            ("bigram, 10k contexts".to_string(), BuildParams { contexts: 10_000, ..base.clone() }, bi(0.02)),
            ("bigram, 2k contexts".to_string(), BuildParams { contexts: 2_000, ..base.clone() }, bi(0.02)),
            ("bigram, 5 followers".to_string(), BuildParams { per_context: 5, ..base.clone() }, bi(0.02)),
            ("bigram, min count 1".to_string(), BuildParams { min_count: 1, ..base.clone() }, bi(0.02)),
            ("bigram, no corpus filter".to_string(), BuildParams { corpus_ratio: 1e9, need_in_reference: false, ..base.clone() }, bi(0.02)),
        ];
        for (ctx, min_total) in [(5_000, 30), (20_000, 15), (50_000, 8)] {
            let b = BuildParams { pair_contexts: ctx, pair_min_total: min_total, ..base.clone() };
            for k in [5.0, 20.0, 60.0] {
                v.push((format!("+pairs {ctx} (min {min_total}), k={k}"), b.clone(), NextParams { tri_k: k, ..np.clone() }));
            }
        }
        v
    } else {
        vec![("shipped settings".to_string(), BuildParams::for_lang(a.lang), NextParams::default())]
    };

    let positions = |r: &[Vec<String>]| r.iter().map(|r| r.len().saturating_sub(1)).sum::<usize>();
    println!("\n{} held-out positions, {} hand-written positions", positions(&kept), positions(&hand_runs));
    println!("({} positions in the loose set: copies of the training part kept, optimistic)", positions(&loose));
    println!("{}", evaluate(&loose, |_, _| top.clone()).line("BASELINE 3 most frequent (loose)"));
    println!("{}", evaluate(&kept, |_, _| top.clone()).line("BASELINE 3 most frequent (novel)"));
    println!("{}", evaluate(&hand_runs, |_, _| top.clone()).line("BASELINE 3 most frequent (hand)"));
    let mut shipped: Option<NextTable> = None;
    for (name, bp, np) in configs {
        let t = builder.build(&reference, &bp);
        let text = t.to_text("x");
        println!(
            "{}   [{} contexts, {:.0} KB]",
            evaluate(&kept, table_predictor(&t, &np)).line(&format!("{name} (novel)")),
            t.len(),
            text.len() as f64 / 1024.0
        );
        println!("{}", evaluate(&loose, table_predictor(&t, &np)).line(&format!("{name} (loose)")));
        println!("{}", evaluate(&hand_runs, table_predictor(&t, &np)).line(&format!("{name} (hand)")));
        if name == "shipped settings" {
            shipped = Some(t);
        }
    }

    // size, load time, memory and speed of the shipped table, through the engine
    if let Some(t) = shipped {
        let text = t.to_text("x");
        let before = LIVE.load(Ordering::Relaxed);
        let t0 = Instant::now();
        let loaded = NextTable::parse(&text);
        let load = t0.elapsed();
        let mem = LIVE.load(Ordering::Relaxed) as isize - before as isize;
        println!(
            "\ntable: {} contexts, {:.0} KB on disk, parsed in {:.1} ms, about {:.2} MB in memory",
            loaded.len(),
            text.len() as f64 / 1024.0,
            load.as_secs_f64() * 1000.0,
            mem as f64 / 1048576.0
        );
        drop(loaded);
        let mut engine = LocalEngine::new();
        engine.add(a.lang, reference);
        engine.load_next_words(a.lang, &text);
        let mut texts: Vec<String> = Vec::new();
        for run in kept.iter().take(20_000) {
            for i in 1..run.len() {
                texts.push(format!("{} ", run[..i].join(" ")));
            }
        }
        let mut times: Vec<u128> = Vec::with_capacity(texts.len());
        let mut answered = 0usize;
        for text in &texts {
            let t0 = Instant::now();
            let r = engine.next_words(Some(a.lang), text, 3);
            times.push(t0.elapsed().as_nanos());
            answered += usize::from(!r.is_empty());
        }
        times.sort_unstable();
        let q = |p: f64| times[((times.len() - 1) as f64 * p) as usize] as f64 / 1000.0;
        println!(
            "next_words: {} calls ({} answered), p50 {:.1} us, p95 {:.1} us, p99 {:.1} us, max {:.1} us",
            times.len(),
            answered,
            q(0.5),
            q(0.95),
            q(0.99),
            q(1.0)
        );
    }
}
