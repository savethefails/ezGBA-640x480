/*
 * A stand-in for libSDL2-2.0.so.0 that implements exactly what PICO-8
 * uses, so PICO-8 can run inside another program's screen instead of
 * owning the display, the sound card and the buttons.
 *
 * - The picture: PICO-8 draws its 128x128 screen into a texture, scaled up,
 *   and copies it to the window. Each present publishes that texture,
 *   sampled back down to 128x128, to the shared memory in p8host.h.
 * - Sound: PICO-8's audio callback is run on a thread of our own that keeps
 *   the shared ring topped up, so sound plays at the rate the frontend
 *   takes it.
 * - Buttons: the frontend's buttons become the keys PICO-8 reads (arrows, Z,
 *   X, Enter), both as keyboard state and as key events.
 * - Pausing: when the frontend stops asking for frames (its menu is open,
 *   the lid is shut), PICO-8 waits at its next present until it asks again.
 *
 * Without P8HOST_FD the shared memory is private, so PICO-8 runs headless.
 */
#define _GNU_SOURCE
#include "p8host.h"

#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <time.h>
#include <unistd.h>

#define EXPORT __attribute__((visibility("default")))

typedef uint8_t Uint8;
typedef uint16_t Uint16;
typedef uint32_t Uint32;
typedef int16_t Sint16;
typedef int SDL_bool;

typedef struct { int x, y, w, h; } SDL_Rect;
typedef struct { Uint8 major, minor, patch; } SDL_version;
typedef struct { Uint32 format; int w, h, refresh_rate; void *driverdata; } SDL_DisplayMode;
typedef struct {
	const char *name;
	Uint32 flags, num_texture_formats, texture_formats[16];
	int max_texture_width, max_texture_height;
} SDL_RendererInfo;
typedef struct {
	int freq;
	Uint16 format;
	Uint8 channels, silence;
	Uint16 samples, padding;
	Uint32 size;
	void (*callback)(void *userdata, Uint8 *stream, int len);
	void *userdata;
} SDL_AudioSpec;
typedef struct {
	Uint32 format;
	void *palette;
	Uint8 BitsPerPixel, BytesPerPixel, padding[2];
	Uint32 Rmask, Gmask, Bmask, Amask;
	Uint8 Rloss, Gloss, Bloss, Aloss, Rshift, Gshift, Bshift, Ashift;
	int refcount;
	void *next;
} SDL_PixelFormat;
typedef struct {
	Uint32 flags;
	SDL_PixelFormat *format;
	int w, h, pitch;
	void *pixels, *userdata;
	int locked;
	void *list_blitmap;
	SDL_Rect clip_rect;
	void *map;
	int refcount;
} SDL_Surface;
typedef struct {
	Uint32 type, timestamp, windowID;
	Uint8 state, repeat, padding2, padding3;
	struct { int scancode, sym; Uint16 mod; Uint32 unused; } keysym;
} SDL_KeyboardEvent;
typedef union { Uint32 type; SDL_KeyboardEvent key; Uint8 padding[56]; } SDL_Event;

#define PIXELFORMAT_ARGB8888 0x16362004u
#define SDL_KEYDOWN 0x300
#define SDL_KEYUP 0x301
#define NUM_SCANCODES 512

struct texture { int w, h; uint32_t *px; };
struct window { int w, h; SDL_Surface *surface; };
struct renderer { struct window *win; struct texture *shown; long shown_area; };

static struct p8host *shm;
static struct p8host local_shm;

static uint64_t now_ns(void)
{
	struct timespec ts;
	clock_gettime(CLOCK_MONOTONIC, &ts);
	return (uint64_t)ts.tv_sec * 1000000000u + ts.tv_nsec;
}
static uint64_t start_ns;
static pthread_t main_thread;         /* PICO-8's: see "alignment" */
static int64_t adjust_ns;               /* taken off PICO-8's clock; negative runs it ahead */
/* For the summary: frame starts measured, and their sum, since the last one. */
static int64_t align_sum_ns;
static unsigned align_n, align_skipped;

/* ---- the latency log ----
 * A press followed from the frontend changing the buttons, to PICO-8 taking them in, to the
 * first frame PICO-8 published after that, to the frontend taking that frame. Written to
 * PICO-8's own output, which ezGBA keeps on the card: a line for each of the first presses of a
 * run and then every LAT_THEN'th, and every LAT_WINDOW_NS a summary of the frames, the input
 * polls, the waits on the frontend and the presses. What it cannot see: the frames a cart takes
 * to answer a press, and what the frontend does before and after. */
