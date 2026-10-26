#!/bin/bash
# 为 Android arm64-v8a 交叉编译带 libx264 的静态 ffmpeg CLI。
#
# 用途：把 ffmpeg 一起打进 APK（作为 jniLibs 下的 `libffmpeg.so`），
# 这样铺面渲染页在手机上也能直接出片，不依赖系统里的 ffmpeg。
#
# 用法：scripts/build-android-ffmpeg.sh <输出文件路径>
#   例：scripts/build-android-ffmpeg.sh app/build/ffmpegJniLibs/arm64-v8a/libffmpeg.so
#   （Gradle 的 `ffmpegNdkBuild` 任务会自动调用本脚本）
#
# 依赖：Android NDK（默认 $ANDROID_NDK_HOME 或 $ANDROID_HOME/ndk/27.2.12479018）、
#       网络（首次运行下载 x264 与 ffmpeg 源码）、make。
set -euo pipefail

OUT=${1:?用法: $0 <输出文件路径>}
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
NDK=${ANDROID_NDK_HOME:-${ANDROID_HOME:-$HOME/Android/Sdk}/ndk/27.2.12479018}
TOOLCHAIN=$NDK/toolchains/llvm/prebuilt/linux-x86_64
API=26
WORK=${PHIRA_FFMPEG_BUILD_DIR:-$ROOT/target/android-ffmpeg}
SRC=$WORK/src
PREFIX=$WORK/prefix
JOBS=${JOBS:-$(nproc)}

if [ ! -x "$TOOLCHAIN/bin/aarch64-linux-android$API-clang" ]; then
  echo "找不到 NDK 工具链：$TOOLCHAIN" >&2
  exit 1
fi

mkdir -p "$PREFIX" "$SRC" "$(dirname "$OUT")"

# 源码只下载一次，缓存在 target/android-ffmpeg/src（target 已在 .gitignore 中）
if [ ! -d "$SRC/x264-stable" ]; then
  echo "[ffmpeg] 下载 x264"
  curl -sSL --retry 3 -o "$SRC/x264.tar.bz2" \
    https://code.videolan.org/videolan/x264/-/archive/stable/x264-stable.tar.bz2
  tar xf "$SRC/x264.tar.bz2" -C "$SRC"
fi
if [ ! -d "$SRC/ffmpeg-7.1.1" ]; then
  echo "[ffmpeg] 下载 ffmpeg 7.1.1"
  curl -sSL --retry 3 -o "$SRC/ffmpeg.tar.xz" https://ffmpeg.org/releases/ffmpeg-7.1.1.tar.xz
  tar xf "$SRC/ffmpeg.tar.xz" -C "$SRC"
fi

export CC="$TOOLCHAIN/bin/aarch64-linux-android$API-clang"
export CXX="$TOOLCHAIN/bin/aarch64-linux-android$API-clang++"
export AR="$TOOLCHAIN/bin/llvm-ar"
export NM="$TOOLCHAIN/bin/llvm-nm"
export RANLIB="$TOOLCHAIN/bin/llvm-ranlib"
export STRIP="$TOOLCHAIN/bin/llvm-strip"
export PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig"
export PKG_CONFIG_LIBDIR="$PREFIX/lib/pkgconfig"
export PATH="$TOOLCHAIN/bin:$PATH"

if [ ! -f "$PREFIX/lib/libx264.a" ]; then
  echo "[ffmpeg] 编译 x264"
  cd "$SRC/x264-stable"
  ./configure --prefix="$PREFIX" --host=aarch64-linux-android \
    --cross-prefix="$TOOLCHAIN/bin/llvm-" --sysroot="$TOOLCHAIN/sysroot" \
    --enable-static --enable-pic --disable-cli --disable-opencl \
    --extra-cflags="-O2 -fPIC" >"$WORK/x264-configure.log" 2>&1
  make -j"$JOBS" >"$WORK/x264-make.log" 2>&1
  make install >"$WORK/x264-install.log" 2>&1
fi

if [ ! -x "$WORK/ffmpeg-arm64" ]; then
  echo "[ffmpeg] 编译 ffmpeg"
  cd "$SRC/ffmpeg-7.1.1"
  ./configure \
    --prefix="$PREFIX" \
    --target-os=android --arch=aarch64 --cpu=armv8-a \
    --enable-cross-compile --sysroot="$TOOLCHAIN/sysroot" \
    --cc="$CC" --cxx="$CXX" --ar="$AR" --ranlib="$RANLIB" --nm="$NM" --strip="$STRIP" \
    --enable-static --disable-shared --enable-pic \
    --enable-ffmpeg --disable-ffplay --disable-ffprobe \
    --disable-doc --disable-avdevice --disable-postproc --disable-network --disable-debug \
    --disable-autodetect \
    --enable-libx264 --enable-gpl \
    --extra-cflags="-I$PREFIX/include -O2 -fPIC" \
    --extra-ldflags="-L$PREFIX/lib" \
    --extra-libs="-lm" >"$WORK/ffmpeg-configure.log" 2>&1
  make -j"$JOBS" ffmpeg >"$WORK/ffmpeg-make.log" 2>&1
  cp ffmpeg "$WORK/ffmpeg-arm64"
  "$STRIP" --strip-all "$WORK/ffmpeg-arm64"
fi

cp "$WORK/ffmpeg-arm64" "$OUT"
chmod 755 "$OUT"
echo "[ffmpeg] 完成：$OUT（$(du -h "$OUT" | cut -f1)）"
