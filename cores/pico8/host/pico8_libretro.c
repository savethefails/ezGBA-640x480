/*
 * A libretro core that plays PICO-8 carts with the official PICO-8.
 *
 * PICO-8 can't be loaded as a library, so the core runs it as a child
 * process on sdl.c, a stand-in for SDL that trades pictures, sound and
 * buttons with this core through shared memory (p8host.h), and with p8ss.so
 * preloaded for save states.
 *
 * Files:
 *   pico8/pico8_64 (or pico8_dyn), pico8/pico8.dat   from the user's PICO-8,
 *       under the frontend's system (BIOS) directory
 *   pico8/p8ss.so, pico8/lib/libSDL2-2.0.so.0         from this project, next to
 *       this core, or else with PICO-8
 * PICO-8's own home (cart data, config) is pico8/ under the save directory,
 * with p8ss's working files in its p8ss/ folder.
 *
 * Save states are p8ss's: serializing asks PICO-8 to save (SIGUSR1) and
 * returns the file it writes; unserializing writes the file back and asks
 * PICO-8 to load it (SIGUSR2). They are too slow for run-ahead or rewind,
 * so a frontend should only use them to suspend and resume.
 */
#define _GNU_SOURCE
#include "libretro.h"
#include "p8host.h"

#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <signal.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/prctl.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

extern char **environ;

static retro_environment_t env_cb;
static retro_video_refresh_t video_cb;
static retro_audio_sample_batch_t audio_batch_cb;
static retro_input_poll_t input_poll_cb;
static retro_input_state_t input_state_cb;
static retro_log_printf_t log_cb;

static struct p8host *shm;
static int shm_fd = -1;
static pid_t child = -1;
static uint32_t frame[P8HOST_W * P8HOST_H];
static char dir[PATH_MAX];          /* system/pico8: PICO-8 itself */
static char helpers[PATH_MAX];      /* p8ss.so and lib/ */
static char home[PATH_MAX];         /* save/pico8 */
static char state_path[PATH_MAX];   /* home/p8ss/live.p8ss */
static char cart[PATH_MAX];
static unsigned audio_phase;

static void logf_(enum retro_log_level lvl, const char *fmt, ...)
{
	char msg[1024];
	va_list ap;
	va_start(ap, fmt);
	vsnprintf(msg, sizeof msg, fmt, ap);
	va_end(ap);
	if (log_cb) log_cb(lvl, "[pico8] %s\n", msg);
	else fprintf(stderr, "[pico8] %s\n", msg);
}
#define LOGI(...) logf_(RETRO_LOG_INFO, __VA_ARGS__)
#define LOGE(...) logf_(RETRO_LOG_ERROR, __VA_ARGS__)

static uint64_t now_ns(void)
{
	struct timespec ts;
	clock_gettime(CLOCK_MONOTONIC, &ts);
	return (uint64_t)ts.tv_sec * 1000000000u + ts.tv_nsec;
}

/* Tell PICO-8 the frontend is still taking frames: it pauses otherwise. */
static void keep_alive(void)
{
	if (shm) __atomic_store_n(&shm->host_ns, now_ns(), __ATOMIC_RELEASE);
}

static void sleep_ms(int ms)
{
	struct timespec ts = { 0, ms * 1000000L };
	nanosleep(&ts, NULL);
}

static int child_alive(void)
{
	if (child <= 0) return 0;
	int st;
	if (waitpid(child, &st, WNOHANG) == child) {
		LOGE("PICO-8 exited (%s %d)", WIFSIGNALED(st) ? "signal" : "status",
		     WIFSIGNALED(st) ? WTERMSIG(st) : WEXITSTATUS(st));
		child = -1;
		return 0;
	}
	return 1;
}

/* ---- libretro API ---- */
RETRO_API unsigned retro_api_version(void) { return RETRO_API_VERSION; }