#define LAT_WINDOW_NS 10000000000ull
#define LAT_FIRST 40u
#define LAT_THEN 25u
struct lat_stat { double sum, max; unsigned n; };
static void lat_add(struct lat_stat *s, double v)
{
	s->sum += v;
	if (v > s->max) s->max = v;
	s->n++;
}
static struct {
	uint32_t seq, buttons, drawn_frame;
	uint64_t in_ns, read_ns, drawn_ns;
	int state;                          /* 0 nothing followed, 1 read, 2 drawn */
	unsigned presses;
	uint64_t since_ns, frame_ns, poll_ns;
	unsigned frames, polls, waits;
	double frame_gap, poll_gap, wait_ms;
	struct lat_stat to_read, to_draw, to_pick, total;
} lat;
static int lat_on(void);

static void lat_summary(uint64_t t)
{
	if (!lat.since_ns) { lat.since_ns = t; return; }
	if (t - lat.since_ns < LAT_WINDOW_NS) return;
#define AVG(s) ((s).n ? (s).sum / (s).n : 0.0)
	fprintf(stderr, "[sdl] last %.1f s: %u frames published (longest gap %.1f ms), "
		"%u input polls (longest gap %.1f ms), waited on the frontend %u times (%.0f ms); "
		"%u presses: read avg %.1f max %.1f, drawn avg %.1f max %.1f, "
		"taken avg %.1f max %.1f, core to core avg %.1f max %.1f ms\n",
		(t - lat.since_ns) / 1e9, lat.frames, lat.frame_gap, lat.polls, lat.poll_gap,
		lat.waits, lat.wait_ms, lat.total.n, AVG(lat.to_read), lat.to_read.max,
		AVG(lat.to_draw), lat.to_draw.max, AVG(lat.to_pick), lat.to_pick.max,
		AVG(lat.total), lat.total.max);
#undef AVG
	{
		static uint32_t waited, missed;
		uint32_t w = shm->waited, m = shm->missed;
		fprintf(stderr, "[sdl]   alignment: %s; frames started %.1f ms after the buttons on "
			"average (%u measured, %u not), clock moved %.1f ms in all; the core waits "
			"every %u call(s): %u frames had, %u not in time\n",
			__atomic_load_n(&shm->aligned, __ATOMIC_ACQUIRE) ? "aligned" : "NOT aligned",
			align_n ? align_sum_ns / 1e6 / align_n : 0.0, align_n, align_skipped,
			-adjust_ns / 1e6, shm->waits_every, w - waited, m - missed);
		waited = w;
		missed = m;
		align_sum_ns = 0;
		align_n = align_skipped = 0;
	}
	lat.since_ns = t;
	lat.frames = lat.polls = lat.waits = 0;
	lat.frame_gap = lat.poll_gap = lat.wait_ms = 0;
	memset(&lat.to_read, 0, sizeof lat.to_read);
	memset(&lat.to_draw, 0, sizeof lat.to_draw);
	memset(&lat.to_pick, 0, sizeof lat.to_pick);
	memset(&lat.total, 0, sizeof lat.total);
}

/* The followed press's frame has been published: has the frontend taken it yet? */
static void lat_check_picked(void)
{
	if (lat.state != 2) return;
	uint32_t pf = __atomic_load_n(&shm->picked_frame, __ATOMIC_ACQUIRE);
	if ((int32_t)(pf - lat.drawn_frame) < 0) return;
	uint64_t pick_ns = shm->picked_ns;
	int exact = pf == lat.drawn_frame;
	double r = (lat.read_ns - lat.in_ns) / 1e6, d = (lat.drawn_ns - lat.read_ns) / 1e6;
	double p = pick_ns > lat.drawn_ns ? (pick_ns - lat.drawn_ns) / 1e6 : 0;
	double all = pick_ns > lat.in_ns ? (pick_ns - lat.in_ns) / 1e6 : 0;
	lat_add(&lat.to_read, r);
	lat_add(&lat.to_draw, d);
	lat_add(&lat.to_pick, p);
	lat_add(&lat.total, all);
	lat.presses++;
	if (lat.presses <= LAT_FIRST || lat.presses % LAT_THEN == 0)
		fprintf(stderr, "[sdl] press %u (buttons %#x): PICO-8 read it %.1f ms after the core "
			"sent it, published a frame %.1f ms after that, the core took %s %.1f ms later; "
			"%.1f ms core to core\n", lat.presses, lat.buttons, r, d,
			exact ? "it" : "a later one", p, all);
	lat.state = 0;
}

/* PICO-8 has taken in buttons that changed. */
static void lat_read(uint32_t b)
{
	if (!lat_on()) return;
	uint32_t seq = __atomic_load_n(&shm->input_seq, __ATOMIC_ACQUIRE);
	if (seq == lat.seq) return;
	lat.seq = seq;
	lat.buttons = b;
	lat.in_ns = shm->input_ns;
	lat.read_ns = now_ns();
	lat.state = 1;
}

/* PICO-8 asked for input. */
static void lat_poll(void)
{
	if (!lat_on()) return;
	uint64_t t = now_ns();
	if (lat.poll_ns) {
		double gap = (t - lat.poll_ns) / 1e6;
		if (gap > lat.poll_gap) lat.poll_gap = gap;
	}
	lat.poll_ns = t;
	lat.polls++;
}

