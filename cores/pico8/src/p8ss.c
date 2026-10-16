/*
 * p8ss: save states for the official PICO-8 binary, loaded with LD_PRELOAD.
 *
 * How it works:
 *  - malloc/calloc/realloc/free are interposed. Calls whose return address
 *    lies inside the PICO-8 executable's code are served from a private
 *    arena; everything else (libc, SDL, GPU and audio drivers) goes to the
 *    normal glibc heap. The arena's allocator metadata lives inside the
 *    arena, so copying the arena copies the allocator state too.
 *  - SDL_PollEvent is interposed. It is PICO-8's only call into the event
 *    queue and it runs at least once per frame, so it is the safe point
 *    where pending save/load requests are carried out.
 *  - A snapshot is: the executable's writable segments (.data/.bss), the
 *    used part of the arena, the main thread's stack from the hook up to
 *    where main's caller began, and a ucontext captured in the hook.
 *    Loading copies all of that back from a private stack and jumps into
 *    the saved context, so a state can be saved at one depth (e.g. inside
 *    a cart's flip() loop) and loaded at another.
 *  - A state can be loaded into a later run of PICO-8. Pointers into
 *    libraries, the executable and the stack are relocated using the
 *    saving run's memory map; pointers to objects outside PICO-8's memory
 *    (SDL window, renderer, textures) keep this run's values; glibc's
 *    mangled pointers and stack canary are re-encoded for this run.
 *
 * Requests: keyboard hotkeys (P8SS_SAVE_KEY / P8SS_LOAD_KEY, SDL keycodes,
 * default F5 / F8) or signals (SIGUSR1 save, SIGUSR2 load) for firmware
 * hotkey daemons. States are kept in memory and written to P8SS_FILE;
 * with no state in memory, a load reads P8SS_FILE. P8SS_AUTOLOAD=N loads
 * P8SS_FILE automatically once PICO-8 has shown N frames.
 */
#define _GNU_SOURCE
#include <dirent.h>
#include <sys/stat.h>
#include <dlfcn.h>
#include <link.h>
#include <pthread.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <ucontext.h>
#include <unistd.h>

#include "statefile.h"

/* ---- glibc's own allocator, used for everything not owned by PICO-8 ---- */
extern void *__libc_malloc(size_t);
extern void *__libc_calloc(size_t, size_t);
extern void *__libc_realloc(void *, size_t);
extern void __libc_free(void *);

/* ---- logging ---- */
static int log_enabled = 1;
static int debug_enabled;
#define LOG(...) do { if (log_enabled) { fprintf(stderr, "[p8ss] " __VA_ARGS__); fputc('\n', stderr); } } while (0)

/* ---- PICO-8 executable layout ---- */
static uintptr_t text_lo, text_hi;
#define MAX_SEGS 8
static struct { uintptr_t lo, hi; } segs[MAX_SEGS];
static int nsegs;
static uintptr_t stack_top;

static int phdr_cb(struct dl_phdr_info *info, size_t size, void *data)
{
	(void)size; (void)data;
	/* The first object is the main executable. Its RELRO range becomes
	 * read-only after relocation and must not be written on restore. */
	uintptr_t relro_end = 0;
	uintptr_t page = (uintptr_t)sysconf(_SC_PAGESIZE);
	for (int i = 0; i < info->dlpi_phnum; i++) {
		const ElfW(Phdr) *ph = &info->dlpi_phdr[i];
		if (ph->p_type == PT_GNU_RELRO)
			relro_end = (info->dlpi_addr + ph->p_vaddr + ph->p_memsz) & ~(page - 1);
	}
	for (int i = 0; i < info->dlpi_phnum; i++) {
		const ElfW(Phdr) *ph = &info->dlpi_phdr[i];
		if (ph->p_type != PT_LOAD)
			continue;
		uintptr_t lo = info->dlpi_addr + ph->p_vaddr;
		uintptr_t hi = lo + ph->p_memsz;
		if (ph->p_flags & PF_X) {
			if (!text_lo || lo < text_lo) text_lo = lo;
			if (hi > text_hi) text_hi = hi;
		} else if ((ph->p_flags & PF_W) && nsegs < MAX_SEGS) {
			if (lo < relro_end && relro_end < hi)
				lo = relro_end;
			segs[nsegs].lo = lo;
			segs[nsegs].hi = hi;
			nsegs++;
		}
	}
	return 1;
}

static int from_pico8(void *ret)
{
	uintptr_t r = (uintptr_t)ret;
	return r >= text_lo && r < text_hi;
}

/* ---- the arena ---- */
#define ARENA_RESERVE ((size_t)1 << 30)
#define ALIGN 16
#define NCLASSES 64

struct chunk { size_t cap; size_t pad; };   /* precedes every block */
struct freeblk { struct chunk h; struct freeblk *next; };

struct arena_hdr {
	uintptr_t top;                   /* first unused byte */
	struct freeblk *bins[NCLASSES];
};

static unsigned char *arena;
static struct arena_hdr *ah;
static pthread_mutex_t arena_lock = PTHREAD_MUTEX_INITIALIZER;

static int in_arena(void *p)
{
	return arena && (unsigned char *)p >= arena && (unsigned char *)p < arena + ARENA_RESERVE;
}

/* Small sizes get exact 16-byte classes; larger ones power-of-two classes. */
static int size_class(size_t cap)
{
	if (cap <= 32 * ALIGN)
		return (int)(cap / ALIGN) - 1;           /* 0..31 */
	int c = 32;
	size_t lim = 64 * ALIGN;
	while (cap > lim && c < NCLASSES - 1) { lim <<= 1; c++; }
	return c;
}

static size_t round_cap(size_t n)
{
	size_t cap = (n + ALIGN - 1) & ~(size_t)(ALIGN - 1);
	if (cap == 0) cap = ALIGN;
	if (cap > 32 * ALIGN) {
		/* Round big blocks to their class limit so reuse is exact-fit. */
		size_t lim = 64 * ALIGN;
		while (cap > lim) lim <<= 1;
		if (lim - cap < cap / 8) cap = lim;      /* keep waste modest */
	}
	return cap;
}

static void arena_init(void)
{
	if (arena)
		return;
	/* A fixed hint keeps the arena at the same address across runs, which a state
	 * loaded into a later run needs. It has to exist on the smallest address space
	 * there is: arm64 kernels built for 39-bit addresses (the H700's) end user space
	 * at 0x7fffffffff, and a hint past that is ignored for a random address. 360 GB
	 * is under that, and above where the executable and its heap are put. */
	void *hint = (void *)(uintptr_t)(sizeof(void *) == 8 ? 0x5a00000000ULL : 0x60000000UL);
	arena = mmap(hint, ARENA_RESERVE, PROT_READ | PROT_WRITE,
		     MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE, -1, 0);
	if (arena == MAP_FAILED) {
		arena = NULL;
		return;
	}
	if (arena != hint)
		LOG("arena at %p, not %p: states will not load into a later run", (void *)arena, hint);
	ah = (struct arena_hdr *)arena;
	memset(ah, 0, sizeof *ah);
	ah->top = ((uintptr_t)arena + sizeof *ah + ALIGN - 1) & ~(uintptr_t)(ALIGN - 1);
}

