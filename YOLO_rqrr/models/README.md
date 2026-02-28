# Models

This directory holds exported YOLO models. Files are gitignored due to size.

## To regenerate

```bash
cd ../train
python train.py          # produces runs/detect/train/weights/best.pt
python export_onnx.py    # produces ../models/yolo12n-qr.onnx
```

## Expected files

| File | Size | Description |
|------|------|-------------|
| `yolo12n-qr.onnx` | ~5-10MB | YOLOv12n fine-tuned for QR detection, ONNX opset 12 |