/* PICO-8 published frame `seq`. */
static void lat_frame(uint32_t seq)
{
	if (!lat_on()) return;
	uint64_t t = now_ns();
	lat_check_picked();
	if (lat.frame_ns) {
		double gap = (t - lat.frame_ns) / 1e6;
		if (gap > lat.frame_gap) lat.frame_gap = gap;
	}
	lat.frame_ns = t;
	lat.frames++;
	if (lat.state == 1) {
		lat.drawn_ns = t;
		lat.drawn_frame = seq;
		lat.state = 2;
	}
	lat_summary(t);
}

__attribute__((constructor)) static void shim_init(void)
{
	start_ns = now_ns();
	const char *fd = getenv("P8HOST_FD");
	if (fd && *fd) {
		int n = 0;                       /* not atoi: glibc 2.38 renames it */
		while (*fd >= '0' && *fd <= '9') n = n * 10 + (*fd++ - '0');
		void *p = mmap(NULL, sizeof *shm, PROT_READ | PROT_WRITE, MAP_SHARED, n, 0);
		if (p != MAP_FAILED) shm = p;
	}
	if (!shm) {
		shm = &local_shm;
		shm->magic = P8HOST_MAGIC;
		shm->version = P8HOST_VERSION;
	}
	main_thread = pthread_self();
}

/* ---- core ---- */
EXPORT int SDL_Init(Uint32 f) { (void)f; return 0; }
EXPORT int SDL_InitSubSystem(Uint32 f) { (void)f; return 0; }
EXPORT void SDL_GetVersion(SDL_version *v) { v->major = 2; v->minor = 0; v->patch = 20; }
EXPORT const char *SDL_GetError(void) { return ""; }
EXPORT void SDL_ClearError(void) {}
EXPORT SDL_bool SDL_SetHint(const char *n, const char *v) { (void)n; (void)v; return 1; }
EXPORT void SDL_free(void *p) { free(p); }
/* Time PICO-8 spent held by wait_for_host, taken out of its clock: to PICO-8 the frontend
 * being away is time that did not pass. Seen as a slow frame, the half second ezGBA takes to
 * put a cart in had PICO-8 warn at boot that it was running below 30fps. */
static uint64_t held_ns;

/* ---- alignment ----
 * PICO-8 starts a frame when its clock passes the frame's deadline, and the frontend writes the
 * buttons once a frame of its own; the two drift past each other, so the frame drawn after a
 * press was ready a few ms after the frontend had already taken the one before it, and a press
 * waited a whole frontend frame here. Its clock is kept flowing (the ARM PICO-8 drops to 30 fps
 * on one that steps) but nudged, under a millisecond a frame, until each frame starts just
 * after the frontend writes the buttons. Then the frontend waits the few ms for that frame and
 * shows it in the same frame of its own. */
/* Frame start, after the buttons are written: far enough after that PICO-8's whole-millisecond
 * clock and its wait between frames, which jitter a start by about a millisecond either way,
 * never start one before them, which would leave a press for the frame after. */
#define ALIGN_TARGET_NS 2000000ll
#define ALIGN_STEP_NS 500000ll          /* the most the clock is moved in one frame */
#define ALIGN_NEAR_NS 1500000ll         /* within this of the target counts as aligned */
#define ALIGN_FAR_NS 3000000ll          /* past this, no longer aligned */
#define ALIGN_RUN 8                     /* frames near the target in a row to be aligned */
static uint64_t last_ticks_ns;          /* never goes back */
static pthread_mutex_t ticks_lock = PTHREAD_MUTEX_INITIALIZER;
/* On PICO-8's main thread: when it last woke from a wait, and the frontend's frame then. */
static uint64_t woke_ns, woke_host_ns, published_ns;
static int near_run;


EXPORT Uint32 SDL_GetTicks(void)
{
	uint64_t held = __atomic_load_n(&held_ns, __ATOMIC_RELAXED);
	pthread_mutex_lock(&ticks_lock);
	int64_t t = (int64_t)(now_ns() - start_ns - held) - adjust_ns;
	if (t < (int64_t)last_ticks_ns) t = (int64_t)last_ticks_ns;
	last_ticks_ns = (uint64_t)t;
	pthread_mutex_unlock(&ticks_lock);
	return (Uint32)(t / 1000000);
}

static int lat_on(void);

/* A frame has just been published: how far after the frontend's buttons did it start, and
 * the clock moved to bring that to the target. */
