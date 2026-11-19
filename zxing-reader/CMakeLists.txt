# The scanner's QR reader: zxing-cpp's reader of QR codes, and reader.cpp in
# front of it, as one WebAssembly module. Run by build.sh, which says where
# the zxing-cpp source is.

cmake_minimum_required (VERSION 3.16)

project (KipukasQrReader C CXX)

if (NOT ZXING_SOURCE)
    message (FATAL_ERROR "ZXING_SOURCE must name a zxing-cpp source tree (see build.sh)")
endif ()

set (CMAKE_CXX_STANDARD 20)
set (CMAKE_CXX_STANDARD_REQUIRED ON)
if (NOT CMAKE_BUILD_TYPE)
    set (CMAKE_BUILD_TYPE "Release" CACHE STRING "" FORCE)
endif ()

# Of zxing-cpp: reading, of QR codes, and nothing else.
set (ZXING_READERS ON CACHE BOOL "" FORCE)
set (ZXING_WRITERS "OFF" CACHE STRING "" FORCE)
set (ZXING_C_API OFF CACHE BOOL "" FORCE)
foreach (format 1D AZTEC DATAMATRIX MAXICODE PDF417)
    set (ZXING_ENABLE_${format} OFF CACHE BOOL "" FORCE)
endforeach ()
set (ZXING_ENABLE_QRCODE ON CACHE BOOL "" FORCE)
# ReaderOptions::tryDenoise is behind this.
set (ZXING_EXPERIMENTAL_API ON CACHE BOOL "" FORCE)

# C++ exceptions are WebAssembly's own, not passed through JavaScript: the
# module needs no JavaScript beside it. Optimised across the whole program.
set (READER_FLAGS -fwasm-exceptions -flto ${READER_EXTRA_FLAGS})
add_compile_options (${READER_FLAGS})
add_link_options (${READER_FLAGS})

include (${ZXING_SOURCE}/zxing.cmake)
add_subdirectory (${ZXING_SOURCE}/core ${CMAKE_BINARY_DIR}/ZXing)

add_executable (reader reader.cpp)
target_link_libraries (reader ZXing::ZXing)
set_target_properties (reader PROPERTIES SUFFIX ".wasm")
target_link_options (reader PRIVATE
    # A module that stands alone: no generated JavaScript, and no main().
    -sSTANDALONE_WASM=1
    --no-entry
    -sFILESYSTEM=0
    -sALLOW_MEMORY_GROWTH=1
)