RETRO_API void retro_set_environment(retro_environment_t cb)
{
	env_cb = cb;
	struct retro_log_callback l;
	if (cb(RETRO_ENVIRONMENT_GET_LOG_INTERFACE, &l)) log_cb = l.log;
	bool no_game = false;
	cb(RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME, &no_game);
}
RETRO_API void retro_set_video_refresh(retro_video_refresh_t cb) { video_cb = cb; }
RETRO_API void retro_set_audio_sample(retro_audio_sample_t cb) { (void)cb; }
RETRO_API void retro_set_audio_sample_batch(retro_audio_sample_batch_t cb) { audio_batch_cb = cb; }
RETRO_API void retro_set_input_poll(retro_input_poll_t cb) { input_poll_cb = cb; }
RETRO_API void retro_set_input_state(retro_input_state_t cb) { input_state_cb = cb; }

RETRO_API void retro_init(void) {}
RETRO_API void retro_deinit(void) {}

RETRO_API void retro_get_system_info(struct retro_system_info *info)
{
	memset(info, 0, sizeof *info);
	info->library_name = "PICO-8";
	info->library_version = "0.1";
	info->valid_extensions = "p8|png";
	info->need_fullpath = true;
	info->block_extract = true;
}

RETRO_API void retro_get_system_av_info(struct retro_system_av_info *info)
{
	memset(info, 0, sizeof *info);
	info->geometry.base_width = info->geometry.max_width = P8HOST_W;
	info->geometry.base_height = info->geometry.max_height = P8HOST_H;
	info->geometry.aspect_ratio = 1.0f;
	info->timing.fps = 60.0;
	info->timing.sample_rate = P8HOST_RATE;
}

RETRO_API void retro_set_controller_port_device(unsigned port, unsigned device) { (void)port; (void)device; }

static int is_file(const char *p)
{
	struct stat st;
	return stat(p, &st) == 0 && S_ISREG(st.st_mode);
}

static void mkdirs(const char *path)
{
	char p[PATH_MAX];
	snprintf(p, sizeof p, "%s", path);
	for (char *s = p + 1; *s; s++)
		if (*s == '/') { *s = 0; mkdir(p, 0755); *s = '/'; }
	mkdir(p, 0755);
}

/* The directories, from the frontend or P8CORE_DIR / P8CORE_HOME. */
static int find_dirs(void)
{
	const char *sys = NULL, *save = NULL, *e;
	if ((e = getenv("P8CORE_DIR")) && *e) snprintf(dir, sizeof dir, "%s", e);
	else if (env_cb && env_cb(RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY, &sys) && sys)
		snprintf(dir, sizeof dir, "%s/pico8", sys);
	else return -1;
	if ((e = getenv("P8CORE_HOME")) && *e) snprintf(home, sizeof home, "%s", e);
	else if (env_cb && env_cb(RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY, &save) && save)
		snprintf(home, sizeof home, "%s/pico8", save);
	else snprintf(home, sizeof home, "%s/home", dir);
	/* Our own files: pico8/ beside this core if it's there, else PICO-8's folder. */
	Dl_info info;
	snprintf(helpers, sizeof helpers, "%s", dir);
	if (dladdr((void *)find_dirs, &info) && info.dli_fname) {
		char p[PATH_MAX];
		snprintf(p, sizeof p, "%s", info.dli_fname);
		char *slash = strrchr(p, '/');
		if (slash) {
			snprintf(slash, sizeof p - (slash - p), "/pico8/p8ss.so");
			if (is_file(p)) { *strrchr(p, '/') = 0; snprintf(helpers, sizeof helpers, "%s", p); }
		}
	}
	return 0;
}

static const char *find_pico8(char *out, size_t n)
{
	static const char *names[] = {
#if defined(__aarch64__)
		"pico8_64",
#elif defined(__x86_64__)
		"pico8_dyn",
#endif
		"pico8",
	};
	for (size_t i = 0; i < sizeof names / sizeof names[0]; i++) {
		snprintf(out, n, "%s/%s", dir, names[i]);
		if (is_file(out)) return out;
	}
	return NULL;
}

/* Start PICO-8 on the cart. Everything the child needs is built here, so
 * between fork and exec it only calls what is safe there. */