static void align_frame(uint64_t t)
{
	if (!lat_on()) return;
	uint64_t start = woke_ns > published_ns ? woke_ns : published_ns;
	uint64_t host = woke_ns > published_ns ? woke_host_ns : 0;
	published_ns = t;
	if (!host || start < host) { align_skipped++; return; }
	int64_t period = 16800000ll;
	int64_t e = (int64_t)(start - host) % period;
	if (e > period / 2) e -= period;            /* started just before the next buttons */
	align_sum_ns += e;
	align_n++;
	int64_t err = e - ALIGN_TARGET_NS;
	int64_t step = err * 3 / 10;
	if (step > ALIGN_STEP_NS) step = ALIGN_STEP_NS;
	if (step < -ALIGN_STEP_NS) step = -ALIGN_STEP_NS;
	pthread_mutex_lock(&ticks_lock);
	adjust_ns -= step;
	pthread_mutex_unlock(&ticks_lock);
	int64_t mag = err < 0 ? -err : err;
	if (mag <= ALIGN_NEAR_NS) {
		if (near_run < ALIGN_RUN && ++near_run == ALIGN_RUN)
			__atomic_store_n(&shm->aligned, 1, __ATOMIC_RELEASE);
	} else if (mag > ALIGN_FAR_NS) {
		near_run = 0;
		__atomic_store_n(&shm->aligned, 0, __ATOMIC_RELEASE);
	}
}

EXPORT void SDL_Delay(Uint32 ms)
{
	/* PICO-8's main loop between frames: woken as soon as the frontend writes new buttons, so
	 * a frame due then starts then. Other threads (the sound) sleep for real. */
	if (lat_on() && pthread_equal(pthread_self(), main_thread)) {
		uint64_t host = __atomic_load_n(&shm->host_ns, __ATOMIC_ACQUIRE);
		uint64_t until = now_ns() + (uint64_t)ms * 1000000ull;
		while (now_ns() < until && __atomic_load_n(&shm->host_ns, __ATOMIC_ACQUIRE) == host) {
			struct timespec ts = { 0, 200000 };
			nanosleep(&ts, NULL);
		}
		woke_ns = now_ns();
		woke_host_ns = __atomic_load_n(&shm->host_ns, __ATOMIC_ACQUIRE);
		return;
	}
	struct timespec ts = { ms / 1000, (long)(ms % 1000) * 1000000 };
	while (nanosleep(&ts, &ts) != 0) {}
}

/* ---- threads ---- */
/* PICO-8 downloads from the BBS on a thread, keeps the handle, and detaches
 * it when the next download starts. A handle is ours, not PICO-8's: it is
 * not in the memory a save state keeps. So a state loaded in another run
 * holds a handle this process never made, and detaching through it would
 * read freed or unmapped memory. Only handles made here, and not yet
 * detached, are acted on; any other is a thread that no longer exists. */
struct thread { pthread_t t; struct thread *next; };
struct thread_start { int (*fn)(void *); void *data; };
static struct thread *threads;
static pthread_mutex_t threads_lock = PTHREAD_MUTEX_INITIALIZER;
static void *thread_main(void *p)
{
	struct thread_start s = *(struct thread_start *)p;
	free(p);
	return (void *)(intptr_t)s.fn(s.data);
}
EXPORT void *SDL_CreateThread(int (*fn)(void *), const char *name, void *data)
{
	(void)name;
	struct thread *t = calloc(1, sizeof *t);
	struct thread_start *s = malloc(sizeof *s);
	if (!t || !s) { free(t); free(s); return NULL; }
	s->fn = fn;
	s->data = data;
	pthread_mutex_lock(&threads_lock);
	if (pthread_create(&t->t, NULL, thread_main, s) != 0) {
		pthread_mutex_unlock(&threads_lock);
		free(t);
		free(s);
		return NULL;
	}
	t->next = threads;
	threads = t;
	pthread_mutex_unlock(&threads_lock);
	return t;
}
EXPORT void SDL_DetachThread(void *p)
{
	if (!p) return;
	pthread_mutex_lock(&threads_lock);
	for (struct thread **at = &threads; *at; at = &(*at)->next) {
		if (*at != p) continue;
		struct thread *t = *at;
		*at = t->next;
		pthread_detach(t->t);
		free(t);
		break;
	}
	pthread_mutex_unlock(&threads_lock);
}

/* ---- displays and video drivers ---- */
#define SCREEN_W 640
#define SCREEN_H 480
static void fill_mode(SDL_DisplayMode *m)
{
	m->format = PIXELFORMAT_ARGB8888;
	m->w = SCREEN_W;
	m->h = SCREEN_H;
	m->refresh_rate = 60;
	m->driverdata = NULL;
}
EXPORT int SDL_GetNumVideoDrivers(void) { return 1; }
EXPORT const char *SDL_GetVideoDriver(int i) { return i == 0 ? "p8host" : NULL; }
EXPORT const char *SDL_GetCurrentVideoDriver(void) { return "p8host"; }
EXPORT int SDL_GetNumVideoDisplays(void) { return 1; }
EXPORT const char *SDL_GetDisplayName(int i) { (void)i; return "p8host"; }
EXPORT int SDL_GetDisplayBounds(int i, SDL_Rect *r)
{
	(void)i;
	*r = (SDL_Rect){ 0, 0, SCREEN_W, SCREEN_H };
	return 0;
}
EXPORT int SDL_GetNumDisplayModes(int i) { (void)i; return 1; }
EXPORT int SDL_GetDisplayMode(int d, int i, SDL_DisplayMode *m) { (void)d; (void)i; fill_mode(m); return 0; }
EXPORT int SDL_GetCurrentDisplayMode(int d, SDL_DisplayMode *m) { (void)d; fill_mode(m); return 0; }
EXPORT int SDL_GetDesktopDisplayMode(int d, SDL_DisplayMode *m) { (void)d; fill_mode(m); return 0; }

