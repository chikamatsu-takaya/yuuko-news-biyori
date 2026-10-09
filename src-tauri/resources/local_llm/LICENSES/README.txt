ゆうこニュース日和に同梱している「このパソコンの中で動く AI」の部品と、そのライセンス
Third-party notices for the bundled local AI components
=====================================================================

このフォルダ（local_llm/LICENSES）には、同梱している部品のライセンス本文を置いています。
This folder contains the license texts of the bundled components.

---------------------------------------------------------------------
1. AI モデル / AI model
---------------------------------------------------------------------
- Qwen3.5-2B（Qwen Team, Alibaba Cloud）
  入手元 / Source: https://huggingface.co/Qwen/Qwen3.5-2B
  ライセンス / License: Apache License 2.0 → Qwen3.5-Apache-2.0.txt

  ★ 改変の告知 / Notice of modification (Apache License 2.0, Section 4(b)):
  同梱している次のファイルは、上記の公式の重みを llama.cpp（b11269）で GGUF 形式に変換し、
  量子化したものです（中身の追加学習はしていません）。変換は、同じ開発元のアプリ
  「すたりお（stario）」のために行ったもので、そのファイルをそのまま使っています。
  The following file was converted to GGUF and quantized from the official weights above,
  using llama.cpp (b11269), originally for the "stario" app. No fine-tuning was applied.

  - models/stario-qwen3.5-2b-q4_k_m.gguf （Q4_K_M）
    SHA-256: 5405508fd56e0bace3ec4c2484eb4d0f606cbded87760cf3756896e234068b35

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
外部への送信について / Network
---------------------------------------------------------------------
これらの部品は、このパソコンの中だけで動きます（127.0.0.1・ネットへの接続を切った状態で起動）。
These components run only on this computer (127.0.0.1, started in offline mode).
