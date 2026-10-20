# Nori

<p align="center"><b>Describe it. Dig it up.</b><br>
Search the files on your computer by what's in them, with Google DeepMind's
<a href="https://huggingface.co/google/embeddinggemma-2">EmbeddingGemma 2</a>.<br>
<sub>Free and open source. 100% local. Written in Rust. Everything runs on your computer.</sub></p>

---

EmbeddingGemma 2 is the multimodal embedding model Google DeepMind released in October 2026. It puts text, images,
audio and video into one shared space: a photo of a dog on a beach lands next to the words "a dog on the beach", and a
recording of someone talking about sleep lands next to "where they talk about sleep". It understands more than 100
languages, it's open (Apache 2.0), and at 740M parameters it's small enough to run on a laptop.

**Nori** brings that to the files on your computer, on Windows, macOS, or Linux. It is a ground-up **Rust rewrite**
of [DigUp](https://github.com/ARahim3/DigUp), replacing the original split Swift/Tauri architecture with a unified,
modular, memory-safe Rust workspace.

Pick the folders to search, and Nori reads the pictures, PDFs, documents, recordings, videos and code projects in them.
Then describe what you're looking for, in your own words, and it finds the match, whatever kind of file it is.

---

## 🚦 Migration Progress Tracker

Nori is actively migrating DigUp's original Swift engine and Tauri wrapper into a modular, high-performance Rust workspace.

```
DigUp (Swift / SwiftUI / Tauri / C++) ──► Nori (Modular Rust Workspace 2024 Edition)
```

| Subsystem / Crate | DigUp Source (Swift/C++) | Target Rust Architecture | Status | Highlights & Parity Details |
|---|---|---|:---:|---|
| **Core Domain Types** | `Types.swift` | `crates/nori-core/src/types.rs` | 🟢 Done | `FileKind`, `SegmentKind`, `Modality`, `Candidate`, `FileRecord`, `SegmentRecord` with serde serialization. |
| **Model Manifest & Paths** | `ModelFiles.swift`, `AppPaths.swift` | `crates/nori-core/src/media_files.rs` | 🟢 Done | Cross-platform data paths (`LOCALAPPDATA`, `XDG`, `Application Support`), HF Hub cache resolver, streaming SHA-256 verifier. |
| **Task Prompts & Prefix** | `Prompts.swift`, `CodeQuery.swift` | `crates/nori-core/src/prompts.rs` | 🟢 Done | DeepMind task prompts (`task: search result`, `task: code retrieval`), `code:` query parser. |
| **Vector Math & SIMD** | Apple Accelerate / vDSP | `crates/nori-core/src/vectors.rs` | 🟢 Done | 768-dim FP16 binary storage, 8-way unrolled SIMD dot product scoring, anonymous mmap block buffer. |
| **Lexical Normalization** | `SearchText.swift` | `crates/nori-text/src/normalize.rs` | 🟢 Done | NFKD decomposition, selective Latin diacritic stripping, Arabic alef/tatweel normalization, natural language detector (`has_words`). |
| **Script Diagnostics** | `PageTextProblem.swift` | `crates/nori-text/src/script.rs` | 🟢 Done | Bengla glyph order checks (reversed matras/rephs), Arabic lost letter detection (isolated presentation forms), abbreviation handling. |
| **Word Boundary & Matching** | `SearchText.matches` (`.diacriticInsensitive`) | `crates/nori-text/src/matches.rs` | 🟢 Done | UAX #29 grapheme clustering (`unicode-segmentation`), Indic & Arabic inflected suffix swallow, verbatim byte-range mapping. |
| **Snippet Extraction** | `SearchText.snippet` | `crates/nori-text/src/snippet.rs` | 🟢 Done | Sentence-boundary aware context window with exact highlight preservation. |
| **Database & Storage** | SQLite3 + `fts5` | `crates/nori-storage` | 🟡 In Progress | SQLite connection management, schema migration (`files`, `segments`, `fts_segments`), zero-copy binary BLOB storage. |
| **PDF Extraction** | PDFKit / PDFium | `crates/nori-extract/pdf` | 🟡 In Progress | Text layer extraction, scan detection, bounded page rendering fallback. |
| **Image & OCR Extraction** | Apple Vision / WinRT OCR | `crates/nori-extract/ocr` | 🟡 In Progress | Native Windows `Windows.Media.Ocr` WinRT integration (<50ms, zero cloud), cross-platform image metadata. |
| **Audio & Video Windowing** | AVFoundation / FFmpeg | `crates/nori-extract/media` | ⚪ Planned | 30s overlapping audio windows, video shot-boundary keyframe extraction. |
| **Inference Engine** | `llama.cpp` process | `crates/nori-engine` | ⚪ Planned | In-process or helper GGUF runner for EmbeddingGemma 2 740M Q8_0 (Vulkan / Metal / CPU). |
| **Desktop App & Shell** | SwiftUI / Tauri v1 | `crates/nori-app` / Tauri v2 | ⚪ Planned | Global search overlay (`Ctrl+Shift+Space` / `⇧⌘Space`), tray daemon, deep file opener links. |

---

## What you can find

- "zebra in a video" opens the clip at the moment the zebra shows up
- "where they talk about sleep" jumps to that minute of a podcast
- "the clause about pets in the lease" shows the PDF page
- "a dog on the beach" finds the photo, and "payment declined error" the screenshot
- a search in English can find a note written in Bengali, Arabic or any of the model's 100+ languages, and the other
  way around
- "code: retry a failed call with backoff" finds the function in your projects, if you turn on code search

Exact words count too. Invoice numbers, error codes and names are matched as written, in file names and document text,
so a search gets both the meaning and the exact string.

## Private

Nothing you index leaves your computer. Nori goes online only to download the model once and to check for updates
once a day. The update check sends nothing about your files, and you can turn it off. No account, no telemetry.

## Using it

| Windows & Linux | macOS | What it does |
|---|---|---|
| **Ctrl+Shift+Space** | **⇧⌘Space** | Opens the search panel from any app (configurable) |
| **↓ ↑** | **↓ ↑** | Moves through the results |
| **Enter** | **↩** | Opens the result: PDFs at the page, recordings and videos at the moment, code in your editor at the line |
| **Space** | **Space** | A big preview, at the page or moment (once you've moved to a result) |
| **Ctrl+Enter** | **⌘↩** | Shows it in Explorer, Finder or your file manager |
| **Ctrl+C** (in window) | **⌘C** | Copies the file, to paste anywhere |
| **Ctrl+O** | **⌘O** | The same search in a bigger window, with a picture grid and filters |

Nori lives in the tray on Windows and Linux and the menu bar on macOS, where you can see indexing progress,
pause it, or open Settings.

A few things by system:

- On Windows and Linux, PDFs open at the page in your PDF viewer if it's one that can be told a page (Edge, Chrome,
  Firefox, Acrobat, SumatraPDF, Evince, Okular), and recordings and videos at the moment in VLC, mpv or MPC-HC. Other
  apps open the file from the start; the preview plays it from the moment either way.
- On macOS, a video opens at its moment in QuickTime Player or IINA, and a PDF opens in Preview with your words in
  its search.
- On GNOME and KDE under Wayland, which don't let apps set their own global shortcuts, Nori registers the shortcut
  in desktop keyboard settings during setup.

## What it reads

Only the folders you choose:

- **pictures and screenshots** (JPEG, PNG, HEIC, WebP and more), by what they show, and screenshots by their text
- **PDFs**, page by page, scans included. A long one gets its first 30 pages right away and the rest once everything
  else is in (up to 5,000 pages a file)
- **documents**: txt, md, rtf, docx, odt, html. Text that's mostly numbers, like a data export, is matched by its
  exact text only
- **audio of any length**: the model listens to 30 seconds at a time, so Nori takes overlapping 30-second windows and a
  match points to its moment
- **video of any length**: a frame every few seconds, one per shot, plus the soundtrack the same way as audio

Outside the folders you pick for code search, it never reads code, and inside a code project (a folder with `.git`,
`package.json` and the like) it reads only screenshots. It never reads keys, certificates, password files, hidden
folders like `.ssh`, apps or caches. It skips folders that look like datasets, and never downloads files that only
live in the cloud (OneDrive on Windows, iCloud on macOS). You can skip more folders or file types in Settings. Nori
only reads your files; it never changes, moves or uploads them.

The first pass happens once. On GPU (via Vulkan or Metal), processing is fast; on a processor alone a picture takes
a few seconds, so a large photo library can take time, which is why setup shows an estimate first. After that, a new
file is searchable a few seconds after it lands.

## Code

EmbeddingGemma 2 reads code too, so Nori can search your projects. Code search is off until you turn it on in
Settings for the folders you pick. Each repo shows how long reading it takes and how much room its index needs.
Then start a search with `code:` and say what the code does, or type a name that's in it:

- "code: retry a failed call with backoff" finds the function, whatever it's called
- "code: refreshAccessToken" goes to the line that defines it, not the places that call it
- "code: plot how many examples each class has" finds the notebook cell

Results open in your editor at the line: Zed, VS Code, Cursor, Sublime Text, JetBrains IDEs, Notepad++ on Windows,
and Xcode on a Mac (pick one in Settings). Code has an index of its own and never shows up in other searches. Nori
reads source files, notebooks (without their outputs) and a repo's own docs. It skips what git ignores, vendored and
generated code, data files, and anything that looks like a key or a password. While you're working on code, the index
catches up shortly after things go quiet.

## How it works

```
your folders ─► readers that turn files into pictures, text and sound
             ─► EmbeddingGemma 2 on llama.cpp (GGUF), running locally on GPU or CPU
             ─► one SQLite database: 768-dim FP16 vector BLOBs for every picture, page, passage,
                video frame and 30 s of sound, plus an FTS5 keyword index (code has a second one)
your words   ─► nori-text normalizer ─► text embedding (~250 MB in memory) ─► SIMD nearest vectors + FTS5 exact words
```

Nori runs ggml-org's 8-bit build of EmbeddingGemma 2 (`embeddinggemma-2-Q8_0.gguf`) through llama.cpp: on Windows and
Linux on the GPU with Vulkan (falling back to CPU), and on macOS with Metal. Every vector lives in the same
768-dimensional space, which is why one query can rank a video frame against a PDF page.

Exact-word matches get a boost through SQLite FTS5, because no vector embedding holds an invoice number or error code
precisely.

The text processing pipeline is built in pure Rust (`crates/nori-text`):
- NFKD decomposition with selective Latin diacritic folding (`Café` matches `cafe` without mangling Indic vowel marks).
- Inflected script suffix extension: in Bengali and Arabic, searching a root matches and extends to the inflected word
  (e.g., `বাংলা` matches all of `বাংলায়`).
- Grapheme cluster boundary analysis via Unicode Standard Annex #29.

## Build from source

### Prerequisites

- **Rust toolchain**: 2024 Edition (`rustc 1.85+`)
- **C/C++ build tools**: Visual Studio C++ Build Tools (Windows), `build-essential` (Linux), or Xcode Command Line Tools (macOS)
- **CMake**: for building llama.cpp libraries

### Compiling and Testing

```sh
# Clone the repository
git clone https://github.com/ARahim3/nori && cd nori

# Run all unit tests across the workspace
cargo test --workspace

# Run nori-text normalization & parity tests
cargo test -p nori-text

# Run nori-core vector SIMD & model tests
cargo test -p nori-core

# Build release binary
cargo build --release
```

## FAQ

**How is this different from Windows Search or Spotlight?** They find files by their names, text and metadata. Nori
also finds them by what they show or say, and takes you to the page or moment. They work fine side by side.

**Why is the download 865 MB?** That's the model: the text part (310 MB) and the image and audio encoders (555 MB).
The model is downloaded once upon initial setup.

**Will it slow my computer down?** Only the first pass is heavy. After that it reads new files only. The embedding
engine runs at low priority, and the search model only loads while searching.

**Which languages?** The model knows more than 100, and Nori searches in all of them, across languages too. Exact-word
matching supports Latin, Arabic, Indic (Bengali, Devanagari), and other alphabetic/inflected scripts.

**How do I remove it?**
- **Windows**: Delete `%LOCALAPPDATA%\Nori` (which holds the model and index).
- **Linux**: Delete `~/.local/share/nori` (the model and index) and `~/.config/nori`.
- **macOS**: Delete `~/Library/Application Support/Nori`.

## Thanks

Nori is built on [EmbeddingGemma 2](https://huggingface.co/google/embeddinggemma-2) by Google DeepMind (Apache 2.0)
and [llama.cpp](https://github.com/ggml-org/llama.cpp) (MIT). Inspired by [DigUp](https://github.com/ARahim3/DigUp).

Nori isn't affiliated with Google, Apple or Microsoft. Gemma is a trademark of Google LLC; Mac and Spotlight are
trademarks of Apple Inc.; Windows is a trademark of Microsoft Corporation.

## License

MIT