/* ---- windows ---- */
static SDL_PixelFormat argb = {
	.format = PIXELFORMAT_ARGB8888, .BitsPerPixel = 32, .BytesPerPixel = 4,
	.Rmask = 0xff0000, .Gmask = 0xff00, .Bmask = 0xff, .Amask = 0xff000000u,
	.Rshift = 16, .Gshift = 8, .Bshift = 0, .Ashift = 24, .refcount = 1,
};
static struct window *the_window;

EXPORT void *SDL_CreateWindow(const char *t, int x, int y, int w, int h, Uint32 f)
{
	(void)t; (void)x; (void)y; (void)w; (void)h; (void)f;
	struct window *win = calloc(1, sizeof *win);
	if (!win) return NULL;
	win->w = SCREEN_W;
	win->h = SCREEN_H;
	the_window = win;
	return win;
}
EXPORT void SDL_DestroyWindow(void *p)
{
	struct window *w = p;
	if (!w) return;
	if (w->surface) { free(w->surface->pixels); free(w->surface); }
	if (the_window == w) the_window = NULL;
	free(w);
}
EXPORT void SDL_GetWindowSize(void *w, int *x, int *y) { (void)w; if (x) *x = SCREEN_W; if (y) *y = SCREEN_H; }
EXPORT void SDL_GetWindowPosition(void *w, int *x, int *y) { (void)w; if (x) *x = 0; if (y) *y = 0; }
EXPORT int SDL_GetWindowDisplayIndex(void *w) { (void)w; return 0; }
EXPORT Uint32 SDL_GetWindowID(void *w) { (void)w; return 1; }
EXPORT void SDL_SetWindowTitle(void *w, const char *t) { (void)w; (void)t; }
EXPORT void SDL_SetWindowIcon(void *w, void *s) { (void)w; (void)s; }
EXPORT void SDL_SetWindowGrab(void *w, SDL_bool g) { (void)w; (void)g; }
EXPORT void SDL_RaiseWindow(void *w) { (void)w; }
EXPORT void *SDL_GetKeyboardFocus(void) { return the_window; }
EXPORT int SDL_ShowCursor(int t) { (void)t; return 0; }
EXPORT void SDL_WarpMouseInWindow(void *w, int x, int y) { (void)w; (void)x; (void)y; }
EXPORT Uint32 SDL_GetMouseState(int *x, int *y) { if (x) *x = 0; if (y) *y = 0; return 0; }

/* ---- surfaces ---- */
static SDL_Surface *new_surface(void *pixels, int w, int h, int pitch)
{
	SDL_Surface *s = calloc(1, sizeof *s);
	if (!s) return NULL;
	s->format = &argb;
	s->w = w;
	s->h = h;
	s->pitch = pitch;
	s->pixels = pixels;
	s->clip_rect = (SDL_Rect){ 0, 0, w, h };
	s->refcount = 1;
	return s;
}
EXPORT SDL_Surface *SDL_CreateRGBSurfaceFrom(void *px, int w, int h, int depth, int pitch,
					     Uint32 r, Uint32 g, Uint32 b, Uint32 a)
{
	(void)depth; (void)r; (void)g; (void)b; (void)a;
	return new_surface(px, w, h, pitch);
}
EXPORT void SDL_FreeSurface(SDL_Surface *s) { free(s); }
EXPORT int SDL_LockSurface(SDL_Surface *s) { (void)s; return 0; }
EXPORT void SDL_UnlockSurface(SDL_Surface *s) { (void)s; }

/* ---- publishing frames ---- */

/* PICO-8 waits here while the frontend isn't taking frames. */
#define HOST_STALE_NS 200000000ull
static void wait_for_host(void)
{
	if (shm == &local_shm) return;
	uint64_t from = 0;
	for (;;) {
		uint64_t h = __atomic_load_n(&shm->host_ns, __ATOMIC_ACQUIRE);
		uint64_t t = now_ns();
		if (h && t - h < HOST_STALE_NS) {
			if (from) {
				lat.waits++;
				lat.wait_ms += (t - from) / 1e6;
				__atomic_fetch_add(&held_ns, t - from, __ATOMIC_RELAXED);
			}
			return;
		}
		if (!from) from = t;
		SDL_Delay(5);
	}
}

static int lat_on(void) { return shm && shm != &local_shm; }

