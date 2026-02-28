#!/usr/bin/env python3
"""
export_onnx.py — Export trained YOLOv12n to ONNX for browser deployment.

Exports with opset=12 and dynamic shapes for WebGPU compatibility
via ONNX Runtime Web. The exported model does NOT require FlashAttention.

Usage:
    python export_onnx.py                              # default: best.pt
    python export_onnx.py --weights path/to/model.pt   # custom weights
    python export_onnx.py --half                        # FP16 (smaller file)

Output:
    ../models/yolo12n-qr.onnx
"""

import argparse
import shutil
from pathlib import Path
from ultralytics import YOLO


def parse_args():
    parser = argparse.ArgumentParser(description="Export YOLOv12n to ONNX")
    parser.add_argument(
        "--weights", type=str,
        default="runs/detect/train/weights/best.pt",
        help="Path to trained weights (default: runs/detect/train/weights/best.pt)"
    )
    parser.add_argument(
        "--half", action="store_true",
        help="Export as FP16 (smaller model, ~5MB vs ~10MB)"
    )
    parser.add_argument(
        "--imgsz", type=int, default=640,
        help="Input image size (default: 640)"
    )
    return parser.parse_args()


def main():
    args = parse_args()
    weights_path = Path(args.weights)
    output_dir = Path(__file__).parent.parent / "models"
    output_dir.mkdir(exist_ok=True)

    if not weights_path.exists():
        print(f"[export] ERROR: Weights not found: {weights_path}")
        print("[export] Run train.py first, or specify --weights path.")
        return

    print("=" * 60)
    print("ONNX Export for Browser Deployment")
    print("=" * 60)
    print(f"  Weights:  {weights_path}")
    print(f"  Format:   {'FP16' if args.half else 'FP32'}")
    print(f"  Img size: {args.imgsz}")
    print()

    model = YOLO(str(weights_path))

    # Export to ONNX with settings optimized for ONNX Runtime Web
    # opset=12: required for WebGPU compatibility
    # dynamic=True: allows variable input sizes
    # simplify=True: optimizes the graph for inference
    export_path = model.export(
        format="onnx",
        opset=12,
        dynamic=True,
        simplify=True,
        half=args.half,
        imgsz=args.imgsz,
    )

    # Copy to models/ directory with standard name
    suffix = "-fp16" if args.half else ""
    dest = output_dir / f"yolo12n-qr{suffix}.onnx"
    shutil.copy2(export_path, dest)

    size_mb = dest.stat().st_size / (1024 * 1024)

    print()
    print("=" * 60)
    print("Export Complete")
    print("=" * 60)
    print(f"  ONNX model: {dest}")
    print(f"  Size:       {size_mb:.1f} MB")
    print()
    print("Next steps:")
    print("  1. python validate.py          # verify model accuracy")
    print("  2. cd ../web && npm run dev     # test in browser")


if __name__ == "__main__":
    main()
