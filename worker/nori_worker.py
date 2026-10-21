"""Nori embedding worker: EmbeddingGemma 2 on MLX (mlx-vlm) or PyTorch/Transformers.

Protocol: one JSON object per line on stdin, one JSON reply per line on stdout. Logs go to stderr.
The first line the worker prints is {"ready": true, "info": {...}} once the model is loaded.

Requests:
  {"id": 1, "op": "info"}
  {"id": 2, "op": "text",  "inputs": ["task: search result | query: cat", ...]}
  {"id": 3, "op": "image", "inputs": ["/path/to/a.jpg", ...], "budget": 280}
  {"id": 4, "op": "audio", "inputs": ["/path/to/a.wav", ...]}
  {"id": 5, "op": "quit"}

Reply:
  {"id": 2, "ok": true, "dim": 768, "count": N, "vectors": "<base64 float32 LE, N x dim>", "errors": [...]}
  A failed item gets a zero vector and a message in "errors"; a failed request gets {"ok": false, "error": "..."}.
  Text inputs arrive already prompted ("task: search result | query: ..."); prompts are the caller's business.
"""

import argparse
import base64
import json
import os
import sys
import time
from pathlib import Path

import numpy as np

DIM = 768
BATCH = {"text": 16, "image": 4, "audio": 2}
BUDGETS = (70, 140, 280, 560, 1120)


def log(*parts):
    print("[nori-worker]", *parts, file=sys.stderr, flush=True)


def resolve_model(model):
    if os.path.isdir(model):
        return model
    from huggingface_hub import snapshot_download

    try:
        return snapshot_download(model, local_files_only=True)
    except Exception:
        log(f"downloading {model} (first run only)")
        return snapshot_download(model)


def get_runtime_fingerprint():
    """Returns the backend and library versions for indexing fingerprint."""
    try:
        import mlx.core as mx
        from importlib import metadata

        dist = metadata.distribution("mlx-vlm")
        commit = ""
        try:
            direct = json.loads(dist.read_text("direct_url.json") or "{}")
            c = direct.get("vcs_info", {}).get("commit_id")
            if c:
                commit = f"-git-{c[:7]}"
        except Exception:
            pass
        return f"mlx-{mx.__version__}+mlx-vlm-{dist.version}{commit}"
    except ImportError:
        try:
            import torch
            import transformers

            device = "cuda" if torch.cuda.is_available() else ("mps" if hasattr(torch.backends, "mps") and torch.backends.mps.is_available() else "cpu")
            return f"torch-{torch.__version__}+{device}+transformers-{transformers.__version__}"
        except ImportError:
            return "unknown"


class MlxEmbedder:
    def __init__(self, model_name, text_only):
        import mlx.core as mx

        self.mx = mx
        self.text_only = text_only
        path = resolve_model(model_name)
        started = time.perf_counter()

        if text_only:
            from mlx_vlm.embedding_loader import load_embedding_model
            from mlx_vlm.utils import load_config
            from transformers import AutoTokenizer

            config = load_config(path)
            config["audio_config"] = None
            config["vision_config"] = None
            self.model = load_embedding_model(Path(path), config=config)
            self.tokenizer = AutoTokenizer.from_pretrained(path)
            self.processor = None
        else:
            from mlx_vlm import load

            self.model, self.processor = load(path)
            self.tokenizer = self.processor.tokenizer

        mx.eval(self.model.parameters())
        mx.set_cache_limit(512 * 1024 * 1024)

        if text_only:
            for _ in range(2):
                self._vectors(
                    self.model(
                        **{
                            k: mx.array(v)
                            for k, v in self.tokenizer(
                                ["task: search result | query: warm up"], return_tensors="np"
                            ).items()
                        }
                    )
                )

        self.info = {
            "model": model_name,
            "revision": Path(path).name,
            "runtime": get_runtime_fingerprint(),
            "dtype": "bf16",
            "dim": DIM,
            "text_only": text_only,
            "load_seconds": round(time.perf_counter() - started, 2),
        }

    def _vectors(self, output):
        return np.array(output.text_embeds.astype(self.mx.float32))

    def text(self, inputs):
        rows = []
        for i in range(0, len(inputs), BATCH["text"]):
            batch = inputs[i : i + BATCH["text"]]
            tokens = self.tokenizer(
                batch, padding=True, truncation=True, max_length=2048, return_tensors="np"
            )
            rows.append(self._vectors(self.model(**{k: self.mx.array(v) for k, v in tokens.items()})))
        return np.concatenate(rows), [None] * len(inputs)

    def media(self, kind, inputs, budget=None):
        if self.processor is None:
            raise RuntimeError("this worker was started --text-only")
        if kind == "image":
            budget = budget or 280
            if budget not in BUDGETS:
                raise ValueError(f"budget must be one of {BUDGETS}")
            self.processor.image_processor.max_soft_tokens = budget
        rows, errors = [], []
        size = BATCH[kind]
        for i in range(0, len(inputs), size):
            batch = inputs[i : i + size]
            try:
                rows.append(self._media_batch(kind, batch))
                errors += [None] * len(batch)
            except Exception:
                for path in batch:
                    try:
                        rows.append(self._media_batch(kind, [path]))
                        errors.append(None)
                    except Exception as item_error:
                        rows.append(np.zeros((1, DIM), np.float32))
                        errors.append(f"{type(item_error).__name__}: {item_error}"[:300])
        return np.concatenate(rows), errors

    def _media_batch(self, kind, paths):
        conversations = [[{"role": "user", "content": [{"type": kind, "url": p}]}] for p in paths]
        inputs = self.processor.apply_chat_template(
            conversations, tokenize=True, return_dict=True, return_tensors="mlx"
        )
        return self._vectors(self.model(**inputs))


