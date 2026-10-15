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
}

/* ---- core ---- */
EXPORT int SDL_Init(Uint32 f) { (void)f; return 0; }
EXPORT int SDL_InitSubSystem(Uint32 f) { (void)f; return 0; }
EXPORT void SDL_GetVersion(SDL_version *v) { v->major = 2; v->minor = 0; v->patch = 20; }
EXPORT const char *SDL_GetError(void) { return ""; }
EXPORT void SDL_ClearError(void) {}
EXPORT SDL_bool SDL_SetHint(const char *n, const char *v) { (void)n; (void)v; return 1; }
EXPORT void SDL_free(void *p) { free(p); }
EXPORT Uint32 SDL_GetTicks(void) { return (Uint32)((now_ns() - start_ns) / 1000000); }
EXPORT void SDL_Delay(Uint32 ms)
{
	struct timespec ts = { ms / 1000, (long)(ms % 1000) * 1000000 };
	while (nanosleep(&ts, &ts) != 0) {}
}

/* ---- threads ---- */
struct thread { pthread_t t; int (*fn)(void *); void *data; };
static void *thread_main(void *p)
{
	struct thread *t = p;
	return (void *)(intptr_t)t->fn(t->data);
}
EXPORT void *SDL_CreateThread(int (*fn)(void *), const char *name, void *data)
{
	(void)name;
	struct thread *t = calloc(1, sizeof *t);
	if (!t) return NULL;
	t->fn = fn;
	t->data = data;
	if (pthread_create(&t->t, NULL, thread_main, t) != 0) { free(t); return NULL; }
	return t;
}
EXPORT void SDL_DetachThread(void *p) { if (p) pthread_detach(((struct thread *)p)->t); }

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
	for (;;) {
		uint64_t h = __atomic_load_n(&shm->host_ns, __ATOMIC_ACQUIRE);
		if (h && now_ns() - h < HOST_STALE_NS) return;
		SDL_Delay(5);
	}
}

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
	uint32_t b = __atomic_load_n(&shm->buttons, __ATOMIC_ACQUIRE);
	if (b == buttons_seen) return;
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
