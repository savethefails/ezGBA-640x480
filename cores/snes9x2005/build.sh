#!/bin/sh
# Builds libretro's snes9x2005_plus core from libretro/snes9x2005 at a pinned commit: snes9x 1.43
# by way of CATSFC, with blargg's sound chip (USE_BLARGG_APU=1, which is what `_plus` is). An
# experiment beside cores/snes9x: about twice as fast on the SP, which is what lets a SNES game
# afford a frame of run-ahead. taskfile.yml's core:snes9x2005 runs it inside the arm64 bullseye
# box, so the .so links against the same glibc as slot.
#
# Every patch beside this script is applied to that source. runahead-audio.patch keeps the sound
# going across a state load, which run-ahead does every frame: unpatched, each load resets the
# sound chip's output, and the player hears that as crackle.
#
#   build.sh stamp COMMIT               print what a build of COMMIT would record
#   build.sh build COMMIT WORKDIR OUT   build into WORKDIR, then write OUT, OUT.meta,
#                                       OUT.LICENSE and OUT.src.tar.gz
#
# OUT.LICENSE is the checkout's `copyright`: snes9x's non-commercial licence, and the GPL-2.0
# that the CATSFC and ndssfc parts are under. OUT.src.tar.gz is the source the binary was built
# from, patches applied, which is what the GPL asks to travel with it.
set -eu

here="$(cd "$(dirname "$0")" && pwd)"

sha256() {
	if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1"; else shasum -a 256 "$1"; fi |
		cut -d' ' -f1
}

# Nothing beyond the Makefile's own flags, as for cores/snes9x.
device_cflags=""

usage() {
	echo "usage: $0 stamp COMMIT | build COMMIT WORKDIR OUT" >&2
	exit 2
}

stamp() {
	echo "commit=$1"
	echo "source=https://github.com/libretro/snes9x2005/tree/$1"
	echo "make=USE_BLARGG_APU=1"
	echo "device_cflags=$device_cflags"
	for p in "$here"/*.patch; do
		echo "patch=$(basename "$p") sha256:$(sha256 "$p")"
	done
}

build() {
	commit="$1" work="$2" out="$3"
	src="$work/snes9x2005"

	# Pristine at COMMIT on every run; the checkout's own .git first, so git cannot wander up
	# into slot's repository.
	mkdir -p "$src"
	[ -d "$src/.git" ] || git init -q "$src"
	git -C "$src" cat-file -e "$commit^{commit}" 2>/dev/null ||
		git -C "$src" fetch -q --depth 1 https://github.com/libretro/snes9x2005 "$commit"
	git -C "$src" checkout -q --force --detach "$commit"
	git -C "$src" clean -q -fdx
	for p in "$here"/*.patch; do
		git -C "$src" apply "$p"
	done

	make -s -C "$src" -j "$(getconf _NPROCESSORS_ONLN)" USE_BLARGG_APU=1 >/dev/null 2>"$work/snes9x2005-build.log" || {
		tail -n 40 "$work/snes9x2005-build.log" >&2
		exit 1
	}

	for ext in dylib so; do
		if [ -f "$src/snes9x2005_plus_libretro.$ext" ]; then
			mkdir -p "$(dirname "$out")"
			cp "$src/snes9x2005_plus_libretro.$ext" "$out"
			cp "$src/copyright" "$out.LICENSE"
			# The patched tree, as built: `git archive` would give the commit without them.
			tar -czf "$out.src.tar.gz" -C "$work" --exclude=.git --exclude='*.o' --exclude='*.so' --exclude='*.dylib' \
				--transform "s,^snes9x2005,snes9x2005-$commit," snes9x2005
			stamp "$commit" >"$out.meta"
			return
		fi
	done
	echo "make finished without producing snes9x2005_plus_libretro" >&2
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
