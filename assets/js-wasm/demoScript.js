  <script>
		var zxing = ZXing().then(function (instance) {
			zxing = instance; // this line is supposedly not required but with current emsdk it is :-/
		});

		const cameraSelector = document.getElementById("cameraSelector");
		const format = document.getElementById("format");
		const mode = document.getElementById("mode");
		const canvas = document.getElementById("canvas");
		const resultElement = document.getElementById("result");

		const ctx = canvas.getContext("2d", { willReadFrequently: true });
		const video = document.createElement("video");
		video.setAttribute("id", "video");
		video.setAttribute("width", canvas.width);
		video.setAttribute("height", canvas.height);
		video.setAttribute("autoplay", "");

		function readBarcodeFromCanvas(canvas, format, mode) {
			var imgWidth = canvas.width;
			var imgHeight = canvas.height;
			var imageData = canvas.getContext('2d').getImageData(0, 0, imgWidth, imgHeight);
			var sourceBuffer = imageData.data;

			if (zxing != null) {
				var buffer = zxing._malloc(sourceBuffer.byteLength);
				zxing.HEAPU8.set(sourceBuffer, buffer);
				var result = zxing.readBarcodeFromPixmap(buffer, imgWidth, imgHeight, mode, format);
				zxing._free(buffer);
				return result;
			} else {
				return { error: "ZXing not yet initialized" };
			}
		}

		function drawResult(code) {
			ctx.beginPath();
			ctx.lineWidth = 4;
			ctx.strokeStyle = "red";
			// ctx.textAlign = "center";
			// ctx.fillStyle = "#green"
			// ctx.font = "25px Arial";
			// ctx.fontWeight = "bold";
			with (code.position) {
				ctx.moveTo(topLeft.x, topLeft.y);
				ctx.lineTo(topRight.x, topRight.y);
				ctx.lineTo(bottomRight.x, bottomRight.y);
				ctx.lineTo(bottomLeft.x, bottomLeft.y);
				ctx.lineTo(topLeft.x, topLeft.y);
				ctx.stroke();
				// ctx.fillText(code.text, (topLeft.x + bottomRight.x) / 2, (topLeft.y + bottomRight.y) / 2);
			}
		}

		function escapeTags(htmlStr) {
			return htmlStr.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;").replace(/'/g, "&#39;");
		}

		const processFrame = function () {
			ctx.drawImage(video, 0, 0, canvas.width, canvas.height);

			const code = readBarcodeFromCanvas(canvas, format.value, mode.value === 'true');
			if (code.format) {
				resultElement.innerText = code.format + ": " + escapeTags(code.text);
				drawResult(code)
			} else {
				resultElement.innerText = "No barcode found";
			}
			requestAnimationFrame(processFrame);
		};

		const updateVideoStream = function (deviceId) {
			// To ensure the camera switch, it is advisable to free up the media resources
			if (video.srcObject) video.srcObject.getTracks().forEach(track => track.stop());

			navigator.mediaDevices
				.getUserMedia({ video: { facingMode: deviceId }, audio: false })
				.then(function (stream) {
					video.srcObject = stream;
					video.setAttribute("playsinline", true); // required to tell iOS safari we don't want fullscreen
					video.play();
					processFrame();
				})
				.catch(function (error) {
					console.error("Error accessing camera:", error);
				});
		};

		cameraSelector.addEventListener("change", function () {
			updateVideoStream(this.value);
		});

		updateVideoStream();
  </script>