static void *arena_alloc(size_t n)
{
	size_t cap = round_cap(n);
	int c = size_class(cap);
	pthread_mutex_lock(&arena_lock);
	struct freeblk **pp = &ah->bins[c];
	for (; *pp; pp = &(*pp)->next) {
		if ((*pp)->h.cap >= cap) {
			struct freeblk *b = *pp;
			*pp = b->next;
			pthread_mutex_unlock(&arena_lock);
			return (struct chunk *)b + 1;
		}
	}
	struct chunk *h = (struct chunk *)ah->top;
	if ((unsigned char *)h + sizeof *h + cap > arena + ARENA_RESERVE) {
		pthread_mutex_unlock(&arena_lock);
		return NULL;
	}
	ah->top += sizeof *h + cap;
	h->cap = cap;
	pthread_mutex_unlock(&arena_lock);
	return h + 1;
}

static void arena_free(void *p)
{
	struct freeblk *b = (struct freeblk *)((struct chunk *)p - 1);
	int c = size_class(b->h.cap);
	pthread_mutex_lock(&arena_lock);
	b->next = ah->bins[c];
	ah->bins[c] = b;
	pthread_mutex_unlock(&arena_lock);
}

static size_t arena_cap(void *p)
{
	return ((struct chunk *)p - 1)->cap;
}

/* ---- interposed allocator ---- */
static int ready;

void *malloc(size_t n)
{
	if (ready && from_pico8(__builtin_return_address(0))) {
		void *p = arena_alloc(n);
		if (p) return p;
	}
	return __libc_malloc(n);
}

void *calloc(size_t a, size_t b)
{
	if (ready && from_pico8(__builtin_return_address(0))) {
		if (b && a > SIZE_MAX / b) return NULL;
		void *p = arena_alloc(a * b);
		if (p) { memset(p, 0, a * b); return p; }
	}
	return __libc_calloc(a, b);
}

void free(void *p)
{
	if (!p) return;
	if (in_arena(p)) arena_free(p);
	else __libc_free(p);
}

void *realloc(void *p, size_t n)
{
	int mine = ready && from_pico8(__builtin_return_address(0));
	if (!p) {
		if (mine) { void *q = arena_alloc(n); if (q) return q; }
		return __libc_malloc(n);
	}
	if (!in_arena(p))
		return __libc_realloc(p, n);
	if (n == 0) { arena_free(p); return NULL; }
	size_t cap = arena_cap(p);
	if (n <= cap) return p;
	void *q = arena_alloc(n);
	if (!q) return NULL;
	memcpy(q, p, cap);
	arena_free(p);
	return q;
}

/* ---- small parsers ----
 * Hand-rolled rather than sscanf/strtol: glibc 2.38 headers redirect those
 * to new symbols, and the H700 firmwares ship older glibc (BaseOS: 2.35). */
static unsigned long parse_num(const char **sp, int base)
{
	const char *s = *sp;
	unsigned long v = 0;
	while (*s == ' ') s++;
	if (base == 0) {
		base = 10;
		if (s[0] == '0' && (s[1] == 'x' || s[1] == 'X')) { base = 16; s += 2; }
	}
	for (;; s++) {
		int d;
		if (*s >= '0' && *s <= '9') d = *s - '0';
		else if (*s >= 'a' && *s <= 'f') d = *s - 'a' + 10;
		else if (*s >= 'A' && *s <= 'F') d = *s - 'A' + 10;
		else break;
		if (d >= base) break;
		v = v * (unsigned long)base + (unsigned long)d;
	}
	*sp = s;
	return v;
}

static long env_num(const char *v)
{
	int neg = *v == '-';
	const char *s = v + neg;
	long n = (long)parse_num(&s, 0);
	return neg ? -n : n;
}

static const char *skip_field(const char *s)
{
	while (*s == ' ') s++;
	while (*s && *s != ' ' && *s != '\n') s++;
	return s;
}

/* ---- process memory map ---- */
/*
 * A state records the saving process's memory map so pointers can be
 * translated when it is loaded into another process. Every mapping is one
 * of: the PICO-8 executable or a shared library (relocated by that
 * object's load address; an anonymous mapping directly after a file
 * mapping is that object's .bss), the main stack (relocated by the stack
 * end), the arena (fixed address), or foreign memory such as the glibc
 * heap, which cannot be translated by address alone.
 */
enum { R_OBJ, R_STACK, R_ARENA, R_FOREIGN };

#define MAX_REGIONS 2048
#define MAX_OBJS 256
#define NAME_LEN 128

struct region { uintptr_t lo, hi; int kind, obj; };
struct obj { char name[NAME_LEN]; uintptr_t base; };
struct pmap {
	struct region r[MAX_REGIONS];
	int nr;
	struct obj o[MAX_OBJS];
	int no;
};

static int find_obj(const struct pmap *m, const char *name)
{
	for (int i = 0; i < m->no; i++)
		if (!strcmp(m->o[i].name, name)) return i;
	return -1;
}

static void read_pmap(struct pmap *m)
{
	FILE *f = fopen("/proc/self/maps", "r");
	char line[512];
	m->nr = m->no = 0;
	while (f && m->nr < MAX_REGIONS && fgets(line, sizeof line, f)) {
		/* lo-hi perms offset dev inode   name */
		const char *q = line;
		unsigned long lo = parse_num(&q, 16);
		if (*q++ != '-') continue;
		unsigned long hi = parse_num(&q, 16);
		q = skip_field(q);
		unsigned long off = parse_num(&q, 16);
		q = skip_field(skip_field(q));
		while (*q == ' ') q++;
		char name[NAME_LEN];
		size_t nl = strcspn(q, "\n");
		if (nl >= NAME_LEN) nl = NAME_LEN - 1;
		memcpy(name, q, nl);
		name[nl] = 0;
		struct region *r = &m->r[m->nr];
		r->lo = lo; r->hi = hi; r->obj = -1;
		if (in_arena((void *)lo)) {
			r->kind = R_ARENA;
		} else if (!strcmp(name, "[stack]")) {
			r->kind = R_STACK;
		} else if (name[0] == '/' || !strcmp(name, "[vdso]")) {
			int o = find_obj(m, name);
			if (o < 0 && m->no < MAX_OBJS) {
				o = m->no++;
				strcpy(m->o[o].name, name);
				m->o[o].base = lo - off;
			}
			r->kind = o < 0 ? R_FOREIGN : R_OBJ;
			r->obj = o;
		} else if (!name[0] && m->nr > 0 && m->r[m->nr - 1].kind == R_OBJ &&
			   m->r[m->nr - 1].hi == lo) {
			r->kind = R_OBJ;                         /* the object's .bss */
			r->obj = m->r[m->nr - 1].obj;
		} else {
			r->kind = R_FOREIGN;
		}
		m->nr++;
	}
	if (f) fclose(f);
}

