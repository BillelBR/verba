"""Words error rate and characters error rate of a speech-recognition output, for docs/ACCURACY.md.

usage: python wer.py <reference.tsv> <hypotheses.tsv> <language ar|fr|en> [--show N]
reference.tsv: file<TAB>reference; hypotheses.tsv: file<TAB>hypothesis<TAB>seconds (what `whisper_eval` writes).

Both sides are normalised the same way (lower case, no punctuation, no Arabic vowel marks, alef / ya / ta marbuta / hamza written one way
for the "lenient" figure, since a dictation that writes `إلى` for `الى` is not wrong in a way anybody sees). Prints strict and lenient WER and CER.
"""
import re
import sys
import unicodedata

MARKS = re.compile("[ً-ٰٟـ]")


def normal(text, lang, lenient):
    text = unicodedata.normalize("NFKC", text).lower()
    text = MARKS.sub("", text)
    text = re.sub(r"[^\w\s]", " ", text)
    if lenient and lang == "ar":
        text = re.sub("[أإآٱ]", "ا", text).replace("ى", "ي").replace("ة", "ه").replace("ؤ", "و").replace("ئ", "ي").replace("ء", "")
    if lenient and lang == "fr":
        text = unicodedata.normalize("NFD", text)
        text = "".join(c for c in text if unicodedata.category(c) != "Mn")
    return text.split()


def edit_distance(a, b):
    prev = list(range(len(b) + 1))
    for i, x in enumerate(a, 1):
        cur = [i]
        for j, y in enumerate(b, 1):
            cur.append(min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (x != y)))
        prev = cur
    return prev[-1]


def main():
    ref_path, hyp_path, lang = sys.argv[1:4]
    show = int(sys.argv[sys.argv.index("--show") + 1]) if "--show" in sys.argv else 0
    ref = dict(line.rstrip("\n").split("\t")[:2] for line in open(ref_path, encoding="utf-8") if "\t" in line)
    hyp = {}
    for line in open(hyp_path, encoding="utf-8"):
        f = line.rstrip("\n").split("\t")
        hyp[f[0]] = f[1] if len(f) > 1 else ""
    for lenient in (False, True):
        w_err = w_len = c_err = c_len = 0
        for name, r in ref.items():
            if name not in hyp:
                continue
            rw, hw = normal(r, lang, lenient), normal(hyp[name], lang, lenient)
            w_err += edit_distance(rw, hw)
            w_len += len(rw)
            rc, hc = list("".join(rw)), list("".join(hw))
            c_err += edit_distance(rc, hc)
            c_len += len(rc)
        print(f"{'lenient' if lenient else 'strict '}: WER {100 * w_err / max(w_len, 1):5.1f} %   CER {100 * c_err / max(c_len, 1):5.1f} %   ({w_len} words, {c_len} letters)")
    if show:
        for name in list(ref)[:show]:
            print("REF:", ref[name])
            print("HYP:", hyp.get(name, ""))
            print()


main()
