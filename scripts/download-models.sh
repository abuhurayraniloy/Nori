#!/bin/bash
# Nori model downloader: fetches EmbeddingGemma 2 Q8_0 GGUF models from Hugging Face
# Repository: ggml-org/embeddinggemma-2-GGUF (commit bfcd298762cc34d0357ece5ebdd31791a3a374d8)
#
# Usage:
#   scripts/download-models.sh [--to <directory>]
#
# Defaults:
#   macOS:   ~/Library/Application Support/Nori/Models
#   Linux:   ${XDG_DATA_HOME:-~/.local/share}/nori/Models
#   Windows: $LOCALAPPDATA/Nori/Models

set -euo pipefail

REPO="ggml-org/embeddinggemma-2-GGUF"
REV="bfcd298762cc34d0357ece5ebdd31791a3a374d8"
BASE_URL="https://huggingface.co/${REPO}/resolve/${REV}"

# Model definitions: name, size in bytes, expected sha256
TEXT_NAME="embeddinggemma-2-Q8_0.gguf"
TEXT_BYTES=309855456
TEXT_SHA="2188ac1deca4b77dffefd603c2776a9d76d9d74ec01841392982ebb840b09135"

MEDIA_NAME="mmproj-embeddinggemma-2-Q8_0.gguf"
MEDIA_BYTES=554821024
MEDIA_SHA="c4a8a52691ecef40618438928bdf9e68379b854e24166f292592353db0aab64f"

# Determine target directory
TARGET_DIR=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --to)
      TARGET_DIR="$2"
      shift 2
      ;;
    *)
      echo "Unknown argument: $1" >&2
      echo "Usage: $0 [--to <dir>]" >&2
      exit 1
      ;;
  esac
done

if [[ -z "$TARGET_DIR" ]]; then
  case "$(uname -s)" in
    Darwin*)
      TARGET_DIR="$HOME/Library/Application Support/Nori/Models"
      ;;
    Linux*)
      DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}"
      TARGET_DIR="$DATA_DIR/nori/Models"
      ;;
    MINGW*|MSYS*|CYGWIN*)
      if [[ -n "${LOCALAPPDATA:-}" ]]; then
        TARGET_DIR="$(cygpath -u "$LOCALAPPDATA")/Nori/Models"
      else
        TARGET_DIR="$HOME/.local/share/nori/Models"
      fi
      ;;
    *)
      TARGET_DIR="$HOME/.local/share/nori/Models"
      ;;
  esac
fi

mkdir -p "$TARGET_DIR"
echo "==> Nori Models destination: $TARGET_DIR"

# Hugging Face local cache snapshot location
HF_CACHE_DIR="${HF_HUB_CACHE:-${HF_HOME:-$HOME/.cache/huggingface}/hub}"
REPO_DIRNAME="models--$(echo "$REPO" | sed 's/\//--/g')"
SNAPSHOT_DIR="$HF_CACHE_DIR/$REPO_DIRNAME/snapshots/$REV"

calc_sha256() {
  local file="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$file" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$file" | awk '{print $1}'
  else
    python3 -c "import hashlib, sys; print(hashlib.sha256(open(sys.argv[1], 'rb').read()).hexdigest())" "$file"
  fi
}

fetch_file() {
  local name="$1"
  local expected_bytes="$2"
  local expected_sha="$3"
  local dest="$TARGET_DIR/$name"
  local partial="$TARGET_DIR/${name}.partial"
  local url="$BASE_URL/$name"

  # Check if already present and valid
  if [[ -f "$dest" ]]; then
    local actual_size
    actual_size=$(wc -c < "$dest" | tr -d ' ')
    if [[ "$actual_size" -eq "$expected_bytes" ]]; then
      echo "--> $name exists with valid size ($actual_size bytes). Verifying SHA-256..."
      local actual_sha
      actual_sha=$(calc_sha256 "$dest")
      if [[ "$actual_sha" == "$expected_sha" ]]; then
        echo "--> [OK] $name verified."
        return 0
      else
        echo "--> [WARNING] Checksum mismatch for $dest. Re-downloading..."
        rm -f "$dest"
      fi
    fi
  fi

  # Check if present in Hugging Face Hub cache
  local cached="$SNAPSHOT_DIR/$name"
  if [[ -f "$cached" ]]; then
    local cached_size
    cached_size=$(wc -c < "$cached" | tr -d ' ')
    if [[ "$cached_size" -eq "$expected_bytes" ]]; then
      echo "--> Found $name in Hugging Face cache ($cached). Verifying..."
      local cached_sha
      cached_sha=$(calc_sha256 "$cached")
      if [[ "$cached_sha" == "$expected_sha" ]]; then
        echo "--> Copying from cache to $dest..."
        cp "$cached" "$dest"
        echo "--> [OK] $name copied and verified."
        return 0
      fi
    fi
  fi

  # Download via curl (resumable)
  echo "--> Downloading $name (~$(( expected_bytes / 1024 / 1024 )) MB)..."
  curl -L -C - --progress-bar -o "$partial" "$url"

  echo "--> Verifying SHA-256 checksum for $name..."
  local downloaded_sha
  downloaded_sha=$(calc_sha256 "$partial")
  if [[ "$downloaded_sha" != "$expected_sha" ]]; then
    echo "--> [ERROR] SHA-256 verification failed for $name!" >&2
    echo "    Expected: $expected_sha" >&2
    echo "    Actual:   $downloaded_sha" >&2
    rm -f "$partial"
    return 1
  fi

  mv "$partial" "$dest"
  echo "--> [OK] $name successfully downloaded and verified."
}

echo ""
echo "=== Step 1/2: Text Model ($TEXT_NAME) ==="
fetch_file "$TEXT_NAME" "$TEXT_BYTES" "$TEXT_SHA"

echo ""
echo "=== Step 2/2: Multimodal Vision/Audio Projector ($MEDIA_NAME) ==="
fetch_file "$MEDIA_NAME" "$MEDIA_BYTES" "$MEDIA_SHA"

echo ""
echo "All Nori models are installed and ready at: $TARGET_DIR"