static const struct region *lookup(const struct pmap *m, uintptr_t v)
{
	int lo = 0, hi = m->nr - 1;                      /* maps are sorted */
	while (lo <= hi) {
		int mid = (lo + hi) / 2;
		if (v < m->r[mid].lo) hi = mid - 1;
		else if (v >= m->r[mid].hi) lo = mid + 1;
		else return &m->r[mid];
	}
	return NULL;
}

/* ---- per-process secrets glibc mixes into the stack ---- */
#if defined(__x86_64__)
static uintptr_t get_canary(void) { uintptr_t v; __asm__("mov %%fs:0x28, %0" : "=r"(v)); return v; }
static uintptr_t get_ptr_guard(void) { uintptr_t v; __asm__("mov %%fs:0x30, %0" : "=r"(v)); return v; }
static uintptr_t ror(uintptr_t v, int n) { return (v >> n) | (v << (64 - n)); }
static uintptr_t demangle(uintptr_t v, uintptr_t g) { return ror(v, 0x11) ^ g; }
static uintptr_t mangle(uintptr_t v, uintptr_t g) { return ror(v ^ g, 64 - 0x11); }
#elif defined(__aarch64__)
extern uintptr_t __stack_chk_guard __attribute__((weak));
extern uintptr_t __pointer_chk_guard __attribute__((weak));
static uintptr_t get_canary(void) { return &__stack_chk_guard ? __stack_chk_guard : 0; }
static uintptr_t get_ptr_guard(void) { return &__pointer_chk_guard ? __pointer_chk_guard : 0; }
static uintptr_t demangle(uintptr_t v, uintptr_t g) { return v ^ g; }
static uintptr_t mangle(uintptr_t v, uintptr_t g) { return v ^ g; }
#else
static uintptr_t get_canary(void) { return 0; }
static uintptr_t get_ptr_guard(void) { return 0; }
static uintptr_t demangle(uintptr_t v, uintptr_t g) { (void)g; return v; }
static uintptr_t mangle(uintptr_t v, uintptr_t g) { (void)g; return v; }
#endif

/* ---- PICO-8's clocks ----
 * PICO-8 paces frames with SDL_GetTicks, clock() and clock_gettime. In a
 * new run those start again from zero, so a loaded state would wait until
 * they caught up with the saving run. Calls made from PICO-8's own code
 * see each clock shifted so it carries on from the moment of the save.
 */
#include <time.h>
#define NCLOCKS 8                      /* clockids 0..7: monotonic, cpu... */
static int64_t clock_off_ns[NCLOCKS];  /* clock_gettime offsets */
static int64_t clock_c_off;            /* clock() offset, in clock_t units */
static int64_t ticks_off;              /* SDL_GetTicks offset, ms */
static uint32_t (*real_get_ticks)(void);
static int (*real_clock_gettime)(clockid_t, struct timespec *);
static clock_t (*real_clock)(void);

static int virtual_clock(clockid_t id)
{
	return id >= 0 && id < NCLOCKS && id != CLOCK_REALTIME && id != CLOCK_REALTIME_COARSE;
}

static int64_t real_ns(clockid_t id)
{
	struct timespec ts;
	if (real_clock_gettime(id, &ts) != 0) return 0;
	return (int64_t)ts.tv_sec * 1000000000 + ts.tv_nsec;
}

uint32_t SDL_GetTicks(void)
{
	if (!real_get_ticks) real_get_ticks = dlsym(RTLD_NEXT, "SDL_GetTicks");
	return (uint32_t)(real_get_ticks() + ticks_off);
}

int clock_gettime(clockid_t id, struct timespec *ts)
{
	if (!real_clock_gettime) real_clock_gettime = dlsym(RTLD_NEXT, "clock_gettime");
	int r = real_clock_gettime(id, ts);
	if (r == 0 && virtual_clock(id) && clock_off_ns[id] &&
	    from_pico8(__builtin_return_address(0))) {
		int64_t ns = (int64_t)ts->tv_sec * 1000000000 + ts->tv_nsec + clock_off_ns[id];
		ts->tv_sec = ns / 1000000000;
		ts->tv_nsec = ns % 1000000000;
	}
	return r;
}

clock_t clock(void)
{
	if (!real_clock) real_clock = dlsym(RTLD_NEXT, "clock");
	clock_t c = real_clock();
	if (from_pico8(__builtin_return_address(0))) c += clock_c_off;
	return c;
}

struct clock_state { int64_t ns[NCLOCKS]; int64_t c; int64_t ticks; };

/* What PICO-8 sees on each clock right now. */
static void read_clocks(struct clock_state *cs)
{
	for (int i = 0; i < NCLOCKS; i++)
		cs->ns[i] = virtual_clock(i) ? real_ns(i) + clock_off_ns[i] : 0;
	cs->c = (int64_t)real_clock() + clock_c_off;
	cs->ticks = (int64_t)real_get_ticks() + ticks_off;
}

/* Make PICO-8's clocks read cs from now on. */
static void set_clocks(const struct clock_state *cs)
{
	for (int i = 0; i < NCLOCKS; i++)
		if (virtual_clock(i)) clock_off_ns[i] = cs->ns[i] - real_ns(i);
	clock_c_off = cs->c - (int64_t)real_clock();
	ticks_off = cs->ticks - (int64_t)real_get_ticks();
	if (debug_enabled)
		LOG("clocks: ticks %lld (off %lld), clock %lld (off %lld), mono %lld ns (off %lld)",
		    (long long)cs->ticks, (long long)ticks_off, (long long)cs->c, (long long)clock_c_off,
		    (long long)cs->ns[CLOCK_MONOTONIC], (long long)clock_off_ns[CLOCK_MONOTONIC]);
}

/* ---- snapshots ---- */
#define STATE_MAGIC "P8SS0005"

struct state_hdr {
	char magic[8];
	uint32_t word_size, pid;
	uint64_t start_time;              /* with pid, identifies the process */
	uint64_t exe_hash;                /* states only load into the same PICO-8 */
	uintptr_t canary, ptr_guard;
	uintptr_t stack_lo, stack_end;    /* saved stack is [stack_lo, stack_end) */
	uintptr_t arena_addr;
	uint64_t segs_len, stack_len, arena_len;
	struct clock_state clocks;        /* PICO-8's clocks at the save */
	ucontext_t ctx;
};

struct snapshot {
	int valid;
	struct state_hdr h;
	struct pmap map;                  /* the saving process's memory map */
	unsigned char *buf;               /* segs, then stack, then arena */
	size_t buf_cap;
};