static int spawn(void)
{
	char bin[PATH_MAX], so[PATH_MAX];
	if (!find_pico8(bin, sizeof bin)) { LOGE("no PICO-8 in %s", dir); return -1; }
	snprintf(so, sizeof so, "%s/p8ss.so", helpers);
	int have_p8ss = is_file(so);
	if (!have_p8ss) LOGE("no %s: save states are off", so);

	/* environ, minus what we set, plus ours */
	size_t n = 0;
	while (environ[n]) n++;
	char **envp = calloc(n + 8, sizeof *envp);
	static char own[6][PATH_MAX * 2 + 32];
	int k = 0;
	if (!envp) return -1;
	const char *pre = getenv("LD_PRELOAD"), *lib = getenv("LD_LIBRARY_PATH");
	if (have_p8ss)
		snprintf(own[k++], sizeof own[0], "LD_PRELOAD=%s%s%s", so, pre && *pre ? ":" : "", pre && *pre ? pre : "");
	snprintf(own[k++], sizeof own[0], "LD_LIBRARY_PATH=%s/lib%s%s", helpers, lib && *lib ? ":" : "", lib && *lib ? lib : "");
	snprintf(own[k++], sizeof own[0], "P8HOST_FD=%d", shm_fd);
	snprintf(own[k++], sizeof own[0], "P8SS_FILE=%s", state_path);
	snprintf(own[k++], sizeof own[0], "P8SS_LOAD_FILE=1");
	snprintf(own[k++], sizeof own[0], "SDL_AUDIODRIVER=p8host");
	size_t m = 0;
	for (size_t i = 0; i < n; i++) {
		const char *v = environ[i];
		if (!strncmp(v, "LD_PRELOAD=", 11) || !strncmp(v, "LD_LIBRARY_PATH=", 16) ||
		    !strncmp(v, "P8HOST_FD=", 10) || !strncmp(v, "P8SS_", 5) ||
		    !strncmp(v, "SDL_AUDIODRIVER=", 16) || !strncmp(v, "SDL_VIDEODRIVER=", 16))
			continue;
		envp[m++] = (char *)v;
	}
	for (int i = 0; i < k; i++)
		envp[m++] = own[i];
	envp[m] = NULL;
	/* P8CORE_WRAPPER runs PICO-8 through another program: for tests, an
	 * emulator, given by full path, such as "/usr/bin/qemu-aarch64 -L /". */
	char *argv[16];
	int ac = 0;
	static char wrap[PATH_MAX];
	const char *w = getenv("P8CORE_WRAPPER");
	if (w && *w) {
		snprintf(wrap, sizeof wrap, "%s", w);
		for (char *t = strtok(wrap, " "); t && ac < 8; t = strtok(NULL, " ")) argv[ac++] = t;
	}
	argv[ac++] = bin;
	argv[ac++] = "-run";
	argv[ac++] = cart;
	argv[ac++] = "-home";
	argv[ac++] = home;
	argv[ac] = NULL;

	pid_t parent = getpid();
	pid_t pid = fork();
	if (pid == 0) {
		/* PICO-8 goes when we do. */
		prctl(PR_SET_PDEATHSIG, SIGTERM);
		if (getppid() != parent) _exit(1);
		sigset_t none;
		sigemptyset(&none);
		sigprocmask(SIG_SETMASK, &none, NULL);
		if (chdir(dir) != 0) _exit(127);
		execve(argv[0], argv, envp);
		_exit(127);
	}
	free(envp);
	if (pid < 0) { LOGE("fork: %s", strerror(errno)); return -1; }
	child = pid;
	LOGI("started %s on %s", bin, cart);
	return 0;
}

