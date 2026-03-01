#!/usr/bin/env python3
"""
augment.py — Augment Kipukas QR dataset using the rqrr preprocessing cascade.

Applies the same image transforms that rqrr uses to decode QR codes,
so YOLO learns to detect QR codes under exactly the conditions that
rqrr can successfully decode them.

Transforms (from rqrr-wasm/src/lib.rs):
  1. adaptive_thresh (block=15, c=8)
  2. adaptive_thresh_fine (block=11, c=6)
  3. adaptive_thresh_coarse (block=21, c=10)
  4. clahe (4x4 tiles, clip=2.0)
  5. blur + adaptive_thresh
  6. contrast_stretch + adaptive_thresh
  7. yellow_aware (max(R,G) - B)
  8. otsu threshold
Plus standard noise augmentations:
  9. gaussian_noise (sigma=15)
  10. jpeg_compression (quality=35)

Bounding boxes are unchanged (all transforms are pixel-level, non-spatial).

Merges augmented Kipukas images + kolabit dataset into data/ with 80/20 split.

Usage:
    python augment.py
    python augment.py --kipukas-dir ../kipukas-qr-dataset-57imgs
"""

import argparse
import random
import shutil
from pathlib import Path

import cv2
import numpy as np


# ── rqrr cascade transforms ────────────────────────────────────────

def adaptive_thresh(img, block=15, c=8):
    """Adaptive threshold — rqrr's primary strategy."""
    grey = cv2.cvtColor(img, cv2.COLOR_BGR2GRAY) if len(img.shape) == 3 else img
    binary = cv2.adaptiveThreshold(
        grey, 255, cv2.ADAPTIVE_THRESH_MEAN_C, cv2.THRESH_BINARY, block * 2 + 1, c
    )
    return cv2.cvtColor(binary, cv2.COLOR_GRAY2BGR)


def adaptive_thresh_fine(img):
    """Finer block adaptive threshold."""
    return adaptive_thresh(img, block=11, c=6)


def adaptive_thresh_coarse(img):
    """Coarser block adaptive threshold."""
    return adaptive_thresh(img, block=21, c=10)


def clahe_transform(img, tiles=4, clip=2.0):
    """CLAHE — contrast-limited adaptive histogram equalization."""
    grey = cv2.cvtColor(img, cv2.COLOR_BGR2GRAY) if len(img.shape) == 3 else img
    clahe = cv2.createCLAHE(clipLimit=clip, tileGridSize=(tiles, tiles))
    result = clahe.apply(grey)
    return cv2.cvtColor(result, cv2.COLOR_GRAY2BGR)


def blur_adaptive(img):
    """Gaussian blur + adaptive threshold — rqrr's blur_at strategy."""
    grey = cv2.cvtColor(img, cv2.COLOR_BGR2GRAY) if len(img.shape) == 3 else img
    blurred = cv2.GaussianBlur(grey, (5, 5), 0)
    binary = cv2.adaptiveThreshold(
        blurred, 255, cv2.ADAPTIVE_THRESH_MEAN_C, cv2.THRESH_BINARY, 31, 8
    )
    return cv2.cvtColor(binary, cv2.COLOR_GRAY2BGR)


def contrast_stretch_adaptive(img):
    """Contrast stretch + adaptive threshold — rqrr's contrast_stretch_at."""
    grey = cv2.cvtColor(img, cv2.COLOR_BGR2GRAY) if len(img.shape) == 3 else img
    lo, hi = grey.min(), grey.max()
    if hi == lo:
        stretched = grey
    else:
        stretched = ((grey.astype(np.float32) - lo) / (hi - lo) * 255).astype(np.uint8)
    binary = cv2.adaptiveThreshold(
        stretched, 255, cv2.ADAPTIVE_THRESH_MEAN_C, cv2.THRESH_BINARY, 31, 8
    )
    return cv2.cvtColor(binary, cv2.COLOR_GRAY2BGR)


def yellow_aware(img):
    """Yellow-aware channel — rqrr's anti-camouflage strategy: max(R,G) - B."""
    if len(img.shape) == 2:
        return cv2.cvtColor(img, cv2.COLOR_GRAY2BGR)
    b, g, r = cv2.split(img)
    yellow = np.maximum(r, g).astype(np.int16) - b.astype(np.int16)
    yellow = np.clip(yellow, 0, 255).astype(np.uint8)
    return cv2.cvtColor(yellow, cv2.COLOR_GRAY2BGR)


