#!/usr/bin/env bash
# Build a signed release APK from an already-built arm64 jniLibs tree.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
: "${ANDROID_HOME:?Set ANDROID_HOME}"
: "${ANDROID_JAR:?Set ANDROID_JAR to platforms/android-35/android.jar}"
BUILD_TOOLS="${ANDROID_BUILD_TOOLS:-$ANDROID_HOME/build-tools/35.0.0}"
KEYSTORE="${RIFT_KEYSTORE:-$HOME/.android/debug.keystore}"
KS_PASS="${RIFT_KEYSTORE_PASS:-android}"
KEY_ALIAS="${RIFT_KEY_ALIAS:-androiddebugkey}"
KEY_PASS="${RIFT_KEY_PASS:-android}"

mkdir -p target/pkg/lib/arm64-v8a
cp jniLibs/arm64-v8a/libriff.so target/pkg/lib/arm64-v8a/
rm -f target/res.zip target/base.apk target/aligned.apk target/Rift.apk

"$BUILD_TOOLS/aapt2" compile --dir android/res -o target/res.zip
"$BUILD_TOOLS/aapt2" link -o target/base.apk \
  -I "$ANDROID_JAR" --manifest AndroidManifest.xml -R target/res.zip \
  --min-sdk-version 26 --target-sdk-version 35 \
  --version-code 100 --version-name 1.0.0 --auto-add-overlay
(cd target/pkg && "$JAVA_HOME/bin/jar" uf ../base.apk lib/arm64-v8a/libriff.so)
"$BUILD_TOOLS/zipalign" -p 4 target/base.apk target/aligned.apk
"$BUILD_TOOLS/apksigner" sign \
  --ks "$KEYSTORE" --ks-pass "pass:$KS_PASS" \
  --ks-key-alias "$KEY_ALIAS" --key-pass "pass:$KEY_PASS" \
  --out target/Rift.apk target/aligned.apk
"$BUILD_TOOLS/apksigner" verify --print-certs target/Rift.apk
ls -lh target/Rift.apk
