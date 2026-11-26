"""Build the shared transcription engine with the Android NDK or LLVM-MinGW."""
import argparse
from pathlib import Path
import subprocess

from dependencies import prepare, WHISPER_VERSION

here = Path(__file__).resolve().parent

def build(ndk, mingw):
    root = prepare(include_android=bool(ndk))
    common = [
        "cmake", "-S", str(here), "-G", "Ninja", "-DCMAKE_BUILD_TYPE=Release",
        f"-DWHISPER_SOURCE={root / ('whisper.cpp-' + WHISPER_VERSION)}",
    ]
    if ndk:
        for abi in ("arm64-v8a", "x86_64"):
            target = root / "android" / abi
            subprocess.run(common + [
                "-B", str(target), f"-DANDROID_ABI={abi}", "-DANDROID_PLATFORM=android-26",
                "-DANDROID_STL=c++_static", "-DGGML_CPU_ARM_ARCH=armv8-a",
                f"-DCMAKE_TOOLCHAIN_FILE={ndk / 'build/cmake/android.toolchain.cmake'}",
            ], check=True)
            subprocess.run(["cmake", "--build", str(target), "--parallel", "6"], check=True)
            library = root / "jni" / abi / "libytm_transcription.so"
            toolchains = ndk / "toolchains/llvm/prebuilt"
            strip = next(p for p in toolchains.glob("*/bin/llvm-strip*") if p.name in ("llvm-strip", "llvm-strip.exe"))
            subprocess.run([str(strip), "--strip-unneeded", "-o", str(library),
                           str(target / library.name)], check=True)
    else:
        target = root / "windows"
        ext = ".exe" if (mingw / "bin/clang.exe").exists() else ""
        subprocess.run(common + [
            "-B", str(target), "-DCMAKE_SYSTEM_NAME=Windows", "-DCMAKE_SYSTEM_PROCESSOR=x86_64",
            "-DGGML_SSE42=ON", "-DGGML_AVX=ON", "-DGGML_AVX2=ON", "-DGGML_FMA=ON", "-DGGML_F16C=ON", "-DGGML_BMI2=OFF",
            f"-DCMAKE_C_COMPILER={mingw / ('bin/x86_64-w64-mingw32-clang' + ext)}",
            f"-DCMAKE_CXX_COMPILER={mingw / ('bin/x86_64-w64-mingw32-clang++' + ext)}",
        ], check=True)
        subprocess.run(["cmake", "--build", str(target), "--parallel", "6"], check=True)

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--ndk", type=Path)
    mode.add_argument("--mingw", type=Path)
    args = parser.parse_args()
    build(args.ndk, args.mingw)