static struct snapshot snap;
static struct pmap cur_map;
static volatile int just_restored;     /* lives in our .so, not restored */
static volatile sig_atomic_t want_save, want_load;
static int save_key = 0x4000003e;      /* SDLK_F5 */
static int load_key = 0x40000041;      /* SDLK_F8 */
static const char *state_file;
static long autoload_frames = -1;
static volatile long frames;           /* frames PICO-8 has put on screen */

static size_t segs_size(void)
{
	size_t n = 0;
	for (int i = 0; i < nsegs; i++) n += segs[i].hi - segs[i].lo;
	return n;
}

static uint64_t process_start_time(void)
{
	char buf[1024];
	FILE *f = fopen("/proc/self/stat", "r");
	if (!f) return 0;
	size_t n = fread(buf, 1, sizeof buf - 1, f);
	fclose(f);
	buf[n] = 0;
	char *p = strrchr(buf, ')');                 /* skip "pid (comm)" */
	if (!p) return 0;
	/* starttime is the 20th field after the command name. */
	const char *q = p + 1;
	for (int i = 0; i < 19; i++) q = skip_field(q);
	return parse_num(&q, 10);
}

static uint64_t fnv(uint64_t h, const unsigned char *p, const unsigned char *end)
{
	for (; p < end; p++)
		h = (h ^ *p) * 1099511628211ULL;
	return h;
}

static int self_text_cb(struct dl_phdr_info *info, size_t size, void *data)
{
	(void)size;
	uint64_t *h = data;
	for (int i = 0; i < info->dlpi_phnum; i++) {
		const ElfW(Phdr) *ph = &info->dlpi_phdr[i];
		uintptr_t lo = info->dlpi_addr + ph->p_vaddr;
		if (ph->p_type == PT_LOAD && (ph->p_flags & PF_X) &&
		    (uintptr_t)&self_text_cb >= lo && (uintptr_t)&self_text_cb < lo + ph->p_memsz) {
			*h = fnv(*h, (void *)lo, (void *)(lo + ph->p_memsz));
			return 1;
		}
	}
	return 0;
}

/* A state holds return addresses into PICO-8 and into this library, so it
 * only loads into the same build of both. */
static uint64_t exe_hash(void)
{
	uint64_t h = fnv(1469598103934665603ULL, (void *)text_lo, (void *)text_hi);
	dl_iterate_phdr(self_text_cb, &h);
	return h;
}

/* Audio callbacks read PICO-8 memory from SDL's audio thread. */
static void (*real_lock_audio)(void);
static void (*real_unlock_audio)(void);

static void audio_lock(void)   { if (real_lock_audio) real_lock_audio(); }
static void audio_unlock(void) { if (real_unlock_audio) real_unlock_audio(); }

static uint64_t state_exe_hash;

/* ---- state files ----
 * File: struct state_hdr, then u32 flags, u64 payload length, u64 stored
 * length, then the payload (zlib-compressed when flags has SF_ZLIB): the id
 * of the page dictionary used (zero for none), the saving process's memory
 * map, then the data, stack and arena sections in the encoding of
 * statefile.c.
 */
#define SF_ZLIB 1

static int64_t now_ms(void) { return real_ns(CLOCK_MONOTONIC) / 1000000; }

/* The page dictionary (see statefile.h). Dictionaries live next to the
 * states, named by build and id: p8ss-pages-<build>-<id>.bin. The first
 * save with none makes one from its own memory, once PICO-8 has fully
 * started, and a save that finds most of its memory missing from the
 * dictionary (it was made too early, or by a different game) makes a new
 * one. Old ones stay for the states that use them. */
static char *dict_dir;
static struct sf_dict dict;                     /* hashes only, for saving */
static int dict_state;                          /* 0 not looked for, 1 read, -1 none */
static int dict_made;                           /* this process made one already */

/* Make a new dictionary when a state would store more pages than this in
 * full (16 MiB): a good one leaves a few hundred. */
#define DICT_STALE_PAGES 4096

static void dict_name(char *out, size_t n, struct sf_hash id)
{
	snprintf(out, n, "%s/p8ss-pages-%016llx-%016llx.bin", dict_dir,
		 (unsigned long long)state_exe_hash, (unsigned long long)id.a);
}

/* The newest dictionary for this build, if any. */
static void find_dict(void)
{
	dict_state = -1;
	DIR *d = dict_dir ? opendir(dict_dir) : NULL;
	if (!d) return;
	char prefix[64], best[4096] = "", path[4096];
	snprintf(prefix, sizeof prefix, "p8ss-pages-%016llx-", (unsigned long long)state_exe_hash);
	time_t best_t = 0;
	struct dirent *e;
	while ((e = readdir(d))) {
		if (strncmp(e->d_name, prefix, strlen(prefix)) || !strstr(e->d_name, ".bin")) continue;
		snprintf(path, sizeof path, "%s/%s", dict_dir, e->d_name);
		struct stat st;
		if (stat(path, &st) == 0 && (!best[0] || st.st_mtime >= best_t)) {
			best_t = st.st_mtime;
			snprintf(best, sizeof best, "%s", path);
		}
	}
	closedir(d);
	const char *why;
	if (best[0]) {
		if (sf_dict_read(best, &dict, 0, &why) == 0) dict_state = 1;
		else LOG("not using %s: %s", best, why);
	}
}

/* Make a dictionary from the arena in snap and use it from now on. */
static void make_dict(const unsigned char *arena_copy, size_t len)
{
	dict_made = 1;
	int64_t t0 = now_ms();
	char tmp[4096], path[4096];
	snprintf(tmp, sizeof tmp, "%s/p8ss-pages.%d.tmp", dict_dir, (int)getpid());
	struct sf_hash id;
	if (sf_dict_write(tmp, arena_copy, len, &id) != 0) {
		LOG("writing a page dictionary in %s failed", dict_dir);
		unlink(tmp);
		return;
	}
	dict_name(path, sizeof path, id);
	if (rename(tmp, path) != 0) { unlink(tmp); return; }
	struct sf_dict d;
	const char *why;
	if (sf_dict_read(path, &d, 0, &why) != 0) return;
	sf_dict_free(&dict);
	dict = d;
	dict_state = 1;
	LOG("page dictionary at %s (%lld ms)", path, (long long)(now_ms() - t0));
}

/* Encode snap's memory into raw. Returns 0, with the number of arena pages
 * stored in full. */
static int encode_snap(struct sf_buf *raw, const struct sf_index *d, struct sf_hash id, size_t *full)
{
	const unsigned char *b = snap.buf;
	raw->len = 0;
	return sf_put(raw, &id, sizeof id) == 0 &&
	       sf_put(raw, &snap.map, sizeof snap.map) == 0 &&
	       sf_encode(raw, b, snap.h.segs_len, d, NULL) == 0 &&
	       sf_encode(raw, b + snap.h.segs_len, snap.h.stack_len, d, NULL) == 0 &&
	       sf_encode(raw, b + snap.h.segs_len + snap.h.stack_len, snap.h.arena_len, d, full) == 0
	       ? 0 : -1;
}