class TorchEmbedder:
    """Fallback PyTorch/Transformers embedder for non-Apple Silicon systems."""

    def __init__(self, model_name, text_only):
        import torch
        from transformers import AutoModel, AutoTokenizer

        self.torch = torch
        self.text_only = text_only
        self.device = "cuda" if torch.cuda.is_available() else "cpu"
        path = resolve_model(model_name)
        started = time.perf_counter()

        self.tokenizer = AutoTokenizer.from_pretrained(path)
        self.model = AutoModel.from_pretrained(path, trust_remote_code=True).to(self.device)
        self.model.eval()

        self.info = {
            "model": model_name,
            "revision": Path(path).name,
            "runtime": get_runtime_fingerprint(),
            "dtype": "fp32" if self.device == "cpu" else "fp16",
            "dim": DIM,
            "text_only": text_only,
            "load_seconds": round(time.perf_counter() - started, 2),
        }

    def text(self, inputs):
        rows = []
        with self.torch.no_grad():
            for i in range(0, len(inputs), BATCH["text"]):
                batch = inputs[i : i + BATCH["text"]]
                tokens = self.tokenizer(
                    batch, padding=True, truncation=True, max_length=2048, return_tensors="pt"
                ).to(self.device)
                outputs = self.model(**tokens)
                if hasattr(outputs, "text_embeds"):
                    embeds = outputs.text_embeds
                elif hasattr(outputs, "last_hidden_state"):
                    embeds = outputs.last_hidden_state[:, 0, :]
                else:
                    embeds = outputs[0][:, 0, :]
                rows.append(embeds.cpu().to(self.torch.float32).numpy())
        return np.concatenate(rows), [None] * len(inputs)

    def media(self, kind, inputs, budget=None):
        raise NotImplementedError("multimodal embeddings with PyTorch require mlx on Apple Silicon or llama.cpp backend")


def create_embedder(model_name, text_only):
    try:
        import mlx.core
        return MlxEmbedder(model_name, text_only)
    except ImportError:
        return TorchEmbedder(model_name, text_only)


def reply(message):
    sys.stdout.write(json.dumps(message, separators=(",", ":")) + "\n")
    sys.stdout.flush()


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--model", default="google/embeddinggemma-2")
    parser.add_argument("--text-only", action="store_true", help="load only text part")
    args = parser.parse_args()

    try:
        embedder = create_embedder(args.model, args.text_only)
    except Exception as error:
        reply({"ready": False, "error": f"{type(error).__name__}: {error}"})
        return 1

    log(f"ready in {embedder.info['load_seconds']} s ({'text-only' if args.text_only else 'full'})")
    reply({"ready": True, "info": embedder.info})

    for line in sys.stdin:
        if not line.strip():
            continue
        request_id = None
        try:
            request = json.loads(line)
            request_id, op = request.get("id"), request.get("op")
            if op == "quit":
                reply({"id": request_id, "ok": True})
                break
            if op == "info":
                reply({"id": request_id, "ok": True, "info": embedder.info})
                continue
            inputs = request.get("inputs") or []
            if op == "text":
                vectors, errors = embedder.text(inputs)
            elif op in ("image", "audio"):
                vectors, errors = embedder.media(op, inputs, request.get("budget"))
            else:
                raise ValueError(f"unknown op {op!r}")
            reply({
                "id": request_id,
                "ok": True,
                "dim": DIM,
                "count": len(inputs),
                "vectors": base64.b64encode(vectors.astype("<f4").tobytes()).decode("ascii"),
                "errors": errors,
            })
        except Exception as error:
            reply({"id": request_id, "ok": False, "error": f"{type(error).__name__}: {error}"[:500]})
    return 0


if __name__ == "__main__":
    sys.exit(main())
