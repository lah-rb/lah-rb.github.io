#!/usr/bin/env python3
"""
fetch_dataset.py — Download and prepare QR code detection datasets.

Downloads the kolabit/qr-codes dataset (YOLO format, Apache-2.0 license)
from GitHub, which is based on the Kaggle Finder Patterns dataset.

Creates the standard YOLO directory structure:
  data/
    images/
      train/
      val/
    labels/
      train/
      val/

Usage:
    python fetch_dataset.py
"""

import os
import shutil
import subprocess
import sys
from pathlib import Path

DATA_DIR = Path(__file__).parent.parent / "data"
REPO_URL = "https://github.com/kolabit/qr-codes.git"
CLONE_DIR = DATA_DIR / "_kolabit_clone"

# Validation split ratio (fraction of images used for validation)
VAL_SPLIT = 0.15


def clone_dataset():
    """Clone the kolabit/qr-codes repository."""
    if CLONE_DIR.exists():
        print(f"[fetch] Clone directory already exists: {CLONE_DIR}")
        return

    print(f"[fetch] Cloning {REPO_URL} ...")
    subprocess.run(
        ["git", "clone", "--depth", "1", REPO_URL, str(CLONE_DIR)],
        check=True,
    )
    print("[fetch] Clone complete.")


def prepare_directory_structure():
    """Create the YOLO-standard directory layout."""
    for split in ("train", "val"):
        (DATA_DIR / "images" / split).mkdir(parents=True, exist_ok=True)
        (DATA_DIR / "labels" / split).mkdir(parents=True, exist_ok=True)


def split_and_copy():
    """Split images/labels into train/val and copy to data/."""
    src_images = CLONE_DIR / "images"
    src_labels = CLONE_DIR / "labels"

    if not src_images.exists() or not src_labels.exists():
        print("[fetch] ERROR: Expected images/ and labels/ in cloned repo")
        sys.exit(1)

    # Collect all image files that have matching labels
    image_files = sorted(
        f for f in src_images.iterdir()
        if f.suffix.lower() in (".jpg", ".jpeg", ".png", ".bmp", ".webp")
    )

    # Filter to only images that have corresponding label files
    paired = []
    for img_path in image_files:
        label_path = src_labels / (img_path.stem + ".txt")
        if label_path.exists():
            paired.append((img_path, label_path))

    if not paired:
        print("[fetch] ERROR: No image-label pairs found in cloned dataset")
        sys.exit(1)

    print(f"[fetch] Found {len(paired)} image-label pairs")

    # Split: last N% for validation
    val_count = max(1, int(len(paired) * VAL_SPLIT))
    train_pairs = paired[:-val_count]
    val_pairs = paired[-val_count:]

    print(f"[fetch] Train: {len(train_pairs)}, Val: {len(val_pairs)}")

    for pairs, split in [(train_pairs, "train"), (val_pairs, "val")]:
        for img_src, lbl_src in pairs:
            shutil.copy2(img_src, DATA_DIR / "images" / split / img_src.name)
            shutil.copy2(lbl_src, DATA_DIR / "labels" / split / lbl_src.name)

    print("[fetch] Files copied to data/images/ and data/labels/")


def cleanup():
    """Remove the cloned repository to save space."""
    if CLONE_DIR.exists():
        shutil.rmtree(CLONE_DIR)
        print("[fetch] Cleaned up clone directory")


def main():
    print("=" * 60)
    print("QR Code Detection Dataset Preparation")
    print("=" * 60)

    clone_dataset()
    prepare_directory_structure()
    split_and_copy()
    cleanup()

    # Print summary
    train_count = len(list((DATA_DIR / "images" / "train").iterdir()))
    val_count = len(list((DATA_DIR / "images" / "val").iterdir()))
    print()
    print(f"Dataset ready in: {DATA_DIR}")
    print(f"  Train images: {train_count}")
    print(f"  Val images:   {val_count}")
    print()
    print("Next step: python train.py")


if __name__ == "__main__":
    main()
