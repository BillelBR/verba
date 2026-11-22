"""Takes the first N utterances of the Algerian test split of Casablanca (UBC-NLP/Casablanca, CC BY-NC-ND 4.0, spoken Arabic of several countries)
as 16 kHz mono WAV files and a `file<TAB>reference` list, for measuring dictation on real Algerian Darija. For measuring only: the audio is not
redistributed (it stays under data/, which git ignores) and the licence forbids derivatives and commercial use.

usage: python fetch-casablanca-test.py <N> <out dir>      (needs: pyarrow soundfile numpy scipy; downloads one parquet file of about 300 MB)
"""
import os
import subprocess
import sys

import numpy as np
import pyarrow.parquet as pq
import scipy.signal
import soundfile as sf

n, out = int(sys.argv[1]), sys.argv[2]
url = "https://huggingface.co/datasets/UBC-NLP/Casablanca/resolve/main/Algeria/test-00000-of-00002.parquet"
os.makedirs(os.path.join(out, "dz_test"), exist_ok=True)
parquet = os.path.join(out, "casablanca-algeria-test-0.parquet")
if not os.path.exists(parquet):
    subprocess.run(["curl.exe", "-L", "--fail", "--retry", "3", "-C", "-", "-o", parquet, url], check=True)

pf = pq.ParquetFile(parquet)
rows = []
for batch in pf.iter_batches(batch_size=16, columns=["audio", "seg_id", "transcription", "duration"]):
    for rec in batch.to_pylist():
        audio, text = rec["audio"], (rec["transcription"] or "").strip()
        if not text or not audio or not audio.get("bytes"):
            continue
        import io
        data, rate = sf.read(io.BytesIO(audio["bytes"]), dtype="float32", always_2d=True)
        data = data.mean(axis=1)
        if rate != 16000:
            data = scipy.signal.resample_poly(data, 16000, rate).astype(np.float32)
        name = f"{rec['seg_id']}.wav".replace("/", "_")
        sf.write(os.path.join(out, "dz_test", name), data, 16000, subtype="PCM_16")
        rows.append((name, text.replace("\t", " ").replace("\n", " "), len(data) / 16000))
        if len(rows) >= n:
            break
    if len(rows) >= n:
        break
open(os.path.join(out, "dz_test.tsv"), "w", encoding="utf-8", newline="\n").write("".join(f"{a}\t{b}\n" for a, b, _ in rows))
print(len(rows), "utterances,", round(sum(d for _, _, d in rows)), "seconds")
