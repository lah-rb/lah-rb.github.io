#!/usr/bin/env python3
"""
train.py — Fine-tune YOLOv12n for QR code detection.

Uses Ultralytics YOLO API with the sunsmarterjie/yolov12 architecture.
Trains on the QR detection dataset prepared by fetch_dataset.py.

The nano (n) variant is chosen for browser deployment:
  - 2.6M parameters, ~6.5 GFLOPs
  - ~1.64ms inference on T4 TensorRT
  - ~5-10MB ONNX model size

Usage:
    python train.py                    # default: 100 epochs, auto device
    python train.py --epochs 200       # more training
    python train.py --device cpu       # force CPU training
    python train.py --resume           # resume from last checkpoint

Output:
    runs/detect/train/weights/best.pt  # best checkpoint
    runs/detect/train/weights/last.pt  # last checkpoint
"""

import argparse
from pathlib import Path
from ultralytics import YOLO


def parse_args():
    parser = argparse.ArgumentParser(description="Train YOLOv12n for QR detection")
    parser.add_argument(
        "--epochs", type=int, default=100,
        help="Number of training epochs (default: 100)"
    )
    parser.add_argument(
        "--batch", type=int, default=-1,
        help="Batch size (-1 for auto, default: -1)"
    )
    parser.add_argument(
        "--imgsz", type=int, default=640,
        help="Input image size (default: 640)"
    )
    parser.add_argument(
        "--device", type=str, default=None,
        help="Device: 'cpu', '0', '0,1', etc. (default: auto)"
    )
    parser.add_argument(
        "--resume", action="store_true",
        help="Resume training from last checkpoint"
    )
    parser.add_argument(
        "--pretrained", type=str, default="yolo12n.pt",
        help="Pretrained weights to fine-tune from (default: yolo12n.pt)"
    )
    return parser.parse_args()


def main():
    args = parse_args()
    dataset_yaml = Path(__file__).parent / "dataset.yaml"

    if not dataset_yaml.exists():
        print("[train] ERROR: dataset.yaml not found. Run fetch_dataset.py first.")
        return

    # Verify data directory exists
    data_dir = Path(__file__).parent.parent / "data"
    if not (data_dir / "images" / "train").exists():
        print("[train] ERROR: Training data not found. Run fetch_dataset.py first.")
        return

    print("=" * 60)
    print("YOLOv12n QR Detection Training")
    print("=" * 60)
    print(f"  Pretrained: {args.pretrained}")
    print(f"  Epochs:     {args.epochs}")
    print(f"  Image size: {args.imgsz}")
    print(f"  Batch:      {'auto' if args.batch == -1 else args.batch}")
    print(f"  Device:     {args.device or 'auto'}")
    print(f"  Dataset:    {dataset_yaml}")
    print()

    if args.resume:
        # Resume from last checkpoint
        model = YOLO("runs/detect/train/weights/last.pt")
        print("[train] Resuming from last checkpoint...")
    else:
        # Load pretrained YOLOv12n (downloads automatically from Ultralytics hub)
        model = YOLO(args.pretrained)
        print(f"[train] Loaded pretrained {args.pretrained}")

    # Train configuration optimized for small dataset fine-tuning
    # These settings follow the YOLOv12 paper's recommendations for nano model
    train_kwargs = dict(
        data=str(dataset_yaml),
        epochs=args.epochs,
        imgsz=args.imgsz,
        batch=args.batch,
        # Augmentation — moderate for QR codes
        # QR codes have specific geometry, so we don't want too much distortion
        scale=0.5,        # scale augmentation
        mosaic=1.0,       # mosaic augmentation (helpful for small objects)
        mixup=0.0,        # no mixup (QR is single-class, doesn't benefit)
        copy_paste=0.1,   # light copy-paste augmentation
        flipud=0.0,       # no vertical flip (QR codes are orientation-sensitive)
        fliplr=0.5,       # horizontal flip is fine
        degrees=15.0,     # moderate rotation (QR codes can be tilted)
        perspective=0.001, # slight perspective (simulates phone tilt)
        # Training params
        optimizer="AdamW",
        lr0=0.001,
        lrf=0.01,         # final LR = lr0 * lrf
        warmup_epochs=5,
        # Output
        project="runs/detect",
        name="train",
        exist_ok=True,
        verbose=True,
    )

    if args.device is not None:
        train_kwargs["device"] = args.device

    results = model.train(**train_kwargs)

    # Print final metrics
    print()
    print("=" * 60)
    print("Training Complete")
    print("=" * 60)
    print(f"  Best weights: runs/detect/train/weights/best.pt")
    print()
    print("Next step: python export_onnx.py")


if __name__ == "__main__":
    main()