def otsu_thresh(img):
    """Otsu's global threshold — rqrr's fallback strategy."""
    grey = cv2.cvtColor(img, cv2.COLOR_BGR2GRAY) if len(img.shape) == 3 else img
    _, binary = cv2.threshold(grey, 0, 255, cv2.THRESH_BINARY + cv2.THRESH_OTSU)
    return cv2.cvtColor(binary, cv2.COLOR_GRAY2BGR)


def gaussian_noise(img, sigma=15):
    """Add Gaussian noise — simulates sensor noise on cheap cameras."""
    noise = np.random.normal(0, sigma, img.shape).astype(np.float32)
    noisy = np.clip(img.astype(np.float32) + noise, 0, 255).astype(np.uint8)
    return noisy


def jpeg_compress(img, quality=35):
    """JPEG compression artifacts — simulates low-quality video frames."""
    encode_param = [int(cv2.IMWRITE_JPEG_QUALITY), quality]
    _, encoded = cv2.imencode('.jpg', img, encode_param)
    return cv2.imdecode(encoded, cv2.IMREAD_COLOR)


# ── All transforms ──────────────────────────────────────────────────

TRANSFORMS = [
    ("at15", adaptive_thresh),
    ("at11", adaptive_thresh_fine),
    ("at21", adaptive_thresh_coarse),
    ("clahe", clahe_transform),
    ("blur_at", blur_adaptive),
    ("stretch_at", contrast_stretch_adaptive),
    ("yellow", yellow_aware),
    ("otsu", otsu_thresh),
    ("noise15", gaussian_noise),
    ("jpeg35", jpeg_compress),
]


# ── Main pipeline ──────────────────────────────────────────────────

