"""Converts a full Whisper model of Hugging Face (not an adapter) to CTranslate2 with int8 weights, in the layout of the dictation packs.

usage: python convert-hf-whisper.py <repo id> <out dir> [--revision <sha>] [--quantization int8|float16]
Needs: torch, transformers, ctranslate2, huggingface_hub. Prints the size and SHA-256 of every file of the result.
"""
import argparse
import hashlib
import os
import shutil
import subprocess
import sys

p = argparse.ArgumentParser()
p.add_argument("repo")
p.add_argument("out")
p.add_argument("--revision", default=None)
p.add_argument("--quantization", default="int8")
a = p.parse_args()

from huggingface_hub import hf_hub_download, snapshot_download  # noqa: E402

src = snapshot_download(a.repo, revision=a.revision, allow_patterns=["*.json", "*.txt", "model.safetensors"], ignore_patterns=["checkpoint-*/*", "training_args.bin"])
# the preprocessing settings of OpenAI's Whisper small, when the repository has none (they are the same for every size of 80 mel bins)
if not os.path.exists(os.path.join(src, "preprocessor_config.json")):
    shutil.copy(hf_hub_download("openai/whisper-small", "preprocessor_config.json", revision="973afd24965f72e36ca33b3055d56a652f456b4d"), os.path.join(src, "preprocessor_config.json"))
if not os.path.exists(os.path.join(src, "tokenizer.json")):
    shutil.copy(hf_hub_download("openai/whisper-small", "tokenizer.json", revision="973afd24965f72e36ca33b3055d56a652f456b4d"), os.path.join(src, "tokenizer.json"))
pack = os.path.join(a.out, "pack")
if os.path.isdir(pack):
    shutil.rmtree(pack)
subprocess.run(
    [sys.executable, "-m", "ctranslate2.converters.transformers", "--model", src, "--output_dir", pack, "--quantization", a.quantization,
     "--copy_files", "tokenizer.json", "preprocessor_config.json"],
    check=True,
)
for name in sorted(os.listdir(pack)):
    path = os.path.join(pack, name)
    print(f"{name}\t{os.path.getsize(path)}\t{hashlib.sha256(open(path, 'rb').read()).hexdigest()}")