/* Sample a w x h picture (pitch in pixels) down to 128x128 and publish it. */
static void publish(const uint32_t *px, int w, int h, int pitch)
{
	uint32_t seq = __atomic_load_n(&shm->frame_seq, __ATOMIC_RELAXED);
	__atomic_store_n(&shm->frame_seq, seq + 1, __ATOMIC_RELEASE);
	__atomic_thread_fence(__ATOMIC_SEQ_CST);
	for (int y = 0; y < P8HOST_H; y++) {
		const uint32_t *row = px + (long)(y * h / P8HOST_H + h / P8HOST_H / 2) * pitch;
		for (int x = 0; x < P8HOST_W; x++)
			shm->frame[y * P8HOST_W + x] = row[x * w / P8HOST_W + w / P8HOST_W / 2] | 0xff000000u;
	}
	__atomic_thread_fence(__ATOMIC_SEQ_CST);
	__atomic_store_n(&shm->frame_seq, seq + 2, __ATOMIC_RELEASE);
	__atomic_store_n(&shm->ready, 1, __ATOMIC_RELEASE);
	lat_frame(seq + 2);
	align_frame(now_ns());
}

EXPORT SDL_Surface *SDL_GetWindowSurface(void *p)
{
	struct window *w = p;
	if (!w) return NULL;
	if (!w->surface) {
		void *px = calloc((size_t)w->w * w->h, 4);
		w->surface = px ? new_surface(px, w->w, w->h, w->w * 4) : NULL;
		if (!w->surface) free(px);
	}
	return w->surface;
}
EXPORT int SDL_UpdateWindowSurface(void *p)
{
	struct window *w = p;
	if (!w || !w->surface) return -1;
	/* The picture is the centred square, as PICO-8 lays it out. */
	int side = w->h < w->w ? w->h : w->w;
	const uint32_t *px = (const uint32_t *)w->surface->pixels + (w->w - side) / 2;
	publish(px, side, side, w->w);
	wait_for_host();
	return 0;
}

/* ---- renderer ---- */
static void fill_info(SDL_RendererInfo *i)
{
	memset(i, 0, sizeof *i);
	i->name = "p8host";
	i->flags = 0x2 | 0x4 | 0x8;     /* accelerated, vsync, target textures */
	i->num_texture_formats = 1;
	i->texture_formats[0] = PIXELFORMAT_ARGB8888;
	i->max_texture_width = i->max_texture_height = 4096;
}
EXPORT int SDL_GetNumRenderDrivers(void) { return 1; }
EXPORT int SDL_GetRenderDriverInfo(int i, SDL_RendererInfo *info) { (void)i; fill_info(info); return 0; }
EXPORT void *SDL_CreateRenderer(void *w, int i, Uint32 f)
{
	(void)i; (void)f;
	struct renderer *r = calloc(1, sizeof *r);
	if (r) r->win = w;
	return r;
}
EXPORT void SDL_DestroyRenderer(void *r) { free(r); }
EXPORT int SDL_GetRendererInfo(void *r, SDL_RendererInfo *info) { (void)r; fill_info(info); return 0; }
EXPORT int SDL_SetRenderDrawColor(void *r, Uint8 a, Uint8 b, Uint8 c, Uint8 d)
{
	(void)r; (void)a; (void)b; (void)c; (void)d;
	return 0;
}
EXPORT int SDL_RenderClear(void *r) { (void)r; return 0; }

EXPORT void *SDL_CreateTexture(void *r, Uint32 fmt, int access, int w, int h)
{
	(void)r; (void)fmt; (void)access;
	if (w <= 0 || h <= 0) return NULL;
	struct texture *t = calloc(1, sizeof *t);
	if (!t) return NULL;
	t->px = calloc((size_t)w * h, 4);
	if (!t->px) { free(t); return NULL; }
	t->w = w;
	t->h = h;
	return t;
}
EXPORT void SDL_DestroyTexture(void *p)
{
	struct texture *t = p;
	if (!t) return;
	free(t->px);
	free(t);
}
EXPORT int SDL_UpdateTexture(void *p, const SDL_Rect *rect, const void *pixels, int pitch)
{
	struct texture *t = p;
	if (!t || !pixels) return -1;
	SDL_Rect r = rect ? *rect : (SDL_Rect){ 0, 0, t->w, t->h };
	if (r.x < 0 || r.y < 0 || r.x + r.w > t->w || r.y + r.h > t->h) return -1;
	for (int y = 0; y < r.h; y++)
		memcpy(t->px + (long)(r.y + y) * t->w + r.x, (const char *)pixels + (long)y * pitch, (size_t)r.w * 4);
	return 0;
}
/* The game's picture is whichever texture is drawn biggest this frame. */
EXPORT int SDL_RenderCopy(void *p, void *tex, const SDL_Rect *src, const SDL_Rect *dst)
{
	(void)src;
	struct renderer *r = p;
	if (!r || !tex) return -1;
	long area = dst ? (long)dst->w * dst->h : (long)SCREEN_W * SCREEN_H;
	if (area > r->shown_area) {
		r->shown = tex;
		r->shown_area = area;
	}
	return 0;
}
EXPORT void SDL_RenderPresent(void *p)
{
	struct renderer *r = p;
	if (!r) return;
	if (r->shown) publish(r->shown->px, r->shown->w, r->shown->h, r->shown->w);
	r->shown = NULL;
	r->shown_area = 0;
	wait_for_host();
}

