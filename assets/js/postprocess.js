/**
 * postprocess.js — Parse YOLO output tensor into bounding boxes.
 *
 * YOLOv12n output shape: [1, 5, 8400]
 *   - 5 = [cx, cy, w, h, confidence] for single-class detection
 *   - 8400 = number of anchor predictions
 *
 * Applies Non-Maximum Suppression (NMS) and returns clean bounding boxes
 * in source image coordinates (accounting for letterbox scaling).
 */

const CONF_THRESHOLD = 0.25;
const NMS_IOU_THRESHOLD = 0.45;

/**
 * Parse YOLO output into bounding boxes.
 *
 * @param {Object} output - From yolo-inference.js runDetection()
 * @param {Float32Array} output.data - Raw output tensor
 * @param {number[]} output.dims - Tensor dimensions [1, 5, N]
 * @param {number} output.scaleX - X scale factor (src/input)
 * @param {number} output.scaleY - Y scale factor (src/input)
 * @param {number} srcWidth - Original frame width
 * @param {number} srcHeight - Original frame height
 * @returns {Array<{x: number, y: number, w: number, h: number, confidence: number}>}
 */
export function parseDetections(output, srcWidth, srcHeight) {
  const { data, dims } = output;

  // Output shape: [1, 5, N] where 5 = cx, cy, w, h, conf
  const numDetections = dims[2];
  const numChannels = dims[1]; // 5 for single-class

  // Determine letterbox parameters
  const inputSize = 640;
  const scale = Math.min(inputSize / srcWidth, inputSize / srcHeight);
  const padX = (inputSize - srcWidth * scale) / 2;
  const padY = (inputSize - srcHeight * scale) / 2;

  // Extract detections above confidence threshold
  const candidates = [];

  for (let i = 0; i < numDetections; i++) {
    // Data is in [1, 5, N] layout → channel-first
    const cx = data[0 * numDetections + i];
    const cy = data[1 * numDetections + i];
    const w = data[2 * numDetections + i];
    const h = data[3 * numDetections + i];
    const conf = data[4 * numDetections + i];

    if (conf < CONF_THRESHOLD) continue;

    // Convert from letterboxed coordinates to source image coordinates
    const x1 = ((cx - w / 2) - padX) / scale;
    const y1 = ((cy - h / 2) - padY) / scale;
    const bw = w / scale;
    const bh = h / scale;

    // Clamp to source image bounds
    const x = Math.max(0, Math.min(x1, srcWidth));
    const y = Math.max(0, Math.min(y1, srcHeight));
    const clampedW = Math.min(bw, srcWidth - x);
    const clampedH = Math.min(bh, srcHeight - y);

    if (clampedW > 0 && clampedH > 0) {
      candidates.push({ x, y, w: clampedW, h: clampedH, confidence: conf });
    }
  }

  // Sort by confidence (descending)
  candidates.sort((a, b) => b.confidence - a.confidence);

  // Non-Maximum Suppression
  return nms(candidates, NMS_IOU_THRESHOLD);
}

/**
 * Non-Maximum Suppression — removes overlapping boxes.
 *
 * @param {Array} boxes - Sorted by confidence descending
 * @param {number} iouThreshold
 * @returns {Array} Filtered boxes
 */
function nms(boxes, iouThreshold) {
  const kept = [];
  const suppressed = new Set();

  for (let i = 0; i < boxes.length; i++) {
    if (suppressed.has(i)) continue;
    kept.push(boxes[i]);

    for (let j = i + 1; j < boxes.length; j++) {
      if (suppressed.has(j)) continue;
      if (iou(boxes[i], boxes[j]) > iouThreshold) {
        suppressed.add(j);
      }
    }
  }

  return kept;
}

/**
 * Compute Intersection over Union between two boxes.
 */
function iou(a, b) {
  const x1 = Math.max(a.x, b.x);
  const y1 = Math.max(a.y, b.y);
  const x2 = Math.min(a.x + a.w, b.x + b.w);
  const y2 = Math.min(a.y + a.h, b.y + b.h);

  const intersection = Math.max(0, x2 - x1) * Math.max(0, y2 - y1);
  const areaA = a.w * a.h;
  const areaB = b.w * b.h;
  const union = areaA + areaB - intersection;

  return union > 0 ? intersection / union : 0;
}

/**
 * Crop a bounding box region from RGBA pixel data.
 * Adds padding for quiet zone around the QR code.
 *
 * @param {Uint8ClampedArray} rgba - Full frame RGBA pixels
 * @param {number} frameW - Frame width
 * @param {number} frameH - Frame height
 * @param {Object} box - Detection bounding box {x, y, w, h}
 * @param {number} [padRatio=0.1] - Padding as fraction of box size
 * @returns {{rgba: Uint8ClampedArray, width: number, height: number}}
 */
export function cropDetection(rgba, frameW, frameH, box, padRatio = 0.1) {
  // Add padding around the detection
  const padX = Math.round(box.w * padRatio);
  const padY = Math.round(box.h * padRatio);

  const x0 = Math.max(0, Math.round(box.x) - padX);
  const y0 = Math.max(0, Math.round(box.y) - padY);
  const x1 = Math.min(frameW, Math.round(box.x + box.w) + padX);
  const y1 = Math.min(frameH, Math.round(box.y + box.h) + padY);

  const cropW = x1 - x0;
  const cropH = y1 - y0;
  const cropRgba = new Uint8ClampedArray(cropW * cropH * 4);

  for (let row = 0; row < cropH; row++) {
    const srcOffset = ((y0 + row) * frameW + x0) * 4;
    const dstOffset = row * cropW * 4;
    cropRgba.set(rgba.subarray(srcOffset, srcOffset + cropW * 4), dstOffset);
  }

  return { rgba: cropRgba, width: cropW, height: cropH };
}
