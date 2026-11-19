// The scanner's QR reader: zxing-cpp behind three functions, built to
// WebAssembly with nothing of JavaScript's in it (build.sh). The page's
// worker writes a picture into the reader's memory and takes back the text
// of the code in it (assets/js/zxing-decode.js).

#include "ReadBarcode.h"

#include <cstddef>
#include <cstdint>
#include <string>
#include <vector>

#include <emscripten/emscripten.h>

namespace {

// The picture to read: four bytes (red, green, blue, alpha) a pixel.
std::vector<uint8_t> picture;
int wide = 0;
int high = 0;

// What the last code read says.
std::string text;

// A camera frame is 640 pixels a side. Nothing near this is ever asked for.
const int LONGEST_SIDE = 8192;

} // namespace

extern "C" {

/// Make room for a picture of this size and say where it is. The worker
/// writes the picture there. Null if the size is not one a picture can have.
EMSCRIPTEN_KEEPALIVE uint8_t* qr_picture(int width, int height)
{
	if (width <= 0 || height <= 0 || width > LONGEST_SIDE || height > LONGEST_SIDE)
		return nullptr;
	wide = width;
	high = height;
	picture.resize(static_cast<size_t>(width) * static_cast<size_t>(height) * 4);
	return picture.data();
}

/// Read the QR code in the picture. Gives the length in bytes of what it
/// says, which is then at qr_text(); 0 if no code was read; -1 if the reader
/// failed.
///
/// `denoise`: also look after closing small gaps in the picture's dark
/// parts, which helps with a picture out of focus (a camera with no
/// autofocus) at the cost of a second look at every picture with no code.
EMSCRIPTEN_KEEPALIVE int qr_read(int denoise)
{
	text.clear();
	if (picture.empty())
		return 0;
	try {
		auto options = ZXing::ReaderOptions()
			// The codes on the cards are ordinary QR codes. Micro QR and
			// rMQR, the other members of the family, are not looked for.
			.formats(ZXing::BarcodeFormat::QRCodeModel2)
			.tryHarder(true)
			.tryRotate(true)
			.tryInvert(true)
			.tryDownscale(true)
			.tryDenoise(denoise != 0)
			.maxNumberOfSymbols(1);
		auto codes = ZXing::ReadBarcodes({picture.data(), wide, high, ZXing::ImageFormat::RGBA}, options);
		for (const auto& code : codes) {
			if (code.isValid()) {
				text = code.text();
				break;
			}
		}
	} catch (...) {
		return -1;
	}
	return static_cast<int>(text.size());
}

/// What the last code read says, in UTF-8. Its length is what qr_read() gave.
EMSCRIPTEN_KEEPALIVE const char* qr_text()
{
	return text.data();
}

} // extern "C"
