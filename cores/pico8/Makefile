# Builds the PICO-8 core and the two files it runs PICO-8 with. The source is a copy of
# savethefails/pico-8-save-states (see README.md here). taskfile.yml's core:pico8 runs this in
# the arm64 bullseye box, so all three link against the same glibc as slot.
#
#   make OUT=dir   writes dir/pico8_libretro.so, dir/pico8/p8ss.so and
#                  dir/pico8/lib/libSDL2-2.0.so.0
CC ?= cc
CFLAGS ?= -O2 -Wall -Wextra -Wno-format-truncation
OUT ?= build

all: $(OUT)/pico8_libretro.so $(OUT)/pico8/p8ss.so $(OUT)/pico8/lib/libSDL2-2.0.so.0

$(OUT)/pico8_libretro.so: host/pico8_libretro.c host/p8host.h host/libretro.h
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) -fPIC -shared -fvisibility=hidden -o $@ host/pico8_libretro.c -ldl

$(OUT)/pico8/p8ss.so: src/p8ss.c src/statefile.c src/statefile.h
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) -fPIC -shared -fno-omit-frame-pointer -fno-stack-protector -o $@ src/p8ss.c src/statefile.c -ldl -lpthread

$(OUT)/pico8/lib/libSDL2-2.0.so.0: host/sdl.c host/p8host.h
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) -fPIC -shared -fvisibility=hidden -Wl,-soname,libSDL2-2.0.so.0 -o $@ host/sdl.c -lpthread

.PHONY: all