/* ---- OpenGL: not offered ---- */
EXPORT int SDL_GL_SetAttribute(int a, int v) { (void)a; (void)v; return 0; }
EXPORT void *SDL_GL_CreateContext(void *w) { (void)w; return NULL; }
EXPORT void SDL_GL_DeleteContext(void *c) { (void)c; }

/* ---- clipboard ---- */
EXPORT char *SDL_GetClipboardText(void) { return strdup(""); }
EXPORT SDL_bool SDL_HasClipboardText(void) { return 0; }
EXPORT int SDL_SetClipboardText(const char *t) { (void)t; return 0; }

/* ---- keyboard ---- */
enum { SC_C = 6, SC_X = 27, SC_Z = 29, SC_RETURN = 40, SC_RIGHT = 79, SC_LEFT = 80,
       SC_DOWN = 81, SC_UP = 82 };

/* Frontend buttons, as the keys PICO-8 knows: O is Z, X is X, pause is Enter. */
static const struct { int button, scancode; } keymap[] = {
	{ P8B_UP, SC_UP }, { P8B_DOWN, SC_DOWN }, { P8B_LEFT, SC_LEFT }, { P8B_RIGHT, SC_RIGHT },
	{ P8B_B, SC_Z }, { P8B_A, SC_X }, { P8B_Y, SC_C }, { P8B_X, SC_X }, { P8B_START, SC_RETURN },
};
#define NKEYS (int)(sizeof keymap / sizeof keymap[0])

static Uint8 keys[NUM_SCANCODES];
static uint32_t buttons_seen;

EXPORT int SDL_GetKeyFromScancode(int sc)
{
	if (sc >= 4 && sc <= 29) return 'a' + sc - 4;
	if (sc >= 30 && sc <= 38) return '1' + sc - 30;
	if (sc == 39) return '0';
	if (sc == SC_RETURN) return '\r';
	if (sc == 41) return 27;
	if (sc == 42) return 8;
	if (sc == 43) return '\t';
	if (sc == 44) return ' ';
	return sc | (1 << 30);
}
EXPORT const char *SDL_GetScancodeName(int sc) { (void)sc; return ""; }
EXPORT int SDL_GetModState(void) { return 0; }

/* Key changes waiting to be delivered as events. */
static SDL_Event pending[64];
static int npending;

static void sync_keys(void)
{
	lat_poll();
	uint32_t b = __atomic_load_n(&shm->buttons, __ATOMIC_ACQUIRE);
	if (b == buttons_seen) return;
	lat_read(b);
	Uint8 want[NUM_SCANCODES] = { 0 };
	for (int i = 0; i < NKEYS; i++)
		if (b & (1u << keymap[i].button)) want[keymap[i].scancode] = 1;
	for (int i = 0; i < NKEYS; i++) {
		int sc = keymap[i].scancode;
		if (want[sc] == keys[sc]) continue;
		keys[sc] = want[sc];
		if (npending < (int)(sizeof pending / sizeof pending[0])) {
			SDL_Event *e = &pending[npending++];
			memset(e, 0, sizeof *e);
			e->key.type = want[sc] ? SDL_KEYDOWN : SDL_KEYUP;
			e->key.timestamp = SDL_GetTicks();
			e->key.windowID = 1;
			e->key.state = want[sc];
			e->key.keysym.scancode = sc;
			e->key.keysym.sym = SDL_GetKeyFromScancode(sc);
		}
	}
	buttons_seen = b;
}

EXPORT const Uint8 *SDL_GetKeyboardState(int *n)
{
	sync_keys();
	if (n) *n = NUM_SCANCODES;
	return keys;
}

EXPORT int SDL_PollEvent(SDL_Event *e)
{
	sync_keys();
	if (!npending) return 0;
	if (e) {
		*e = pending[0];
		memmove(pending, pending + 1, (size_t)(npending - 1) * sizeof *pending);
	}
	npending--;
	return 1;
}

