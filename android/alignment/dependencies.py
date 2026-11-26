"""Fetch pinned Android runtime and English acoustic model for the APK packager."""
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from urllib.request import urlopen
from zipfile import ZipFile
import hashlib
import shutil
import tarfile

WHISPER_VERSION = "1.8.3"

ROOT = Path(__file__).resolve().parents[2] / "target/lyric-alignment"
MODEL_REV = "a19f851b3d42865797e410752b4c570c871e4825"
ORT = "1.22.0"
MAVEN = f"https://repo.maven.apache.org/maven2/com/microsoft/onnxruntime/onnxruntime-android/{ORT}/onnxruntime-android-{ORT}.aar"

def fetch(item):
    name, url, algorithm, expected = item
    path = ROOT / name
    if not path.exists():
        with urlopen(url, timeout=120) as response, path.with_suffix(".part").open("wb") as out:
            shutil.copyfileobj(response, out)
        path.with_suffix(".part").replace(path)
    with path.open("rb") as fd:
        if hashlib.file_digest(fd, algorithm).hexdigest() != expected:
            raise ValueError(f"Digest mismatch for {path}")

def prepare(include_android=True):
    whisper = [
        ("ggml-base-q5_1.bin", "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base-q5_1.bin", "sha256", "422f1ae452ade6f30a004d7e5c6a43195e4433bc370bf23fac9cc591f01a8898"),
        ("whisper-src.tar.gz", f"https://codeload.github.com/ggml-org/whisper.cpp/tar.gz/refs/tags/v{WHISPER_VERSION}", "sha256", "870ba21409cdf66697dc4db15ebdb13bc67037d76c7cc63756c81471d8f1731a"),
    ]
    ROOT.mkdir(parents=True, exist_ok=True)
    items = [
        ("english.onnx", f"https://huggingface.co/Xenova/wav2vec2-base-960h/resolve/{MODEL_REV}/onnx/model_quantized.onnx", "sha256", "cd5040c147381580ed73258143dd8e0c28e800a09e74ee42ee2b3e8cb4d760a3"),
        ("runtime.aar", MAVEN, "sha256", "04a4617a9c797cf49225595e45b5546081cb34c86ac817581141577d3b7dbfe2"),
    ]
    with ThreadPoolExecutor() as pool:
        list(pool.map(fetch, whisper + (items if include_android else [])))
    with tarfile.open(ROOT / "whisper-src.tar.gz") as archive:
        archive.extractall(ROOT, filter="data")
    if not include_android:
        return ROOT
    with ZipFile(ROOT / "runtime.aar") as archive:
        for item in archive.namelist():
            if item.startswith("headers/") or item in [f"jni/{abi}/libonnxruntime.so" for abi in ("arm64-v8a", "x86_64")]:
                archive.extract(item, ROOT)
    return ROOT

if __name__ == "__main__":
    print(prepare())

