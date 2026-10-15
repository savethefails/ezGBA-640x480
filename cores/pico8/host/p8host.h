/*
 * Shared memory between PICO-8 (running on the libSDL2 stand-in, sdl.c) and
 * the frontend that shows it (pico8_libretro.c). The frontend creates it
 * with memfd_create and passes the descriptor to PICO-8 in P8HOST_FD.
 */
#ifndef P8HOST_H_INCLUDED
#define P8HOST_H_INCLUDED

#include <stdint.h>

#define P8HOST_MAGIC 0x50384853u        /* "P8HS" */
#define P8HOST_VERSION 1
#define P8HOST_W 128
#define P8HOST_H 128
#define P8HOST_RATE 22050               /* PICO-8's mono 16-bit audio */
#define P8HOST_AUDIO 8192               /* ring size in samples, a power of two */

/* Buttons, by libretro joypad id. */
enum {
	P8B_B = 0, P8B_Y = 1, P8B_SELECT = 2, P8B_START = 3, P8B_UP = 4, P8B_DOWN = 5,
	P8B_LEFT = 6, P8B_RIGHT = 7, P8B_A = 8, P8B_X = 9, P8B_L = 10, P8B_R = 11,
};

struct p8host {
	uint32_t magic, version;
	/* PICO-8 side */
	uint32_t ready;                     /* set once PICO-8 has shown a frame */
	uint32_t frame_seq;                 /* odd while frame is being written */
	uint32_t frame[P8HOST_W * P8HOST_H]; /* XRGB8888 */
	uint32_t audio_w;                   /* samples written, free running */
	int16_t audio[P8HOST_AUDIO];
	/* frontend side */
	uint32_t audio_r;                   /* samples read, free running */
	uint32_t buttons;                   /* 1 << P8B_* */
	uint64_t host_ns;                   /* CLOCK_MONOTONIC of the frontend's last frame */
};

#endif