/* ---- game controllers: none, the buttons arrive as keys ---- */
EXPORT int SDL_NumJoysticks(void) { return 0; }
EXPORT SDL_bool SDL_IsGameController(int i) { (void)i; return 0; }
EXPORT int SDL_GameControllerAddMapping(const char *m) { (void)m; return 0; }
EXPORT void *SDL_GameControllerOpen(int i) { (void)i; return NULL; }
EXPORT void SDL_GameControllerClose(void *c) { (void)c; }
EXPORT SDL_bool SDL_GameControllerGetAttached(void *c) { (void)c; return 0; }
EXPORT Sint16 SDL_GameControllerGetAxis(void *c, int a) { (void)c; (void)a; return 0; }
EXPORT Uint8 SDL_GameControllerGetButton(void *c, int b) { (void)c; (void)b; return 0; }
EXPORT void *SDL_GameControllerGetJoystick(void *c) { (void)c; return NULL; }
EXPORT char *SDL_GameControllerMapping(void *c) { (void)c; return NULL; }
EXPORT const char *SDL_GameControllerNameForIndex(int i) { (void)i; return NULL; }
EXPORT void *SDL_JoystickOpen(int i) { (void)i; return NULL; }
EXPORT void SDL_JoystickClose(void *j) { (void)j; }
EXPORT int SDL_JoystickInstanceID(void *j) { (void)j; return -1; }
EXPORT const char *SDL_JoystickNameForIndex(int i) { (void)i; return NULL; }
EXPORT int SDL_JoystickNumAxes(void *j) { (void)j; return 0; }
EXPORT int SDL_JoystickNumBalls(void *j) { (void)j; return 0; }
EXPORT int SDL_JoystickNumButtons(void *j) { (void)j; return 0; }
EXPORT Sint16 SDL_JoystickGetAxis(void *j, int a) { (void)j; (void)a; return 0; }
EXPORT Uint8 SDL_JoystickGetButton(void *j, int b) { (void)j; (void)b; return 0; }

/* ---- audio ---- */
static pthread_mutex_t audio_mu;
static pthread_once_t audio_once = PTHREAD_ONCE_INIT;
static SDL_AudioSpec spec;
static int audio_open, audio_paused = 1, audio_started;
static pthread_t audio_thread;

static void audio_mu_init(void)
{
	pthread_mutexattr_t a;
	pthread_mutexattr_init(&a);
	pthread_mutexattr_settype(&a, PTHREAD_MUTEX_RECURSIVE);
	pthread_mutex_init(&audio_mu, &a);
	pthread_mutexattr_destroy(&a);
}
EXPORT void SDL_LockAudio(void) { pthread_once(&audio_once, audio_mu_init); pthread_mutex_lock(&audio_mu); }
EXPORT void SDL_UnlockAudio(void) { pthread_once(&audio_once, audio_mu_init); pthread_mutex_unlock(&audio_mu); }

/* Keep about this many samples queued ahead of the frontend. */
#define AUDIO_AHEAD 2048

static void *audio_main(void *arg)
{
	(void)arg;
	int frames = spec.samples ? spec.samples : 1024;
	int ch = spec.channels ? spec.channels : 1;
	int16_t *buf = malloc((size_t)frames * ch * 2);
	if (!buf) return NULL;
	for (;;) {
		uint32_t w = shm->audio_w;
		uint32_t r = __atomic_load_n(&shm->audio_r, __ATOMIC_ACQUIRE);
		if (audio_paused || (uint32_t)(w - r) + (uint32_t)frames > AUDIO_AHEAD) {
			SDL_Delay(2);
			continue;
		}
		SDL_LockAudio();
		spec.callback(spec.userdata, (Uint8 *)buf, frames * ch * 2);
		SDL_UnlockAudio();
		for (int i = 0; i < frames; i++) {
			int s = buf[i * ch];                     /* mono: the first channel */
			shm->audio[(w + i) & (P8HOST_AUDIO - 1)] = (int16_t)s;
		}
		__atomic_store_n(&shm->audio_w, w + frames, __ATOMIC_RELEASE);
	}
	return NULL;
}

EXPORT int SDL_GetNumAudioDrivers(void) { return 1; }
EXPORT const char *SDL_GetAudioDriver(int i) { return i == 0 ? "p8host" : NULL; }
EXPORT const char *SDL_GetCurrentAudioDriver(void) { return audio_open ? "p8host" : NULL; }

EXPORT int SDL_OpenAudio(SDL_AudioSpec *want, SDL_AudioSpec *have)
{
	pthread_once(&audio_once, audio_mu_init);
	if (audio_open || !want || !want->callback) return -1;
	spec = *want;
	/* Only 16-bit at PICO-8's own rate is offered; SDL converts for a
	 * caller passing no "have", and PICO-8 asks for exactly this. */
	spec.format = 0x8010;                           /* AUDIO_S16LSB */
	spec.freq = P8HOST_RATE;
	if (!spec.channels) spec.channels = 1;
	if (!spec.samples) spec.samples = 1024;
	spec.silence = 0;
	spec.size = (Uint32)spec.samples * spec.channels * 2;
	if (have) *have = spec;
	audio_open = 1;
	return 0;
}

EXPORT void SDL_PauseAudio(int pause)
{
	audio_paused = pause;
	if (!pause && audio_open && !audio_started)
		audio_started = pthread_create(&audio_thread, NULL, audio_main, NULL) == 0;
}