static int write_state_file(void)
{
	if (!state_file) return 0;
	int64_t t0 = now_ms();
	struct sf_buf raw = { 0 }, packed = { 0 };
	if (!dict_state) find_dict();
	struct sf_hash none = { 0, 0 };
	size_t full = 0;
	int ok = encode_snap(&raw, dict_state > 0 ? &dict.idx : NULL,
			     dict_state > 0 ? dict.id : none, &full) == 0;
	if (ok && full > DICT_STALE_PAGES && dict_dir && !dict_made) {
		make_dict(snap.buf + snap.h.segs_len + snap.h.stack_len, snap.h.arena_len);
		if (dict_state > 0) ok = encode_snap(&raw, &dict.idx, dict.id, &full) == 0;
	}
	uint32_t flags = 0;
	const struct sf_buf *payload = &raw;
	if (ok && sf_compress(&raw, &packed) == 0) {
		flags |= SF_ZLIB;
		payload = &packed;
	}
	uint64_t raw_len = raw.len, stored_len = payload->len;

	char tmp[4096];
	snprintf(tmp, sizeof tmp, "%s.tmp", state_file);
	FILE *f = ok ? fopen(tmp, "wb") : NULL;
	if (f) {
		ok = fwrite(&snap.h, sizeof snap.h, 1, f) == 1 &&
		     fwrite(&flags, sizeof flags, 1, f) == 1 &&
		     fwrite(&raw_len, sizeof raw_len, 1, f) == 1 &&
		     fwrite(&stored_len, sizeof stored_len, 1, f) == 1 &&
		     fwrite(payload->p, 1, payload->len, f) == payload->len;
		ok = fflush(f) == 0 && fsync(fileno(f)) == 0 && ok;
		ok = fclose(f) == 0 && ok;
	} else {
		ok = 0;
	}
	if (ok && rename(tmp, state_file) != 0) ok = 0;
	if (ok) {
		LOG("wrote %s (%llu KiB, %lld ms%s)", state_file,
		    (unsigned long long)((sizeof snap.h + 20 + stored_len) >> 10), (long long)(now_ms() - t0),
		    flags & SF_ZLIB ? "" : ", uncompressed: libz not found");
	} else {
		LOG("writing %s failed", state_file);
		unlink(tmp);
	}
	sf_buf_free(&raw);
	sf_buf_free(&packed);
	return ok ? 0 : -1;
}

/* Files are written by a background thread, so a save only costs the game
 * the time to copy memory. */
static pthread_mutex_t writer_mu = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t writer_cv = PTHREAD_COND_INITIALIZER;
static int writer_pending, writer_busy, writer_started;
static pthread_t writer_thread;

static void *writer_main(void *arg)
{
	(void)arg;
	pthread_mutex_lock(&writer_mu);
	for (;;) {
		while (!writer_pending)
			pthread_cond_wait(&writer_cv, &writer_mu);
		writer_pending = 0;
		writer_busy = 1;
		pthread_mutex_unlock(&writer_mu);
		write_state_file();
		pthread_mutex_lock(&writer_mu);
		writer_busy = 0;
		pthread_cond_broadcast(&writer_cv);
	}
	return NULL;
}

/* Wait until no file write is using snap. */
static void writer_wait(void)
{
	pthread_mutex_lock(&writer_mu);
	while (writer_pending || writer_busy)
		pthread_cond_wait(&writer_cv, &writer_mu);
	pthread_mutex_unlock(&writer_mu);
}

static void writer_kick(void)
{
	pthread_mutex_lock(&writer_mu);
	if (!writer_started) {
		pthread_attr_t a;
		pthread_attr_init(&a);
		pthread_attr_setdetachstate(&a, PTHREAD_CREATE_DETACHED);
		writer_started = pthread_create(&writer_thread, &a, writer_main, NULL) == 0;
		pthread_attr_destroy(&a);
	}
	if (writer_started) {
		writer_pending = 1;
		pthread_cond_signal(&writer_cv);
		pthread_mutex_unlock(&writer_mu);
	} else {
		pthread_mutex_unlock(&writer_mu);
		write_state_file();
	}
}

/* A state file being written when PICO-8 quits is finished first. */
__attribute__((destructor)) static void p8ss_fini(void)
{
	writer_wait();
}

/* 0 on success, -1 otherwise. */
static int read_state_file(void)
{
	if (!state_file) return -1;
	FILE *f = fopen(state_file, "rb");
	if (!f) { LOG("no state file at %s", state_file); return -1; }
	struct state_hdr h;
	uint32_t flags = 0;
	uint64_t raw_len = 0, stored_len = 0;
	const char *why = "unreadable";
	int ok = fread(&h, sizeof h, 1, f) == 1 && fread(&flags, sizeof flags, 1, f) == 1 &&
		 fread(&raw_len, sizeof raw_len, 1, f) == 1 && fread(&stored_len, sizeof stored_len, 1, f) == 1;
	if (ok && (memcmp(h.magic, STATE_MAGIC, 8) || h.word_size != sizeof(uintptr_t))) {
		why = "not a state file for this version of p8ss";
		ok = 0;
	}
	if (ok && h.exe_hash != state_exe_hash) {
		why = "from a different build of PICO-8 or p8ss";
		ok = 0;
	}
	if (ok && h.segs_len != segs_size()) { why = "data size mismatch"; ok = 0; }
	if (ok && (raw_len > ((uint64_t)1 << 32) || stored_len > raw_len + (raw_len >> 4) + 4096)) {
		why = "corrupt sizes";
		ok = 0;
	}
	unsigned char *stored = ok ? __libc_malloc(stored_len ? stored_len : 1) : NULL;
	unsigned char *raw = NULL;
	if (ok) ok = stored && fread(stored, 1, stored_len, f) == stored_len;
	fclose(f);
	if (ok && (flags & SF_ZLIB)) {
		raw = __libc_malloc(raw_len ? raw_len : 1);
		ok = raw && sf_uncompress(stored, stored_len, raw, raw_len) == 0;
		if (!ok) why = sf_have_zlib() ? "corrupt compressed data" : "compressed, and libz is missing";
		__libc_free(stored);
	} else if (ok) {
		raw = stored;
		ok = stored_len == raw_len;
	} else {
		__libc_free(stored);
	}

	size_t total = ok ? h.segs_len + h.stack_len + h.arena_len : 0;
	if (ok && total > snap.buf_cap) {
		__libc_free(snap.buf);
		snap.buf = __libc_malloc(total);
		snap.buf_cap = snap.buf ? total : 0;
		if (!snap.buf) { why = "out of memory"; ok = 0; }
	}
	const unsigned char *p = raw, *end = raw + raw_len;
	struct sf_hash id = { 0, 0 };
	if (ok && raw_len < sizeof id + sizeof snap.map) { why = "truncated"; ok = 0; }
	if (ok) {
		memcpy(&id, p, sizeof id);
		p += sizeof id;
		memcpy(&snap.map, p, sizeof snap.map);
		p += sizeof snap.map;
	}
	/* Pages stored by hash come from the page dictionary. */
	struct sf_dict d = { 0 };
	if (ok && (id.a || id.b)) {
		char path[4096];
		ok = dict_dir != NULL;
		if (ok) dict_name(path, sizeof path, id);
		ok = ok && sf_dict_read(path, &d, 1, &why) == 0;
		if (!dict_dir) why = "the page dictionary is missing";
		if (ok && (d.id.a != id.a || d.id.b != id.b)) {
			why = "made with a different page dictionary";
			ok = 0;
		}
	}
	if (ok)
		ok = sf_decode(&p, end, snap.buf, h.segs_len, &d.idx, &why) == 0 &&
		     sf_decode(&p, end, snap.buf + h.segs_len, h.stack_len, &d.idx, &why) == 0 &&
		     sf_decode(&p, end, snap.buf + h.segs_len + h.stack_len, h.arena_len, &d.idx, &why) == 0;
	sf_dict_free(&d);
	__libc_free(raw);
	if (!ok) { LOG("cannot load %s: %s", state_file, why); snap.valid = 0; return -1; }
	snap.h = h;
	snap.valid = 1;
	return 0;
}

