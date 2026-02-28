# YOLO_rqrr — Two-Stage QR Detection & Decode Pipeline

> **YOLOv12n** (attention-centric object detector) → **rqrr** (Rust QR decoder)  
> Designed for Kipukas' anti-cheat camouflaged QR codes on mobile browsers.

## Architecture

```
Camera Frame (640×480 RGBA)
    │
    ▼
┌──────────────────────────────────┐
│  Stage 1: YOLOv12n Detection     │  ONNX Runtime Web (WebGPU / WASM)
│  ─ Locates QR bounding box(es)  │  ~5MB model, ~30-80ms on mobile GPU
│  ─ Attention mechanism handles   │
│    camouflage, tilt, low-res     │
└──────────────┬───────────────────┘
               │ bbox crop (200×200)
               ▼
┌──────────────────────────────────┐
│  Stage 2: rqrr Decode            │  Rust/WASM (~80KB)
│  ─ Preprocessing cascade on      │  ~2-5ms per strategy
│    tight crop (much higher        │
│    effective resolution)          │
│  ─ adaptive_thresh, CLAHE, etc.  │
└──────────────┬───────────────────┘
               │ decoded URL string
               ▼
         QR_FOUND → htmx → redirect
```

## Why Two Stages?

The single-stage approach (rqrr with 28 preprocessing strategies on full frames)
fails because **rqrr's finder pattern detection can't locate QR codes through
Kipukas' cracked-lava SVG camouflage texture**. No amount of preprocessing fixes
a detector that can't see the three position squares.

YOLOv12n **learns** what camouflaged QR codes look like. Its Area Attention
mechanism gives it a global receptive field — it understands the whole region
contextually, not just edges and corners. Once YOLO provides a tight bounding
box, rqrr gets a clean, high-effective-resolution crop where its preprocessing
cascade becomes highly effective.

## Project Structure

```
YOLO_rqrr/
├── train/                     # Python — YOLO training & export
│   ├── requirements.txt       # ultralytics, torch, onnx
│   ├── dataset.yaml           # QR detection dataset config
│   ├── fetch_dataset.py       # Download public QR datasets
│   ├── train.py               # Fine-tune YOLOv12n on QR detection
│   ├── export_onnx.py         # Export → ONNX (opset 12, WebGPU compat)
│   └── validate.py            # Evaluate model performance
│
├── rqrr-wasm/                 # Rust — QR decode WASM crate
│   ├── Cargo.toml
│   └── src/lib.rs             # decode_qr_crop(rgba, w, h) → String
│
├── web/                       # JS — Browser runtime & test harness
│   ├── package.json           # onnxruntime-web
│   ├── src/
│   │   ├── yolo-inference.js  # ONNX session (WebGPU → WASM fallback)
│   │   ├── postprocess.js     # YOLO output → bboxes (NMS)
│   │   └── yolo-rqrr-worker.js # Web Worker orchestrating both stages
│   ├── index.html             # Test harness with live camera
│   └── benchmark.html         # Performance measurement
│
├── models/                    # Exported models (gitignored)
│   └── README.md
│
├── data/                      # Training data (gitignored)
│
└── scripts/
    ├── build-rqrr-wasm.sh     # wasm-pack build
    └── integrate.sh           # Copy artifacts to main site
```

## Quick Start

### 1. Train the model (Python)
```bash
cd train
pip install -r requirements.txt
python fetch_dataset.py        # download QR detection dataset
python train.py                # fine-tune YOLOv12n
python export_onnx.py          # export to ONNX
```

### 2. Build rqrr WASM (Rust)
```bash
cd rqrr-wasm
wasm-pack build --target web --release
```

### 3. Run test harness (JS)
```bash
cd web
npm install
npm run dev
```

## License

- **YOLOv12**: AGPL-3.0 (Ultralytics) — training pipeline and exported model
- **rqrr**: MIT/Apache-2.0
- **ONNX Runtime Web**: MIT
- **This integration code**: AGPL-3.0 (to satisfy YOLO's copyleft)

Per AGPL requirements, the complete QR detection component will be published
as a separate public repository when integrated into the Kipukas production site.
