# YOLO_rqrr — Quick and dirty WASM QR Detection Experiment

## Notice
While the results of the experiment might be valuable in informing a proper project, this is beyond my capacity at this time. It is provided as is without warrenty or guarentee of reproducibility.

> **YOLOv12n** (attention-centric object detector) → **ZXing** (C++/WASM barcode decoder) or rqrr (rust/WASM QR decoder)
> Tested and trained against Kipukas' anti-cheat camouflaged QR codes on mobile browsers.

## Experiment Summary

This directory contains the complete experiment exploring whether YOLOv12's
attention mechanism, trained on a custom QR code dataset, could improve QR
detection speed and accuracy for Kipukas' camouflaged QR codes on low-res
front-facing cameras. This was tested on google pixel 3a, google pixel 4a, google pixel 9, iphone 6s, iphone 14 pro, samsung galaxy s21, and samsung galaxy book 3. YOLOv12n displays good tracking on all capable devices. General detection was best with ZXing compiled to WASM in good environmental conditions. Additionally, it is compatible with the older devices and devices which do not yet support WebGPU. In poor conditions, YOLOv12n driven cropping + adaptive threshold preprocessing yelds positive detection results with both ZXing and rqrr backends. While not as performant as ZXing, YOLOv12n + rqrr + at_21 was able to decode reliably and would be judged sufficient for more standard QR workflows. However, its main benefit (small compared to ZXing) is deminished when paired with YOLOv12n (~10x larger than ZXing).

### Findings

| Approach | Result |
|----------|--------|
| **rqrr alone** (28 preprocessing strategies on full frame) | ⚠️ very slow w. adaptive threashold, Fails wo.— finder pattern detection can't see through SVG camouflage |
| **ZXing-only (std-WASM/CDN)** (no YOLO, full-frame scan) | ⚠️ very slow/fails to detect on most devices |
| **YOLOv12n + rqrr** | ⚠️ slow but functional on powerful devices |
| **YOLO v12n + ZXing** (two-stage: detect → crop → decode) | ✅ Works — YOLO learns camouflage patterns, ZXing decodes clean crops, bogs older devices |
| **YOLO on WASM/CPU** | ⚠️ Slow but functional on powerful devices (laptops) |
| **YOLO on WebGPU** | ✅ Fast on supported mobile GPUs |
| **ZXing-only (gcc17, compiled in house)** (no YOLO, full-frame scan) | ✅ Works very fast with close shots and good environment on all devices |

### Key Decisions

1. **User-controlled CV toggle** — Auto-detecting WebGPU capability is unreliable.
   Some older devices report WebGPU but perform poorly; some without WebGPU have
   CPUs powerful enough for WASM inference. The chip icon (⬜ off / 🟩 on) in the
   scanner UI lets users opt in to YOLO. Default: ZXing-only.

2. **ZXing replaced rqrr for decode** — rqrr is a Rust QR decoder, but ZXing
   (C++/WASM) with `tryHarder` mode proved more reliable for decoded crops and has
   a mature WASM distribution. rqrr remains in the repo for reference.

3. **Square 640×640 capture** — Camera canvas matches YOLO's native input resolution.
   No letterbox distortion, what the user sees is exactly what the decoder receives.

4. **Otsu removed from augmentation** — Otsu's global threshold washes out the
   reflective surfaces on physical cards, producing all-white training images.
   9 transforms remain (adaptive_thresh ×3, CLAHE, blur+AT, contrast_stretch+AT,
   yellow-aware, gaussian noise, JPEG compression).

5. **5-second eager preload** — ONNX model + ZXing WASM load asynchronously 5s
   after page load, so the scanner feels instant when opened.

## Architecture

```
Camera Frame (640×640 RGBA, 1:1 square)
    │
    ├─── CV OFF (default) ──────────────────────┐
    │                                            ▼
    │                                   ┌─────────────────┐
    │                                   │  ZXing Decode    │
    │                                   │  Full-frame scan │
    │                                   └────────┬────────┘
    │                                            │
    ├─── CV ON (user toggle) ───┐                │
    │                           ▼                │
    │               ┌───────────────────────┐    │
    │               │  Stage 1: YOLOv12n    │    │
    │               │  ONNX Runtime Web     │    │
    │               │  (WebGPU → WASM)      │    │
    │               │  ~30-80ms (GPU)       │    │
    │               │  ~1-4s (CPU)          │    │
    │               └───────────┬───────────┘    │
    │                           │ bbox crop      │
    │                           ▼                │
    │               ┌───────────────────────┐    │
    │               │  Stage 2: ZXing       │    │
    │               │  Decode cropped ROI   │    │
    │               │  tryHarder mode       │    │
    │               └───────────┬───────────┘    │
    │                           │                │
    └───────────────────────────┴────────────────┘
                                │
                                ▼
                    decoded URL → WASM server
                    → validation → redirect
```

## Why Two Stages?

The single-stage approach (rqrr with preprocessing on full frames) fails because
**rqrr/ZXing finder pattern detection can't locate QR codes through Kipukas'
cracked-lava SVG camouflage texture**. No amount of image preprocessing fixes a
decoder that can't see the three position squares in a noisy full-resolution frame.

YOLOv12n **learns** what camouflaged QR codes look like. Its Area Attention
mechanism gives it a global receptive field — it understands the whole region
contextually, not just edges and corners. Once YOLO provides a tight bounding
box, ZXing gets a clean, high-effective-resolution crop where decode becomes
highly reliable.

## Training Pipeline

### Dataset