def augment_dataset(kipukas_dir: Path, kolabit_dir: Path, output_dir: Path, val_ratio=0.2):
    """Augment Kipukas images, merge with kolabit, split train/val."""

    # Collect all (image_path, label_path, prefix) tuples
    all_samples = []

    # ── 1. Kipukas originals ────────────────────────────────────────
    kip_imgs = sorted((kipukas_dir / "images" / "train").glob("*.jpg"))
    kip_lbls = kipukas_dir / "labels" / "train"

    print(f"[augment] Found {len(kip_imgs)} Kipukas images")

    for img_path in kip_imgs:
        stem = img_path.stem
        lbl_path = kip_lbls / f"{stem}.txt"
        if not lbl_path.exists():
            print(f"  WARN: no label for {stem}, skipping")
            continue
        all_samples.append((img_path, lbl_path, f"kip_{stem}"))

    # ── 2. Augmented Kipukas variants ───────────────────────────────
    aug_dir = output_dir / "_augmented"
    aug_dir.mkdir(parents=True, exist_ok=True)

    aug_count = 0
    for img_path in kip_imgs:
        stem = img_path.stem
        lbl_path = kip_lbls / f"{stem}.txt"
        if not lbl_path.exists():
            continue

        img = cv2.imread(str(img_path))
        if img is None:
            continue

        for suffix, transform_fn in TRANSFORMS:
            try:
                augmented = transform_fn(img)
            except Exception as e:
                print(f"  WARN: {suffix} failed on {stem}: {e}")
                continue

            aug_name = f"kip_{stem}_{suffix}"
            aug_img_path = aug_dir / f"{aug_name}.jpg"
            cv2.imwrite(str(aug_img_path), augmented, [cv2.IMWRITE_JPEG_QUALITY, 92])

            # Label is identical (pixel-level transform, no spatial change)
            aug_lbl_path = aug_dir / f"{aug_name}.txt"
            shutil.copy2(lbl_path, aug_lbl_path)

            all_samples.append((aug_img_path, aug_lbl_path, aug_name))
            aug_count += 1

    print(f"[augment] Generated {aug_count} augmented images")

    # ── 3. Kolabit originals ────────────────────────────────────────
    kolabit_imgs_dir = kolabit_dir / "images" / "train"
    kolabit_lbls_dir = kolabit_dir / "labels" / "train"

    kolabit_count = 0
    if kolabit_imgs_dir.exists():
        for img_path in sorted(kolabit_imgs_dir.iterdir()):
            if img_path.suffix.lower() not in ('.jpg', '.jpeg', '.png'):
                continue
            stem = img_path.stem
            lbl_path = kolabit_lbls_dir / f"{stem}.txt"
            if not lbl_path.exists():
                continue
            all_samples.append((img_path, lbl_path, f"kol_{stem}"))
            kolabit_count += 1

    # Also grab val images (we'll re-split ourselves)
    kolabit_val_imgs = kolabit_dir / "images" / "val"
    kolabit_val_lbls = kolabit_dir / "labels" / "val"
    if kolabit_val_imgs.exists():
        for img_path in sorted(kolabit_val_imgs.iterdir()):
            if img_path.suffix.lower() not in ('.jpg', '.jpeg', '.png'):
                continue
            stem = img_path.stem
            lbl_path = kolabit_val_lbls / f"{stem}.txt"
            if not lbl_path.exists():
                continue
            all_samples.append((img_path, lbl_path, f"kol_v_{stem}"))
            kolabit_count += 1

    print(f"[augment] Found {kolabit_count} kolabit images")
    print(f"[augment] Total samples: {len(all_samples)}")

    # ── 4. Shuffle and split ────────────────────────────────────────
    random.seed(42)
    random.shuffle(all_samples)

    val_count = max(1, int(len(all_samples) * val_ratio))
    val_samples = all_samples[:val_count]
    train_samples = all_samples[val_count:]

    print(f"[augment] Split: {len(train_samples)} train, {len(val_samples)} val")

    # ── 5. Write to output ──────────────────────────────────────────
    for split_name, samples in [("train", train_samples), ("val", val_samples)]:
        img_out = output_dir / "images" / split_name
        lbl_out = output_dir / "labels" / split_name
        img_out.mkdir(parents=True, exist_ok=True)
        lbl_out.mkdir(parents=True, exist_ok=True)

        # Clean existing
        for f in img_out.iterdir():
            f.unlink()
        for f in lbl_out.iterdir():
            f.unlink()

        for img_path, lbl_path, name in samples:
            ext = img_path.suffix
            shutil.copy2(img_path, img_out / f"{name}{ext}")
            shutil.copy2(lbl_path, lbl_out / f"{name}.txt")

    # ── 6. Write dataset.yaml ───────────────────────────────────────
    yaml_path = output_dir.parent / "train" / "dataset.yaml"
    yaml_content = f"""# Kipukas QR Detection — Augmented Dataset
# Kipukas originals: {len(kip_imgs)}
# Augmented (rqrr cascade): {aug_count}
# Kolabit: {kolabit_count}
# Total: {len(all_samples)} ({len(train_samples)} train, {len(val_samples)} val)

path: {output_dir}
train: images/train
val: images/val

nc: 1
names:
  0: qr-code
"""
    yaml_path.write_text(yaml_content)
    print(f"[augment] Wrote {yaml_path}")

    # Cleanup temp augmented dir
    shutil.rmtree(aug_dir, ignore_errors=True)

    print()
    print("=" * 60)
    print("Augmentation Complete")
    print("=" * 60)
    print(f"  Output:   {output_dir}")
    print(f"  Train:    {len(train_samples)} images")
    print(f"  Val:      {len(val_samples)} images")
    print(f"  Dataset:  {yaml_path}")
    print()
    print("Next: python train.py --epochs 100 --device mps")


def parse_args():
    parser = argparse.ArgumentParser(description="Augment Kipukas QR dataset")
    parser.add_argument(
        "--kipukas-dir", type=str,
        default=str(Path(__file__).parent.parent / "kipukas-qr-dataset-57imgs"),
        help="Path to Kipukas annotated dataset"
    )
    parser.add_argument(
        "--kolabit-dir", type=str,
        default=str(Path(__file__).parent.parent / "data"),
        help="Path to kolabit dataset"
    )
    parser.add_argument(
        "--output-dir", type=str,
        default=str(Path(__file__).parent.parent / "data-augmented"),
        help="Output directory (merged dataset)"
    )
    parser.add_argument(
        "--val-ratio", type=float, default=0.2,
        help="Validation split ratio (default: 0.2)"
    )
    return parser.parse_args()


if __name__ == "__main__":
    args = parse_args()
    augment_dataset(
        kipukas_dir=Path(args.kipukas_dir),
        kolabit_dir=Path(args.kolabit_dir),
        output_dir=Path(args.output_dir),
        val_ratio=args.val_ratio,
    )