RETRO_API bool retro_load_game(const struct retro_game_info *game)
{
	if (!game || !game->path) return false;
	enum retro_pixel_format fmt = RETRO_PIXEL_FORMAT_XRGB8888;
	if (env_cb && !env_cb(RETRO_ENVIRONMENT_SET_PIXEL_FORMAT, &fmt)) return false;
	if (find_dirs() != 0) { LOGE("no system directory"); return false; }
	if (!realpath(game->path, cart)) snprintf(cart, sizeof cart, "%s", game->path);
	char work[PATH_MAX];
	snprintf(work, sizeof work, "%s/p8ss", home);
	mkdirs(work);
	snprintf(state_path, sizeof state_path, "%s/live.p8ss", work);
	unlink(state_path);

	shm_fd = memfd_create("p8host", 0);              /* inherited by PICO-8 */
	if (shm_fd < 0 || ftruncate(shm_fd, sizeof *shm) != 0) { LOGE("shared memory: %s", strerror(errno)); return false; }
	shm = mmap(NULL, sizeof *shm, PROT_READ | PROT_WRITE, MAP_SHARED, shm_fd, 0);
	if (shm == MAP_FAILED) { shm = NULL; return false; }
	shm->magic = P8HOST_MAGIC;
	shm->version = P8HOST_VERSION;
	keep_alive();

	static const struct retro_input_descriptor desc[] = {
		{ 0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_LEFT, "Left" },
		{ 0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_UP, "Up" },
		{ 0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_DOWN, "Down" },
		{ 0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_RIGHT, "Right" },
		{ 0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_B, "O" },
		{ 0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_A, "X" },
		{ 0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_START, "Pause" },
		{ 0 },
	};
	if (env_cb) env_cb(RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS, (void *)desc);
	return spawn() == 0;
}

RETRO_API bool retro_load_game_special(unsigned t, const struct retro_game_info *g, size_t n)
{
	(void)t; (void)g; (void)n;
	return false;
}

static void stop_child(void)
{
	if (child <= 0) return;
	kill(child, SIGTERM);
	for (int i = 0; i < 200; i++) {                  /* 2 s, then it's killed */
		if (waitpid(child, NULL, WNOHANG) == child) { child = -1; return; }
		sleep_ms(10);
	}
	kill(child, SIGKILL);
	waitpid(child, NULL, 0);
	child = -1;
}

RETRO_API void retro_unload_game(void)
{
	stop_child();
	if (shm) munmap(shm, sizeof *shm);
	shm = NULL;
	if (shm_fd >= 0) close(shm_fd);
	shm_fd = -1;
}

RETRO_API void retro_reset(void)
{
	if (!shm) return;
	stop_child();
	memset(shm->frame, 0, sizeof shm->frame);
	shm->ready = 0;
	keep_alive();
	spawn();
}

static void read_frame(void)
{
	for (int tries = 0; tries < 4; tries++) {
		uint32_t s1 = __atomic_load_n(&shm->frame_seq, __ATOMIC_ACQUIRE);
		if (s1 & 1) continue;
		__atomic_thread_fence(__ATOMIC_SEQ_CST);
		memcpy(frame, shm->frame, sizeof frame);
		__atomic_thread_fence(__ATOMIC_SEQ_CST);
		if (__atomic_load_n(&shm->frame_seq, __ATOMIC_ACQUIRE) == s1) return;
	}
}

static uint32_t buttons_now(void)
{
	static const unsigned ids[] = {
		RETRO_DEVICE_ID_JOYPAD_B, RETRO_DEVICE_ID_JOYPAD_Y, RETRO_DEVICE_ID_JOYPAD_SELECT,
		RETRO_DEVICE_ID_JOYPAD_START, RETRO_DEVICE_ID_JOYPAD_UP, RETRO_DEVICE_ID_JOYPAD_DOWN,
		RETRO_DEVICE_ID_JOYPAD_LEFT, RETRO_DEVICE_ID_JOYPAD_RIGHT, RETRO_DEVICE_ID_JOYPAD_A,
		RETRO_DEVICE_ID_JOYPAD_X, RETRO_DEVICE_ID_JOYPAD_L, RETRO_DEVICE_ID_JOYPAD_R,
	};
	uint32_t b = 0;
	if (!input_state_cb) return 0;
	for (size_t i = 0; i < sizeof ids / sizeof ids[0]; i++)
		if (input_state_cb(0, RETRO_DEVICE_JOYPAD, 0, ids[i])) b |= 1u << ids[i];
	return b;
}

