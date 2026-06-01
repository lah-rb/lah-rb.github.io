#!/usr/bin/env python3
"""Run WeasyPrint with JPEG XL support.

WeasyPrint decodes raster images through Pillow, which doesn't read JPEG XL
natively. `pillow_jxl` (pillow-jxl-plugin, injected into WeasyPrint's venv)
registers the JXL codec with Pillow when imported — so importing it before
rendering lets the PDF embed our .jxl rules images.

Usage: weasyprint_jxl.py <input.html> <output.pdf>
"""
import sys

import pillow_jxl  # noqa: F401  — registers JXL with Pillow (import for side effect)
from weasyprint import HTML

HTML(filename=sys.argv[1]).write_pdf(sys.argv[2])
