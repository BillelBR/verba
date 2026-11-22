"""Builds the Algerian Darija dictation pack: Whisper small + the LoRA adapter EL-ANKA-Lite (algerian-nlp, MIT) merged into one model and converted
to CTranslate2 with int8 weights (about 250 MB), in the layout of the other dictation packs (`model.bin`, `config.json`, `tokenizer.json`,
`vocabulary.txt`, `preprocessor_config.json`).

usage: python build-darija-pack.py <out dir> [--adapter algerian-nlp/EL-ANKA-Lite] [--adapter-rev <sha>] [--quantization int8|float16]

Needs: torch (CPU is enough), transformers, peft, ctranslate2, huggingface_hub, sentencepiece. It downloads the base model (1 GB) and the adapter.
Prints the size and the SHA-256 of every file: these go into the catalogue of `crates/verba-app/src/dictation.rs`.
"""
import argparse
import hashlib
import os
import shutil
import subprocess
import sys

BASE = "openai/whisper-small"
BASE_REV = "973afd24965f72e36ca33b3055d56a652f456b4d"
ADAPTER = "algerian-nlp/EL-ANKA-Lite"

p = argparse.ArgumentParser()
p.add_argument("out")
p.add_argument("--adapter", default=ADAPTER)
p.add_argument("--adapter-rev", default=None)
p.add_argument("--quantization", default="int8")
a = p.parse_args()

import torch  # noqa: E402
from huggingface_hub import snapshot_download  # noqa: E402
from peft import PeftModel  # noqa: E402
from transformers import WhisperForConditionalGeneration, WhisperProcessor  # noqa: E402

base_dir = snapshot_download(BASE, revision=BASE_REV, allow_patterns=["*.json", "*.txt", "model.safetensors"])
adapter_dir = snapshot_download(a.adapter, revision=a.adapter_rev)
print("base:", BASE, BASE_REV, "adapter:", a.adapter, os.path.basename(adapter_dir))

model = WhisperForConditionalGeneration.from_pretrained(base_dir, torch_dtype=torch.float32)
model = PeftModel.from_pretrained(model, adapter_dir)
model = model.merge_and_unload()
merged = os.path.join(a.out, "merged")
os.makedirs(merged, exist_ok=True)
model.save_pretrained(merged, safe_serialization=True)
WhisperProcessor.from_pretrained(base_dir).save_pretrained(merged)

pack = os.path.join(a.out, "pack")
if os.path.isdir(pack):
    shutil.rmtree(pack)
subprocess.run(
    [sys.executable, "-m", "ctranslate2.converters.transformers", "--model", merged, "--output_dir", pack, "--quantization", a.quantization,
     "--copy_files", "tokenizer.json", "preprocessor_config.json"],
    check=True,
)
for name in sorted(os.listdir(pack)):
    path = os.path.join(pack, name)
    h = hashlib.sha256(open(path, "rb").read()).hexdigest()
    print(f"{name}\t{os.path.getsize(path)}\t{h}")