/* One frame's sound: 22050 / 60 samples, from what PICO-8 has queued. */
static void play_audio(void)
{
	static int16_t out[2 * 400];
	unsigned n = (P8HOST_RATE + audio_phase) / 60;
	audio_phase = (P8HOST_RATE + audio_phase) % 60;
	uint32_t w = __atomic_load_n(&shm->audio_w, __ATOMIC_ACQUIRE), r = shm->audio_r;
	if (w - r > P8HOST_AUDIO / 2) r = w - n;          /* too far behind: catch up */
	static int16_t last;
	for (unsigned i = 0; i < n; i++) {
		int16_t s = last;
		if (r != w) s = last = shm->audio[r++ & (P8HOST_AUDIO - 1)];
		out[2 * i] = out[2 * i + 1] = s;
	}
	__atomic_store_n(&shm->audio_r, r, __ATOMIC_RELEASE);
	if (audio_batch_cb) audio_batch_cb(out, n);
}

RETRO_API void retro_run(void)
{
	if (input_poll_cb) input_poll_cb();
	if (shm) {
		__atomic_store_n(&shm->buttons, buttons_now(), __ATOMIC_RELEASE);
		keep_alive();
		child_alive();
		read_frame();
		play_audio();
	}
	if (video_cb) video_cb(frame, P8HOST_W, P8HOST_H, P8HOST_W * 4);
}

/* ---- save states ---- */

/* Wait for PICO-8 to have drawn a frame (p8ss is ready by then). */
static int wait_ready(int ms)
{
	for (int t = 0; t < ms; t += 10) {
		if (!child_alive()) return -1;
		if (__atomic_load_n(&shm->ready, __ATOMIC_ACQUIRE)) return 0;
		keep_alive();
		sleep_ms(10);
	}
	return -1;
}

static unsigned char *state_buf;
static size_t state_len;
static int state_fresh;

/* Ask PICO-8 to save, and read what it wrote. */
static int take_state(void)
{
	state_fresh = 0;
	if (!shm || wait_ready(15000) != 0) return -1;
	unlink(state_path);
	kill(child, SIGUSR1);
	FILE *f = NULL;
	for (int t = 0; t < 30000 && !f; t += 5) {   /* the first save also writes a dictionary */
		keep_alive();                                /* PICO-8 saves at its next frame */
		if (!child_alive()) return -1;
		f = fopen(state_path, "rb");
		if (!f) sleep_ms(5);
	}
	if (!f) { LOGE("PICO-8 didn't save"); return -1; }
	struct stat st;
	int ok = fstat(fileno(f), &st) == 0 && st.st_size > 0;
	unsigned char *buf = ok ? malloc(st.st_size) : NULL;
	ok = buf && fread(buf, 1, st.st_size, f) == (size_t)st.st_size;
	fclose(f);
	if (!ok) { free(buf); return -1; }
	free(state_buf);
	state_buf = buf;
	state_len = st.st_size;
	state_fresh = 1;
	return 0;
}

/* The size is known only once the state exists, so asking for it saves. */
RETRO_API size_t retro_serialize_size(void)
{
	if (!state_fresh && take_state() != 0) return 0;
	return state_len;
}

RETRO_API bool retro_serialize(void *data, size_t size)
{
	if (!state_fresh && take_state() != 0) return false;
	state_fresh = 0;
	if (size < state_len) return false;
	memcpy(data, state_buf, state_len);
	memset((char *)data + state_len, 0, size - state_len);
	return true;
}

RETRO_API bool retro_unserialize(const void *data, size_t size)
{
	state_fresh = 0;
	if (!shm || size < 8 || memcmp(data, "P8SS", 4) != 0) return false;
	if (wait_ready(15000) != 0) return false;
	char tmp[PATH_MAX + 8];
	snprintf(tmp, sizeof tmp, "%s.in", state_path);
	FILE *f = fopen(tmp, "wb");
	int ok = f && fwrite(data, 1, size, f) == size;
	if (f) ok = fclose(f) == 0 && ok;
	if (!ok || rename(tmp, state_path) != 0) { unlink(tmp); return false; }
	kill(child, SIGUSR2);
	return true;
}

RETRO_API void retro_cheat_reset(void) {}
RETRO_API void retro_cheat_set(unsigned i, bool e, const char *c) { (void)i; (void)e; (void)c; }
RETRO_API unsigned retro_get_region(void) { return RETRO_REGION_NTSC; }
RETRO_API void *retro_get_memory_data(unsigned id) { (void)id; return NULL; }
RETRO_API size_t retro_get_memory_size(unsigned id) { (void)id; return 0; }