/* Returns 1 when execution arrives here again because a state saved by
 * this call was loaded, 0 after an ordinary save. */
static __attribute__((noinline)) int do_save(void)
{
	volatile uintptr_t here = (uintptr_t)__builtin_frame_address(0);
	uintptr_t lo = (here - 4096) & ~(uintptr_t)15;  /* include our frame and red zone */

	writer_wait();
	int64_t t0 = now_ms();
	audio_lock();
	pthread_mutex_lock(&arena_lock);

	size_t seglen = segs_size();
	size_t stacklen = stack_top - lo;
	size_t arenalen = ah->top - (uintptr_t)arena;
	size_t total = seglen + stacklen + arenalen;
	if (total > snap.buf_cap) {
		__libc_free(snap.buf);
		snap.buf = __libc_malloc(total);
		snap.buf_cap = snap.buf ? total : 0;
	}
	if (!snap.buf) {
		snap.valid = 0;
		pthread_mutex_unlock(&arena_lock);
		audio_unlock();
		LOG("save failed: out of memory");
		return 0;
	}

	just_restored = 0;
	getcontext(&snap.h.ctx);
	if (just_restored) {
		/* We arrive here a second time when a state is loaded. */
		just_restored = 0;
		pthread_mutex_unlock(&arena_lock);
		audio_unlock();
		LOG("state loaded");
		return 1;
	}

	unsigned char *o = snap.buf;
	for (int i = 0; i < nsegs; i++) {
		size_t n = segs[i].hi - segs[i].lo;
		memcpy(o, (void *)segs[i].lo, n);
		o += n;
	}
	memcpy(o, (void *)lo, stacklen); o += stacklen;
	memcpy(o, arena, arenalen);

	struct state_hdr *h = &snap.h;
	memcpy(h->magic, STATE_MAGIC, 8);
	h->word_size = sizeof(uintptr_t);
	h->pid = (uint32_t)getpid();
	h->start_time = process_start_time();
	h->exe_hash = state_exe_hash;
	h->canary = get_canary();
	h->ptr_guard = get_ptr_guard();
	h->stack_lo = lo;
	h->stack_end = stack_top;
	h->arena_addr = (uintptr_t)arena;
	h->segs_len = seglen;
	h->stack_len = stacklen;
	h->arena_len = arenalen;
	read_clocks(&h->clocks);
	read_pmap(&snap.map);
	snap.valid = 1;

	pthread_mutex_unlock(&arena_lock);
	audio_unlock();
	LOG("state saved (%zu KiB in memory, %lld ms)", total >> 10, (long long)(now_ms() - t0));
	if (state_file) writer_kick();
	return 0;
}

/* ---- saving on exit ----
 * With P8SS_SAVE_ON_EXIT=1, SIGTERM, SIGINT and SIGHUP (how firmwares
 * usually close an emulator) save a state before PICO-8 quits, so the
 * next launch can resume with P8SS_AUTOLOAD. The handlers go in once SDL
 * has installed its own, which are then called as before. */
static int save_on_exit;
static volatile sig_atomic_t exit_sig;
static const int exit_sigs[] = { SIGTERM, SIGINT, SIGHUP };
static struct sigaction old_exit_sa[3];
static int exit_handlers_in;

static void on_exit_signal(int sig) { exit_sig = sig; }

static void install_exit_handlers(void)
{
	struct sigaction sa = { 0 };
	sa.sa_handler = on_exit_signal;
	sa.sa_flags = SA_RESTART;
	for (int i = 0; i < 3; i++)
		sigaction(exit_sigs[i], &sa, &old_exit_sa[i]);
	exit_handlers_in = 1;
}

static void save_and_exit(int sig)
{
	/* A state saved here resumes here when it's loaded: carry on playing. */
	if (do_save())
		return;
	writer_wait();                    /* the file is complete before we go */
	for (int i = 0; i < 3; i++)
		sigaction(exit_sigs[i], &old_exit_sa[i], NULL);
	LOG("exiting after save");
	raise(sig);
}

/* ---- translating a state into this process ---- */
struct xlate_stats { size_t moved, kept, mapped, stale, unmangled, canaries; int hit; };

/* Foreign pointers seen in the saved data segments, paired with the value
 * this process has at the same place: the same global (window, renderer,
 * texture...) in both processes. Used to translate copies elsewhere. */
#define MAX_PAIRS 4096
static struct { uintptr_t from, to; } pairs[MAX_PAIRS];
static int npairs;

static const char *dbg_where = "?";
static size_t dbg_off;

static int pair_lookup(uintptr_t v, uintptr_t *out)
{
	for (int i = 0; i < npairs; i++)
		if (pairs[i].from == v) { *out = pairs[i].to; return 1; }
	return 0;
}

/* Translate one word. cur is this process's value at the same place, or
 * has_cur is 0 when there is no meaningful counterpart. Sets st->hit when
 * the word was recognised as a pointer, canary or mangled pointer. */
