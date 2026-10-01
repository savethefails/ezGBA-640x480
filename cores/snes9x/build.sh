#!/bin/sh
# Builds snes9x's libretro core from libretro/snes9x at a pinned commit, for whatever machine
# runs it. taskfile.yml's core:snes9x runs it inside the arm64 bullseye box, so the .so links
# against the same glibc as slot. The source is a checkout of the commit rather than a tarball,
# so git vouches for what was built.
#
#   build.sh stamp COMMIT               print what a build of COMMIT would record
#   build.sh build COMMIT WORKDIR OUT   build into WORKDIR, then write OUT, OUT.meta and
#                                       OUT.LICENSE
#
# The .meta file is how the taskfile tells a stale core from a current one: it is compared
# against `stamp`, so a changed pin or flag rebuilds.
#
# OUT.LICENSE is snes9x's own LICENSE at that commit. Its terms allow the binary to be passed on
# for non-commercial purposes only, and only with that licence and its copyright notice beside
# every copy, so the build keeps the exact text it built from and dist:device ships it.
set -eu

here="$(cd "$(dirname "$0")" && pwd)"

# Nothing beyond libretro's own Makefile flags. Link-time optimisation earned its place on mGBA
# by measurement on the SP; snes9x has not been measured, so it is not assumed.
device_cflags=""

usage() {
	echo "usage: $0 stamp COMMIT | build COMMIT WORKDIR OUT" >&2
	exit 2
}

stamp() {
	echo "commit=$1"
	echo "source=https://github.com/libretro/snes9x/tree/$1"
	echo "device_cflags=$device_cflags"
}

build() {
	commit="$1" work="$2" out="$3"
	src="$work/snes9x"

	# Pristine at COMMIT on every run, so a moved pin never builds over the previous checkout.
	# The checkout's own .git is made first, so a missing one cannot send git up into slot's
	# repository instead.
	mkdir -p "$src"
	[ -d "$src/.git" ] || git init -q "$src"
	git -C "$src" cat-file -e "$commit^{commit}" 2>/dev/null ||
		git -C "$src" fetch -q --depth 1 https://github.com/libretro/snes9x "$commit"
	git -C "$src" checkout -q --force --detach "$commit"
	git -C "$src" clean -q -fdx

	make -s -C "$src/libretro" -j "$(getconf _NPROCESSORS_ONLN)" >/dev/null 2>"$work/snes9x-build.log" || {
		tail -n 40 "$work/snes9x-build.log" >&2
		exit 1
	}

	for ext in dylib so; do
		if [ -f "$src/libretro/snes9x_libretro.$ext" ]; then
			mkdir -p "$(dirname "$out")"
			cp "$src/libretro/snes9x_libretro.$ext" "$out"
			cp "$src/LICENSE" "$out.LICENSE"
			stamp "$commit" >"$out.meta"
			return
		fi
	done
	echo "make finished without producing snes9x_libretro" >&2
	exit 1
}

case "${1:-}" in
stamp)
	[ $# -eq 2 ] && [ -n "$2" ] || usage
	stamp "$2"
	;;
build)
	[ $# -eq 4 ] && [ -n "$2" ] && [ -n "$3" ] && [ -n "$4" ] || usage
	build "$2" "$3" "$4"
	;;
*)
	usage
	;;
esac
