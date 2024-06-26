  // Access alpine.js x-data
  let alpinel = document.querySelector('[x-data]');
  let QRFound = false;

  // Access the video and canvas elements
  const video = document.getElementById('video');
  const canvas = document.getElementById('canvas');
  const context = canvas.getContext("2d", { willReadFrequently: true }, { alpha: false });
  const spinner = document.getElementById('spinner');

  // Establish zxing from zxing_reader.js
  var zxing = ZXing().then(function (instance) {
    zxing = instance; // this line is supposedly not required but with current emsdk it is :-/
  });


  // Function to start scanning for QR codes
  function startScanning() {
    let consent = alpinel._x_dataStack[0].acceptPrivacy;
    if (consent) {
      // Get access to the camera
      navigator.mediaDevices
        .getUserMedia({ video: {
          facingMode: 'user',
          focusMode: 'continous',
          },
          audio: false,
        })
        .then(function(stream) {
          video.srcObject = stream;
          video.setAttribute('playsinline', true); // required to tell iOS safari we don't want fullscreen
          video.play();
          requestAnimationFrame(scanForQR);
          setTimeout(() => {
            alpinel._x_dataStack[0].videoReady = true;
          }, 100);

        })
        .catch(function(err) {
          console.error('Camera access error:', err);
        });
    }
  }

  function stopScanning() {
    cancelAnimationFrame(requestAnimationFrame);
    if (video.srcObject) {
      video.srcObject.getTracks()[0].stop();
      video.srcObject = null;
      alpinel._x_dataStack[0].videoReady = false;
    }
  }

  function readBarcodeFromCanvas(canvas, format, mode) {
    var imgWidth = canvas.width;
    var imgHeight = canvas.height;
    var imageData = context.getImageData(0, 0, imgWidth, imgHeight);
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

  function scanForQR() {
    if (video.readyState === video.HAVE_ENOUGH_DATA) {
      context.drawImage(video, 0, 0, canvas.width, canvas.height);
      const resultString = readBarcodeFromCanvas(canvas, "QRCode", false).text;
      if (resultString) {
        const url = resultString;
        if (url.startsWith('kpks.us/') || url.startsWith('https://www.kpks.us/')) {
          QRFound = true;
          const pattern = /kpks.us\/|https:\/\/www.kpks.us\//
          stopScanning();
          window.location.href = url.replace(pattern, 'https://www.kipukas.cards/');
        }else {
          alert('Invalid URL detected in QR code');
        }
      }
    }
    
    setTimeout(() => {
      if(!QRFound) {
        requestAnimationFrame(scanForQR);
      }
    }, 10);
  }