static uintptr_t xlate(uintptr_t v, uintptr_t cur, int has_cur, struct xlate_stats *st)
{
	st->hit = 0;
	if (v < 0x10000) return v;
	const struct region *r = lookup(&snap.map, v);
	if (r) {
		st->hit = 1;
		switch (r->kind) {
		case R_ARENA:
			return v;
		case R_OBJ: {
			int o = find_obj(&cur_map, snap.map.o[r->obj].name);
			if (o < 0) { st->stale++; return v; }
			st->moved++;
			return v - snap.map.o[r->obj].base + cur_map.o[o].base;
		}
		case R_STACK:
			/* Only the live part of the stack holds real targets; this
			 * keeps numbers that merely look like stack addresses
			 * (common under qemu, whose stack sits low) untouched.
			 * Above the stack end are argv and the environment, which
			 * differ between runs, so those are treated as foreign. */
			if (v >= snap.h.stack_lo && v < snap.h.stack_end) {
				st->moved++;
				return v - snap.h.stack_end + stack_top;
			}
			if (v < snap.h.stack_lo) { st->hit = 0; return v; }
			/* fall through */
		default: {
			uintptr_t t;
			if (has_cur) {
				const struct region *c = lookup(&cur_map, cur);
				if (c && (c->kind == R_FOREIGN || (c->kind == R_STACK && cur >= stack_top))) {
					st->kept++;
					return cur;
				}
			}
			if (pair_lookup(v, &t)) { st->mapped++; return t; }
			st->stale++;
			if (debug_enabled) LOG("unresolved pointer %#lx at %s+%#zx", (unsigned long)v, dbg_where, dbg_off);
			return v;
		}
		}
	}
	/* Not a plain pointer: maybe a glibc-mangled code or stack pointer
	 * (setjmp buffers, e.g. Lua's error handlers), or the stack canary. */
	if (snap.h.canary && v == snap.h.canary) { st->hit = 1; st->canaries++; return get_canary(); }
	uintptr_t d = demangle(v, snap.h.ptr_guard);
	const struct region *dr = lookup(&snap.map, d);
	if (dr && (dr->kind == R_STACK || dr->kind == R_OBJ)) {
		struct xlate_stats ignore = { 0 };
		st->hit = 1;
		st->unmangled++;
		return mangle(xlate(d, 0, 0, &ignore), get_ptr_guard());
	}
	return v;
}

/* Builds the translated image in place in snap.buf's copy and fixes ctx. */
static unsigned char *xbuf;
static size_t xbuf_cap;
static ucontext_t xctx;
static uintptr_t new_stack_lo;

static int prepare_restore(void)
{
	size_t total = snap.h.segs_len + snap.h.stack_len + snap.h.arena_len;
	if (total > xbuf_cap) {
		__libc_free(xbuf);
		xbuf = __libc_malloc(total);
		xbuf_cap = xbuf ? total : 0;
	}
	if (!xbuf) return -1;
	memcpy(xbuf, snap.buf, total);
	xctx = snap.h.ctx;

	int same = snap.h.pid == (uint32_t)getpid() && snap.h.start_time == process_start_time();
	new_stack_lo = snap.h.stack_lo - snap.h.stack_end + stack_top;
	if (same) return 0;

	if (snap.h.arena_addr != (uintptr_t)arena) {
		LOG("arena is at a different address; cannot load");
		return -1;
	}
	read_pmap(&cur_map);
	struct xlate_stats st = { 0 };
	const size_t W = sizeof(uintptr_t);
	/* PICO-8 packs some heap objects (Lua tables among them) at 4-byte
	 * alignment, so pointers in the data and arena are looked for at every
	 * 4 bytes, skipping past each one found. */
	const size_t STEP = 4;

	/* Data segments: same layout in both processes. */
	npairs = 0;
	unsigned char *p = xbuf;
	for (int i = 0; i < nsegs; i++) {
		size_t n = segs[i].hi - segs[i].lo;
		for (size_t off = 0; off + W <= n; off += st.hit ? W : STEP) {
			uintptr_t v, cur;
			memcpy(&cur, (void *)(segs[i].lo + off), W);
			memcpy(&v, p + off, W);
			dbg_where = "data"; dbg_off = off;
			uintptr_t nv = xlate(v, cur, 1, &st);
			if (nv != v && nv == cur && npairs < MAX_PAIRS) {
				pairs[npairs].from = v; pairs[npairs].to = cur; npairs++;
			}
			memcpy(p + off, &nv, W);
		}
		p += n;
	}
	/* Stack: frames line up from the stack end down to the outer frames
	 * (main, the main loop), which hold the longest-lived pointers. */
	for (size_t off = 0; off + W <= snap.h.stack_len; off += W) {
		uintptr_t v, cur = 0;
		uintptr_t at = new_stack_lo + off;
		int has_cur = at >= (uintptr_t)__builtin_frame_address(0);
		if (has_cur) cur = *(uintptr_t *)at;
		memcpy(&v, p + off, W);
		dbg_where = "stack"; dbg_off = snap.h.stack_len - off;   /* from the end */
		v = xlate(v, cur, has_cur, &st);
		memcpy(p + off, &v, W);
	}
	p += snap.h.stack_len;
	/* Arena: PICO-8's heap. */
	st.hit = 0;
	for (size_t off = 0; off + W <= snap.h.arena_len; off += st.hit ? W : STEP) {
		uintptr_t v;
		memcpy(&v, p + off, W);
		dbg_where = "arena"; dbg_off = off;
		uintptr_t nv = xlate(v, 0, 0, &st);
		if (debug_enabled > 1 && nv != v) {
			const struct region *r = lookup(&snap.map, v);
			LOG("arena+%#zx: %#lx -> %#lx (%s)", off, (unsigned long)v, (unsigned long)nv,
			    r ? (r->obj >= 0 ? snap.map.o[r->obj].name : r->kind == R_STACK ? "stack" : "?") : "mangled/canary");
		}
		memcpy(p + off, &nv, W);
	}
	/* Saved registers. */
	dbg_where = "regs"; dbg_off = 0;
#if defined(__x86_64__)
	for (int i = 0; i < NGREG; i++)
		xctx.uc_mcontext.gregs[i] = (greg_t)xlate((uintptr_t)xctx.uc_mcontext.gregs[i], 0, 0, &st);
	xctx.uc_mcontext.fpregs = &xctx.__fpregs_mem;
#elif defined(__aarch64__)
	for (int i = 0; i < 31; i++)
		xctx.uc_mcontext.regs[i] = xlate(xctx.uc_mcontext.regs[i], 0, 0, &st);
	xctx.uc_mcontext.sp = xlate(xctx.uc_mcontext.sp, 0, 0, &st);
	xctx.uc_mcontext.pc = xlate(xctx.uc_mcontext.pc, 0, 0, &st);
#endif
	LOG("translated state from another run: %zu relocated, %zu kept, %zu mapped, %zu unmangled, %zu canaries, %zu unresolved",
	    st.moved, st.kept, st.mapped, st.unmangled, st.canaries, st.stale);
	return 0;
}

/* Runs on a private stack, so overwriting the main stack is safe. */
static unsigned char restore_stack[64 * 1024] __attribute__((aligned(16)));
static ucontext_t restore_ctx;

