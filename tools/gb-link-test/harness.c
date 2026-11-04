/* Minimal libretro frontend for mGBA's slot. Link mode (tools/gb-link-test).
 * usage: harness CORE.so OWN_ROM PEER_ROM PLAYER FRAMES
 * Prints, every 60 frames (EVERY=n) and at the end: frame, the local Game
 * Boy's WRAM counters C000 (bytes right) and C001 (bytes wrong), the pair's
 * state size and an FNV hash of the serialized pair. RELOAD=n saves and
 * restores the pair before frame n, as a Link session start does. MODEL sets
 * mgba_gb_model. */
#include <dlfcn.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "libretro.h"

static const char* player;
static const char* peerRom;

static void logcb(enum retro_log_level level, const char* fmt, ...) {
	(void) level; va_list ap; va_start(ap, fmt); vfprintf(stderr, fmt, ap); va_end(ap);
}
static bool env(unsigned cmd, void* data) {
	switch (cmd) {
	case RETRO_ENVIRONMENT_GET_LOG_INTERFACE:
		((struct retro_log_callback*) data)->log = logcb;
		return true;
	case RETRO_ENVIRONMENT_GET_VARIABLE: {
		struct retro_variable* v = data;
		if (!strcmp(v->key, "mgba_link")) { v->value = "on"; return true; }
		if (!strcmp(v->key, "mgba_link_player")) { v->value = player; return true; }
		if (!strcmp(v->key, "mgba_link_peer_rom")) { v->value = peerRom; return true; }
		if (!strcmp(v->key, "mgba_gb_model")) { v->value = getenv("MODEL") ? getenv("MODEL") : "Game Boy"; return true; }
		if (!strcmp(v->key, "mgba_use_bios")) { v->value = "OFF"; return true; }
		if (!strcmp(v->key, "mgba_skip_bios")) { v->value = "ON"; return true; }
		v->value = NULL;
		return false;
	}
	case RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY:
	case RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY:
		*(const char**) data = "/tmp";
		return true;
	case RETRO_ENVIRONMENT_SET_PIXEL_FORMAT:
		return true;
	case RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE:
		*(bool*) data = false;
		return true;
	default:
		return false;
	}
}
static void video(const void* d, unsigned w, unsigned h, size_t p) { (void) d; (void) w; (void) h; (void) p; }
static void audio(int16_t l, int16_t r) { (void) l; (void) r; }
static size_t audioBatch(const int16_t* d, size_t f) { (void) d; return f; }
static void poll(void) {}
static int16_t input(unsigned port, unsigned dev, unsigned idx, unsigned id) { (void) port; (void) dev; (void) idx; (void) id; return 0; }

#define SYM(name) typeof(&name) p_##name = dlsym(lib, #name); if (!p_##name) { fprintf(stderr, "missing %s\n", #name); return 2; }

int main(int argc, char** argv) {
	if (argc != 6) { fprintf(stderr, "usage\n"); return 2; }
	player = argv[4];
	peerRom = argv[3];
	int frames = atoi(argv[5]);
	void* lib = dlopen(argv[1], RTLD_NOW | RTLD_LOCAL);
	if (!lib) { fprintf(stderr, "%s\n", dlerror()); return 2; }
	SYM(retro_set_environment) SYM(retro_set_video_refresh) SYM(retro_set_audio_sample)
	SYM(retro_set_audio_sample_batch) SYM(retro_set_input_poll) SYM(retro_set_input_state)
	SYM(retro_init) SYM(retro_load_game) SYM(retro_run) SYM(retro_get_memory_data)
	SYM(retro_get_memory_size) SYM(retro_serialize_size) SYM(retro_serialize) SYM(retro_unserialize)
	p_retro_set_environment(env);
	p_retro_set_video_refresh(video);
	p_retro_set_audio_sample(audio);
	p_retro_set_audio_sample_batch(audioBatch);
	p_retro_set_input_poll(poll);
	p_retro_set_input_state(input);
	p_retro_init();
	FILE* f = fopen(argv[2], "rb");
	static uint8_t rom[0x8000];
	if (fread(rom, 1, sizeof(rom), f) != sizeof(rom)) return 2;
	fclose(f);
	struct retro_game_info info = { argv[2], rom, sizeof(rom), NULL };
	if (!p_retro_load_game(&info)) { fprintf(stderr, "load failed\n"); return 2; }
	uint8_t* wram = NULL;
	size_t cap = 1 << 22;
	uint8_t* state = malloc(cap);
	for (int i = 1; i <= frames; ++i) {
		if (getenv("RELOAD") && i == atoi(getenv("RELOAD"))) {
			size_t size = p_retro_serialize_size();
			if (!p_retro_serialize(state, size) || !p_retro_unserialize(state, size)) {
				fprintf(stderr, "reload failed\n"); return 3;
			}
			fprintf(stderr, "reloaded at %d\n", i);
		}
		p_retro_run();
		wram = p_retro_get_memory_data(RETRO_MEMORY_SYSTEM_RAM);
		if (i % (getenv("EVERY") ? atoi(getenv("EVERY")) : 60) == 0 || i == frames) {
			size_t size = p_retro_serialize_size();
			uint64_t h = 1469598103934665603ULL;
			if (size <= cap && p_retro_serialize(state, size)) {
				for (size_t k = 0; k < size; ++k) { h ^= state[k]; h *= 1099511628211ULL; }
			}
			printf("frame=%d ok=%u err=%u size=%zu hash=%016llx\n", i,
				wram ? wram[0] : 0, wram ? wram[1] : 0, size, (unsigned long long) h);
			fflush(stdout);
		}
	}
	return 0;
}
