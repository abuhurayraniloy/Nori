# Nori 🌊

> **The Zero-Cloud, Autonomous Desktop Organizer & Semantic Search Engine.**  
> *Watches your folder, reads documents & screenshots locally, categorizes and normalizes filenames, and guarantees a 1-click Undo safety net.*

---

[![Rust 2024](https://img.shields.io/badge/Rust-2024_Edition-orange?logo=rust)](https://www.rust-lang.org/)
[![Platform Windows](https://img.shields.io/badge/Platform-Windows_10%20%7C%2011-blue?logo=windows)](https://microsoft.com/windows)
[![Zero Cloud](https://img.shields.io/badge/Privacy-100%25_Local_%2F_Zero_Cloud-success)](#privacy--zero-cloud-guarantee)
[![License: MIT](https://img.shields.io/badge/License-MIT-purple.svg)](LICENSE)

---

## 💡 The Problem & The Vision

Downloads folders inevitably become digital landfills of cryptic filenames like `inv_091823(1).pdf`, unstamped screenshots (`Screenshot 2026-03-14.png`), and forgotten installation packages. Cloud-based organizing tools compromise privacy by uploading personal invoices, tax documents, and medical receipts to external servers.

**Nori** is an ultra-fast, local-first background daemon written in Rust. It quietly monitors your drop folders, waits for downloads to physically complete without freezing on lock-contentions, extracts text from documents and screenshots using native Windows APIs, organizes files cleanly, and provides an instant semantic search engine running sub-20ms queries with zero cloud calls.

---

## ⚡ Key Highlights

- **🛡️ Indestructible Safety Net (1-Click Undo):** Every file movement and renaming operation is recorded as an atomic transaction in an embedded SQLite ledger. If anything is misplaced, one click rolls it back to its original name and location.
- **👁️ Multi-Modal Local Eyes (Zero-Cloud Vision):**
  - **PDF Text Extractor:** Extracts text and structural metadata with strict bounds (100 MB limits, page clipping, and `catch_unwind` panic containment).
  - **Native Windows Media OCR:** Directly taps into the Windows Runtime (`Windows.Media.Ocr`) to extract text from screenshots and photos in under 50 ms with zero external cloud dependencies.
- **⏱️ Download Debounce & Quiescence Engine:**
  - Automatically suppresses transient browser download stubs (`.crdownload`, `.part`, `.tmp`).
  - Multi-sample size stability observation ensures the transfer has finished growing.
  - Windows-native `share_mode(0)` (`FILE_SHARE_NONE`) verification guarantees the writing process (Chrome, Edge, Explorer) has released all OS file handles.
- **🧵 Non-Blocking Concurrency:** The primary filesystem event loop runs on sub-millisecond execution times, dispatching file stabilization and parsing tasks to background workers while preventing duplicate concurrent processing with an `Arc<Mutex<HashSet<PathBuf>>>` registry.
- **🔍 Instant Semantic Vector Search:**
  - Employs local ONNX embedding models via `fastembed` (`SnowflakeArcticEmbedM` with 768 dimensions).
  - Uses zero-copy binary serialization with `bytemuck` to store raw `BLOB` vectors in SQLite.
  - Performs sub-20ms cosine similarity scans directly on CPU SIMD instructions.

---

## 🏗️ Architecture & Processing Pipeline

```mermaid
flowchart TD
    subgraph S1 ["1. Watcher & Ingestion"]
        A["File Event (Create / Modify)"] --> B["Non-blocking Watcher Loop"]
        B --> C{"Is file & not already in-flight?"}
        C -- No --> D["Skip Event"]
        C -- Yes --> E["Claim in In-Flight Set"]
        E --> F["Spawn Worker Thread"]
    end

    subgraph S2 ["2. Quiescence & Validation"]
        F --> G["Observe Size Stability (debounce.rs)"]
        G --> H{"Stable size & share_mode(0) lock free?"}
        H -- No / Timed out --> I["Release from In-Flight Set (Allow Retry)"]
        H -- Yes --> J["Extension Sorter & Multi-Modal Router"]
    end

    subgraph S3 ["3. Organization & Text Extraction"]
        J --> K["Relocate File to Organized/<Category>/"]
        K --> L{"Is Text Extractable?"}
        L -- PDF --> M1["PDF Extractor (extractors/pdf.rs)"]
        L -- Images --> M2["Windows Media OCR (extractors/ocr.rs)"]
        M1 & M2 --> N["Paragraph Chunker (512 char windows)"]
    end

    subgraph S4 ["4. Vector Embeddings & Storage"]
        N --> O["SnowflakeArcticEmbedM (fastembed)"]
        O --> P["Zero-Copy bytemuck Serialization"]
        P --> Q["Atomic Batch Insert into SQLite document_chunks"]
    end

    subgraph S5 ["5. User Interface & Search"]
        R["User Query: cargo run -- search <query>"] --> S["Vectorize Query Text"]
        S --> T["Cosine Similarity Scan (SQLite BLOBs)"]
        T --> U["Ranked Top-K Snippets & File Paths (< 20ms)"]
    end
```

---

## 🗺️ Project Roadmap & Milestones

This project follows an anti-burnout granular implementation structure:

### Phase 1: The Watcher Foundation
- [x] **Task 1.1: Rust CLI & SQLite Schema** — Embedded database initialization with `files` and `transactions` tables.
- [x] **Task 1.2: Raw Directory Watcher** — Non-blocking filesystem event listener powered by `notify`.
- [x] **Task 1.3: Download Debounce Engine** — Multi-sample size stabilization, browser extension filtering, and Windows `share_mode(0)` verification with comprehensive unit tests.

### Phase 2: The Multi-Modal Eyes
- [x] **Task 2.1: PDF Header & Text Extractor** — Memory-bounded PDF reader with password handling, page limits, and panic containment (`extractors/pdf.rs`).
- [x] **Task 2.2: Native Windows Media OCR** — Zero-cloud image text extraction using native `Windows.Media.Ocr` (`extractors/ocr.rs`).
- [x] **Task 2.3: Multi-Modal Extraction Router** — Unified dispatcher (`extractors/mod.rs`) mapping file formats to appropriate extractors.

### Phase 3: The Smart Janitor & Safety Net
- [x] **Task 3.1: Extension-Based Media Categorizer** — Clean relocation into `Documents`, `Images`, `Audio`, `Video`, `Archives`, `Installers`, `Code`, and `Other` (`media.rs`).
- [ ] **Task 3.2: Normalized Filename Builder** — Automated naming convention (`YYYY-MM-DD_<Vendor>_<Title>.<ext>`) with Windows path sanitization.
- [ ] **Task 3.3: Safe Mover & 1-Click Undo Ledger** — Atomic filesystem relocation with rollback tracking.

### Phase 4: Instant Search & Memory
- [x] **Task 4.1: Paragraph Chunking Engine** — Semantic paragraph-boundary text segmentation with zero word slicing.
- [x] **Task 4.2: Local ONNX Vector Embeddings** — `SnowflakeArcticEmbedM` local model generating 768-dimensional vectors via `fastembed`.
- [x] **Task 4.3: Binary Vector SQLite Storage** — Zero-copy `bytemuck` slice casting with single-transaction prepared statements and B-Tree indexing.
- [x] **Task 4.4: Sub-20ms Semantic Search** — High-performance cosine similarity ranking directly over SQLite `BLOB` records.

### Phase 5: The Sleek Desktop Shell
- [ ] **Task 5.1: Tauri v2 System Tray** — Minimalist system tray daemon with quick actions.
- [ ] **Task 5.2: Spotlight Launcher (`Alt + Space`)** — Centered, translucent Raycast-style instant search overlay.
- [ ] **Task 5.3: End-to-End Polish & Chaos Testing** — Bulk download stress-testing and distribution builds.

---

## 📂 Codebase Layout

```text
nori/
├── Cargo.toml               # Crate dependencies & metadata (Rust 2024 Edition)
├── README.md                # Project documentation & architectural specification
├── nori.db                  # Local SQLite database (git-ignored)
└── src/
    ├── main.rs              # CLI command router (watch / search) & app bootstrap
    ├── watcher.rs           # Filesystem event loop & background task dispatcher
    ├── debounce.rs          # Quiescence polling, share_mode checks & unit tests
    ├── media.rs             # Extension categorization & collision-safe destinations
    ├── indexer.rs           # Paragraph chunker, FastEmbed ONNX engine & semantic search
    ├── db.rs                # SQLite schema setup, document_chunks table & test helper
    └── extractors/          # Multi-modal extraction engine
        ├── mod.rs           # Extractor router, error taxonomy & safety guards
        ├── pdf.rs           # PDF text extractor with panic isolation
        └── ocr.rs           # Native Windows Media OCR engine (WinRT)
```

---

## 🗄️ Database Schema

Nori relies on embedded SQLite to ensure persistent, verifiable state:

### Table: `document_chunks`
| Column | Type | Constraints | Description |
| :--- | :--- | :--- | :--- |
| `id` | `INTEGER` | `PRIMARY KEY AUTOINCREMENT` | Unique chunk record ID |
| `file_path` | `TEXT` | `NOT NULL` | Absolute target file path |
| `chunk_index` | `INTEGER` | `NOT NULL` | Chronological paragraph order |
| `chunk_text` | `TEXT` | `NOT NULL` | Extracted human-readable paragraph text |
| `embedding` | `BLOB` | `NOT NULL` | 3,072-byte raw binary vector (768 32-bit floats) |

*Indexed on `file_path` via `idx_chunks_file_path` for instant idempotent updates.*

### Table: `files`
| Column | Type | Constraints | Description |
| :--- | :--- | :--- | :--- |
| `id` | `TEXT` | `PRIMARY KEY` | Unique document ID (UUIDv4) |
| `current_path` | `TEXT` | `NOT NULL` | Current path on disk |
| `original_path` | `TEXT` | `NOT NULL` | Original path where file landed |
| `filename` | `TEXT` | `NOT NULL` | Normalized filename |
| `created_at` | `DATETIME`| `NOT NULL` | Timestamp of ingestion |

### Table: `transactions`
| Column | Type | Constraints | Description |
| :--- | :--- | :--- | :--- |
| `transaction_id` | `TEXT` | `PRIMARY KEY` | Unique transaction ID (UUIDv4) |
| `source_path` | `TEXT` | `NOT NULL` | Pre-move file location |
| `destination_path`| `TEXT` | `NOT NULL` | Post-move file location |
| `timestamp` | `DATETIME`| `NOT NULL` | Operation timestamp |
| `is_reverted` | `INTEGER`| `DEFAULT 0` | Reversion state (`0` = active, `1` = reverted) |

---

## 🚀 Getting Started

### Prerequisites

- **Operating System:** Windows 10 or 11 (64-bit)
- **Rust Toolchain:** Rust compiler supporting the **2024 Edition** (`rustc 1.85+`)
- **C Compiler Tools:** Visual Studio C++ Build Tools or Windows SDK (for bundled SQLite compilation)

### Build & Run

1. **Clone the repository:**
   ```powershell
   git clone https://github.com/your-username/nori.git
   cd nori
   ```

2. **Run tests & benchmarks:**
   ```powershell
   # Run all unit tests
   cargo test

   # Run semantic search benchmark with release SIMD optimizations
   cargo test benchmark_search_latency --release -- --nocapture
   ```

3. **Start the background file organizer & watcher:**
   ```powershell
   cargo run -- watch
   ```
   *Any file or PDF dropped into `Downloads/` will be organized into `Organized/` and automatically indexed in the background.*

4. **Search your documents by concept/meaning (Semantic Search):**
   ```powershell
   cargo run -- search "machine learning deployment and model evaluation"
   ```

5. **Build release binary:**
   ```powershell
   cargo build --release
   ```
   The binary will be compiled to `target\release\nori.exe`.

---

## 🔒 Privacy & Zero-Cloud Guarantee

All data processed by Nori—including database indices, PDF contents, OCR text, and semantic vectors—**remains strictly on your local machine**. Nori does not make network calls, does not transmit telemetry, and requires no external API keys or cloud accounts.

---

## 📄 License

This project is licensed under the [MIT License](LICENSE).