static void restore_on_private_stack(void)
{
	const unsigned char *in = xbuf;
	for (int i = 0; i < nsegs; i++) {
		size_t n = segs[i].hi - segs[i].lo;
		memcpy((void *)segs[i].lo, in, n);
		in += n;
	}
	memcpy((void *)new_stack_lo, in, snap.h.stack_len); in += snap.h.stack_len;
	memcpy(arena, in, snap.h.arena_len);
	set_clocks(&snap.h.clocks);
	just_restored = 1;
	setcontext(&xctx);
}

static int load_file;

static void do_load(void)
{
	writer_wait();
	int same = snap.valid && snap.h.pid == (uint32_t)getpid() &&
		   snap.h.start_time == process_start_time();
	/* With P8SS_LOAD_FILE=1 a load always reads P8SS_FILE, which someone
	 * else may have replaced (the libretro core does, to load a state). */
	if (!same || load_file) {
		if (read_state_file() != 0) { LOG("no state to load"); return; }
	}
	/* Locks are released by the do_save frame we return into. */
	audio_lock();
	pthread_mutex_lock(&arena_lock);
	if (prepare_restore() != 0) {
		pthread_mutex_unlock(&arena_lock);
		audio_unlock();
		LOG("load failed");
		return;
	}
	getcontext(&restore_ctx);
	restore_ctx.uc_stack.ss_sp = restore_stack;
	restore_ctx.uc_stack.ss_size = sizeof restore_stack;
	restore_ctx.uc_link = NULL;
	makecontext(&restore_ctx, restore_on_private_stack, 0);
	setcontext(&restore_ctx);
}

/* ---- SDL hook ---- */
struct p8ss_keyevent {             /* matches SDL_KeyboardEvent (SDL 2.x) */
	uint32_t type, timestamp, window_id;
	uint8_t state, repeat, pad2, pad3;
	int32_t scancode, sym;
	uint16_t mod;
};
#define P8SS_KEYDOWN 0x300
#define P8SS_KEYUP   0x301

static int (*real_poll_event)(void *);
static void (*real_render_present)(void *);
static int (*real_update_window_surface)(void *);

/* Counting presented frames tells us PICO-8 has finished starting up
 * (window, renderer, textures and audio all exist) before an autoload.
 * PICO-8 presents through a renderer or straight to the window surface. */
void SDL_RenderPresent(void *renderer)
{
	if (!real_render_present)
		real_render_present = dlsym(RTLD_NEXT, "SDL_RenderPresent");
	frames++;
	real_render_present(renderer);
}

int SDL_UpdateWindowSurface(void *window)
{
	if (!real_update_window_surface)
		real_update_window_surface = dlsym(RTLD_NEXT, "SDL_UpdateWindowSurface");
	frames++;
	return real_update_window_surface(window);
}

int SDL_PollEvent(void *ev)
{
	if (!real_poll_event) {
		real_poll_event = dlsym(RTLD_NEXT, "SDL_PollEvent");
		real_lock_audio = dlsym(RTLD_NEXT, "SDL_LockAudio");
		real_unlock_audio = dlsym(RTLD_NEXT, "SDL_UnlockAudio");
	}
	if (ready) {
		if (autoload_frames >= 0 && frames >= autoload_frames) {
			autoload_frames = -1;
			want_load = 1;
		}
		if (save_on_exit && !exit_handlers_in && frames >= 1)
			install_exit_handlers();
		if (exit_sig) { int sig = exit_sig; exit_sig = 0; save_and_exit(sig); }
		if (want_save) { want_save = 0; do_save(); }
		if (want_load) { want_load = 0; do_load(); }
	}

	for (;;) {
		int r = real_poll_event(ev);
		if (!r || !ev)
			return r;
		struct p8ss_keyevent *k = ev;
		if ((k->type == P8SS_KEYDOWN || k->type == P8SS_KEYUP) &&
		    (k->sym == save_key || k->sym == load_key)) {
			/* Swallow our hotkeys; act once on press. */
			if (k->type == P8SS_KEYDOWN && !k->repeat) {
				if (k->sym == save_key) want_save = 1;
				else want_load = 1;
			}
			continue;
		}
		return r;
	}
}

static void on_signal(int sig)
{
	if (sig == SIGUSR1) want_save = 1;
	else if (sig == SIGUSR2) want_load = 1;
}

extern void *__libc_stack_end;

__attribute__((constructor)) static void p8ss_init(void)
{
	const char *v;
	if ((v = getenv("P8SS_QUIET")) && *v == '1') log_enabled = 0;
	if ((v = getenv("P8SS_DEBUG"))) debug_enabled = (int)env_num(v);
	if ((v = getenv("P8SS_SAVE_KEY"))) save_key = (int)env_num(v);
	if ((v = getenv("P8SS_LOAD_KEY"))) load_key = (int)env_num(v);
	if ((v = getenv("P8SS_AUTOLOAD")) && *v) autoload_frames = env_num(v);
	state_file = getenv("P8SS_FILE");
	if ((v = getenv("P8SS_SAVE_ON_EXIT")) && *v == '1') save_on_exit = 1;
	if ((v = getenv("P8SS_LOAD_FILE")) && *v == '1') load_file = 1;
	real_get_ticks = dlsym(RTLD_NEXT, "SDL_GetTicks");
	real_clock_gettime = dlsym(RTLD_NEXT, "clock_gettime");
	real_clock = dlsym(RTLD_NEXT, "clock");

	dl_iterate_phdr(phdr_cb, NULL);
	/* Everything above this is argv, the environment and the aux vector,
	 * which belong to this run and are never part of a state. */
	stack_top = (uintptr_t)__libc_stack_end;
	arena_init();
	if (!arena || !text_hi || !stack_top || !real_get_ticks || !real_clock_gettime || !real_clock) {
		LOG("disabled (arena %p, text %#lx, stack %#lx)", (void *)arena,
		    (unsigned long)text_hi, (unsigned long)stack_top);
		return;
	}
	state_exe_hash = exe_hash();
	/* Page dictionaries go next to the state file, or in P8SS_PAGES. */
	if ((v = getenv("P8SS_PAGES")) && *v) {
		dict_dir = strdup(v);
	} else if (state_file && *state_file) {
		const char *slash = strrchr(state_file, '/');
		dict_dir = slash ? strndup(state_file, slash > state_file ? (size_t)(slash - state_file) : 1)
				 : strdup(".");
	}

	struct sigaction sa = { 0 };
	sa.sa_handler = on_signal;
	sa.sa_flags = SA_RESTART;
	sigaction(SIGUSR1, &sa, NULL);
	sigaction(SIGUSR2, &sa, NULL);

	ready = 1;
	LOG("ready: text %#lx-%#lx, %d data segments (%zu KiB), arena at %p",
	    (unsigned long)text_lo, (unsigned long)text_hi, nsegs, segs_size() >> 10, (void *)arena);
}
