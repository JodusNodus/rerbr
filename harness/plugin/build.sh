#!/bin/bash
# Build Plugins/RBRRecompHarness.dll with MSVC 7.1 under Wine ($RBR_TOOLCHAIN/msvc71), else with clang-cl and lld-link
# (crt/: the C runtime declarations; msvcrt.dll, ntdll.dll for the compiler helpers msvcrt lacks under Wine, and the
# system DLLs through import libraries from crt/*.def); with
# --install, copy it as the harness plugin every play.sh run installs ($RBR_HARNESS_PLUGIN).
set -euo pipefail
source "$(dirname "$0")/../../scripts/dev-env.sh"
cd "$(dirname "$0")/../.."
toolchain=${RBR_TOOLCHAIN:?set RBR_TOOLCHAIN to the MSVC 7.1 toolchain (scripts/dev-env.sh)}
plugin=${RBR_HARNESS_PLUGIN:?set RBR_HARNESS_PLUGIN to the harness plugin (scripts/dev-env.sh)}
sdk=${RBR_PLUGIN_SDK:-${RBR_PATCH:-original/patch-1.02/files/Main_group}/Plugins/RBRTestPlugin}
[ -f "$sdk/IPlugin.h" ] || { echo "Missing plugin SDK header: $sdk/IPlugin.h (set RBR_PLUGIN_SDK)" >&2; exit 1; }
mkdir -p build/harness
cp harness/plugin/RBRRecompHarness.cpp "$sdk/IPlugin.h" "$sdk/IRBRGame.h" build/harness/
if [ -f "$toolchain/msvc71/bin/cl.exe" ]; then
  export WINEPREFIX=$toolchain/wineprefix WINEDEBUG=-all
  wine "$toolchain/msvc71/bin/cl.exe" /nologo /O2 /LD /MD /GR- /D__SUPPORT_PLUGIN__=1 "/I$toolchain/msvc71/include" \
    /Ibuild/harness build/harness/RBRRecompHarness.cpp /Fobuild/harness/ /Febuild/harness/RBRRecompHarness.dll \
    /link "/LIBPATH:$toolchain/msvc71/lib" kernel32.lib user32.lib >build/harness/cl.txt || { cat build/harness/cl.txt; exit 1; }
else
  for tool in clang-cl lld-link llvm-dlltool; do
    command -v "$tool" >/dev/null || { echo "No MSVC 7.1 at $toolchain/msvc71 and no $tool" >&2; exit 1; }
  done
  for lib in kernel32 user32 msvcrt ntdll; do
    llvm-dlltool -m i386 -k -d "harness/plugin/crt/$lib.def" -l "build/harness/$lib.lib"
  done
  cc=(clang-cl --target=i686-pc-windows-msvc /nologo /O2 /MD /GR- /GS- /EHs-c- /Zc:sizedDealloc- -X
      /D__SUPPORT_PLUGIN__=1 /Iharness/plugin/crt /Ibuild/harness)
  "${cc[@]}" /c build/harness/RBRRecompHarness.cpp /Fobuild/harness/RBRRecompHarness.obj
  "${cc[@]}" /c harness/plugin/crt/fltused.cpp /Fobuild/harness/fltused.obj
  lld-link /nologo /dll /machine:x86 /nodefaultlib /entry:DllMain@12 /out:build/harness/RBRRecompHarness.dll \
    build/harness/RBRRecompHarness.obj build/harness/fltused.obj build/harness/{kernel32,user32,msvcrt,ntdll}.lib
fi
if [ "${1:-}" = "--install" ]; then
  # play.sh copies it into the slot at the start of each run
  mkdir -p "$(dirname "$plugin")"
  cp "$PWD/build/harness/RBRRecompHarness.dll" "$plugin.tmp" && mv "$plugin.tmp" "$plugin"
  echo "installed as $plugin (used by every play.sh run from now on)"
fi