- **70 Kipukas captures** (`kipukas-qr-dataset-70imgs/`) — annotated in the
  custom annotator (`annotator/index.html`), captured at 1280×720 from the
  scanner's front-facing camera with real printed camouflaged cards
- **kolabit public dataset** (`data/`) — ~600 general QR code images for
  diversity, fetched via `train/fetch_dataset.py`
- **9 augmentation transforms** applied to Kipukas images via `train/augment.py`:
  1. `at15` — adaptive threshold (block=15, c=8)
  2. `at11` — adaptive threshold fine (block=11, c=6)
  3. `at21` — adaptive threshold coarse (block=21, c=10)
  4. `clahe` — CLAHE (4×4 tiles, clip=2.0)
  5. `blur_at` — Gaussian blur + adaptive threshold
  6. `stretch_at` — contrast stretch + adaptive threshold
  7. `yellow` — yellow-aware channel (max(R,G) - B) for anti-camouflage
  8. `noise15` — Gaussian noise (σ=15)
  9. `jpeg35` — JPEG compression artifacts (quality=35)

### Training

```bash
cd YOLO_rqrr

# 1. Augment local QR + merge with kolabit
uv run python train/augment.py

# 2. Train YOLOv12n (100 epochs, MPS on Apple Silicon)
uv run python train/train.py --epochs 100 --device mps

# 3. Export to ONNX (opset 12, WebGPU compatible)
uv run python train/export_onnx.py --weights /Users/lah-rb/Repos/lah-rb.github.io/runs/detect/runs/detect/train/weights/best.pt

# 4. Copy model to site assets
cp models/yolo12n-qr.onnx ../assets/js-wasm/yolo12n-qr.onnx
```

### Validation

```bash
uv run python train/validate.py
```

## Runtime Integration

### Files in the main site

| File | Purpose |
|------|---------|
| `assets/js/yolo-inference.js` | ONNX Runtime Web session (WebGPU → WASM fallback) |
| `assets/js/postprocess.js` | YOLO output → bboxes (NMS, confidence threshold) |
| `assets/js/zxing-decode.js` | ZXing C++/WASM barcode decoder |
| `assets/js/kipukas-worker.js` | Web Worker orchestrating YOLO+ZXing or ZXing-only |
| `assets/js/kipukas-api.js` | 5s delayed PRELOAD_QR, CV preference relay |
| `assets/js/qr-camera.js` | Camera capture, frame relay, bbox overlay |
| `kipukas-server/src/routes/qr.rs` | Scanner UI HTML (Rust/WASM), CV toggle button |
| `assets/js-wasm/yolo12n-qr.onnx` | Exported YOLO model (~5MB) |

### CV Toggle Flow

```
User taps chip icon in scanner UI
    → Alpine toggles cvOn state
    → localStorage.setItem('kipukas-cv-enabled', true/false)
    → kipukasWorker.postMessage({ type: 'SET_CV_MODE', enabled })
    → Worker updates qrMode:
        ON:  resets qrReady, next frame triggers YOLO init
        OFF: switches to 'zxing-only' (YOLO session stays loaded but unused)
```

### Preload Flow

```
Page loads → kipukas-api.js spawns worker
    → 5s timeout fires
    → Reads localStorage('kipukas-cv-enabled')
    → Sends PRELOAD_QR { cvEnabled } to worker
    → Worker inits:
        cvEnabled=true:  YOLO (WebGPU→WASM) + ZXing in parallel
        cvEnabled=false: ZXing only
```

## Project Structure

```
YOLO_rqrr/
├── README.md                  # This file
├── pyproject.toml             # Python project config (uv)
├── .python-version
│
├── train/                     # Python — Training pipeline
│   ├── augment.py             # Kipukas augmentation + dataset merge
│   ├── train.py               # Fine-tune YOLOv12n
│   ├── export_onnx.py         # Export → ONNX (opset 12)
│   ├── validate.py            # Evaluate model performance
│   ├── fetch_dataset.py       # Download kolabit public dataset
│   ├── dataset.yaml           # Auto-generated dataset config
│   └── requirements.txt       # ultralytics, torch, onnx
│
├── annotator/                 # Browser-based bbox annotation tool
│   └── index.html             # Capture + annotate QR bounding boxes
│
├── kipukas-qr-dataset-70imgs/ # Annotated Kipukas captures
│   ├── images/train/          # 70 JPG captures from scanner camera
│   └── labels/train/          # YOLO-format label files
│
├── data/                      # kolabit dataset (gitignored)
├── data-augmented/            # Merged augmented dataset (gitignored)
│
├── rqrr-wasm/                 # Rust QR decode WASM crate (reference)
│   ├── Cargo.toml
│   └── src/lib.rs
│
├── web/                       # Standalone test harness
│   ├── src/
│   │   ├── yolo-inference.js
│   │   ├── postprocess.js
│   │   └── yolo-rqrr-worker.js
│   └── index.html
│
├── models/                    # Exported models (gitignored)
├── scripts/
│   ├── build-rqrr-wasm.sh
│   └── integrate.sh
│
├── train_100epoch.log         # Training logs
├── train_320_fp16.log
└── train_augmented.log
```

## License

- **YOLOv12**: AGPL-3.0 (Ultralytics) — training pipeline and exported model
- **ZXing-cpp**: Apache-2.0
- **rqrr**: MIT/Apache-2.0
- **ONNX Runtime Web**: MIT
- **This integration code**: AGPL-3.0 (to satisfy YOLO's copyleft)

Per AGPL requirements, the complete QR detection component is published in
this public repository alongside the Kipukas production site.
