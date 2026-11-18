ゆうこニュース日和に同梱している「このパソコンの中で動く AI」の部品と、そのライセンス
Third-party notices for the bundled local AI components
=====================================================================

このフォルダ（local_llm/LICENSES）には、同梱している部品のライセンス本文を置いています。
This folder contains the license texts of the bundled components.

---------------------------------------------------------------------
1. AI モデル / AI model
---------------------------------------------------------------------
- Gemma 4 E2B（Google）の GGUF 版（Q4_K_M）
  元のモデル / Original model: https://huggingface.co/google/gemma-4-E2B-it
  GGUF 版の入手元 / GGUF source: https://huggingface.co/unsloth/gemma-4-E2B-it-GGUF
    （版 / revision 0314792d7f1f7e229411f620751375812bb9faf2）
  ライセンス / License: Apache License 2.0 → Gemma-4-Apache-2.0.txt
    （https://ai.google.dev/gemma/docs/gemma_4_license）

  このモデルはインストーラーには入っていません。アプリの初回の起動のときに、上の入手元から
  直接ダウンロードし、大きさと SHA-256 を確かめてから使います。置き場所は
  %LOCALAPPDATA%\<アプリの識別子>\local_llm\models\ の下です。
  The model is not included in the installer. On first run, the app downloads it
  directly from the source above and checks its size and SHA-256 before use.

  - gemma-4-E2B-it-Q4_K_M.gguf （3,106,738,272 bytes）
    SHA-256: 740185b21d22ceb83a11c3aa62ad5842ef32c70f6096d756bbee85a1e4ec34b8

  改変の告知 / Notice of modification (Apache License 2.0, Section 4(b)):
  本アプリはこのファイルを改変せず、入手元のファイルをそのまま使います（追加学習・変換・量子化を
  していません）。GGUF 形式への変換と量子化は入手元（unsloth）が行ったものです。
  This app uses the file as published, without modification (no fine-tuning, conversion
  or quantization by us). The GGUF conversion and quantization were done by the source (unsloth).

---------------------------------------------------------------------
2. AI を動かす部品 / AI runtime (runtime/)
---------------------------------------------------------------------
- llama.cpp / ggml（b11269・commit cee37ffea0a5749bce1704f0621b9ddd185b4858）
  入手元 / Source: https://github.com/ggml-org/llama.cpp （公式リリース llama-b11269-bin-win-cpu-x64.zip）
  ライセンス / License: MIT → llama.cpp-MIT.txt

  この実行ファイルには、次の部品が組み込まれています / The runtime includes:
  - cpp-httplib（MIT）→ cpp-httplib-MIT.txt
  - nlohmann/json（MIT）→ nlohmann-json-MIT.txt
  - BoringSSL 0.20260903.0（Apache License 2.0 ほか）→ BoringSSL.txt
  - LLVM OpenMP（libomp.dll・Apache License 2.0 with LLVM Exceptions）→ LLVM-OpenMP-Apache-2.0-with-LLVM-exception.txt
  - stb_image（MIT または Public Domain）→ stb_image-MIT-or-PublicDomain.txt
  - miniaudio（Public Domain または MIT No Attribution）→ miniaudio-PublicDomain-or-MIT-0.txt
  - subprocess.h（The Unlicense）→ subprocess.h-Unlicense.txt

---------------------------------------------------------------------
3. Visual C++ ランタイム / Microsoft Visual C++ runtime (runtime/)
---------------------------------------------------------------------
- vcruntime140.dll / vcruntime140_1.dll / msvcp140.dll（14.44.35211.0）
  Microsoft Visual C++ 再頒布可能パッケージのファイルです。Visual Studio のライセンス条項
  （再頒布可能コードの条件）に従って再頒布しています。著作権は Microsoft Corporation にあります。
  These are Microsoft Visual C++ Redistributable files, redistributed under the
  Visual Studio license terms for Distributable Code. (c) Microsoft Corporation.

---------------------------------------------------------------------
外部への送信について / Network
---------------------------------------------------------------------
これらの部品は、このパソコンの中だけで動きます（127.0.0.1・ネットへの接続を切った状態で起動）。
These components run only on this computer (127.0.0.1, started in offline mode).
モデルの初回のダウンロードのときだけ、1. の入手元（huggingface.co と、その配信用のホスト）へ接続します。
Only the first-run model download connects to the source in section 1 (huggingface.co and its CDN).
