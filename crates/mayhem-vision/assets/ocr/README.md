# Bundled OCR model

| File | What it is |
|---|---|
| `ppocrv4_rec_ch.onnx` | PaddleOCR **PP-OCRv4** text *recognition* model (Chinese + Latin letters, digits), ONNX export |
| `ppocrv4_rec_ch.dict.txt` | Its character list (6,623 entries), extracted from the model's `character` metadata |

- **Source**: PaddlePaddle/PaddleOCR, as exported to ONNX and shipped in the `rapidocr-onnxruntime`
  1.4.4 wheel on PyPI (`rapidocr_onnxruntime/models/ch_PP-OCRv4_rec_infer.onnx`).
- **Licence**: Apache-2.0 (PaddleOCR and RapidOCR).
- **SHA-256** (`ppocrv4_rec_ch.onnx`): `48fc40f24f6d2a207a2b1091d3437eb3cc3eb6b676dc3ef9c37384005483683b`
- **Input**: `x`, float32 `[N, 3, 48, W]`, RGB scaled to `(v / 255 - 0.5) / 0.5`.
- **Output**: softmax `[N, W/8, 6625]`; index 0 is the CTC blank, 1..=6623 the dictionary, 6624 a space.

Only recognition is needed: the app already knows where each title is (the card layout), so no
text-detection model is bundled. It runs in pure Rust through `tract`, with no native runtime.
