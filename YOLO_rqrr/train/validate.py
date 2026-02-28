#!/usr/bin/env python3
"""
validate.py — Evaluate QR detection model on validation set.

Reports mAP, precision, recall, and per-image inference speed.
Can validate either the .pt or .onnx model.

Usage:
    python validate.py                              # validate best.pt
    python validate.py --weights ../models/yolo12n-qr.onnx  # validate ONNX
"""

import argparse
from pathlib import Path
from ultralytics import YOLO


def parse_args():
    parser = argparse.ArgumentParser(description="Validate QR detection model")
    parser.add_argument(
        "--weights", type=str,
        default="runs/detect/train/weights/best.pt",
        help="Model weights (.pt or .onnx)"
    )
    parser.add_argument(
        "--imgsz", type=int, default=640,
        help="Input image size (default: 640)"
    )
    parser.add_argument(
        "--device", type=str, default=None,
        help="Device (default: auto)"
    )
    return parser.parse_args()


def main():
    args = parse_args()
    dataset_yaml = Path(__file__).parent / "dataset.yaml"

    if not Path(args.weights).exists():
        print(f"[validate] ERROR: Weights not found: {args.weights}")
        return

    print("=" * 60)
    print("QR Detection Model Validation")
    print("=" * 60)
    print(f"  Weights: {args.weights}")
    print()

    model = YOLO(args.weights)

    val_kwargs = dict(
        data=str(dataset_yaml),
        imgsz=args.imgsz,
        verbose=True,
    )

    if args.device is not None:
        val_kwargs["device"] = args.device

    metrics = model.val(**val_kwargs)

    print()
    print("=" * 60)
    print("Validation Results")
    print("=" * 60)
    print(f"  mAP@50:     {metrics.box.map50:.4f}")
    print(f"  mAP@50-95:  {metrics.box.map:.4f}")
    print(f"  Precision:  {metrics.box.mp:.4f}")
    print(f"  Recall:     {metrics.box.mr:.4f}")
    print()

    # For QR detection, we care most about recall (don't miss QR codes)
    # and speed (needs to run in browser at interactive rates)
    if metrics.box.mr < 0.8:
        print("⚠️  Recall < 0.8 — model may miss QR codes.")
        print("   Consider: more training data, more epochs, or")
        print("   adding Kipukas-specific images to the dataset.")
    else:
        print("✅  Recall looks good for QR detection.")


if __name__ == "__main__":
    main()
