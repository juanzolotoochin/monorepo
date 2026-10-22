// Asset-free, layered foley. The callback allocates nothing and never locks.
#define MA_NO_ALSA
#define MA_NO_JACK
#define MA_NO_NULL
#define MA_NO_DECODING
#define MA_NO_ENCODING
#define MA_NO_RESOURCE_MANAGER
#define MA_NO_NODE_GRAPH
#define MA_NO_ENGINE
#define MINIAUDIO_IMPLEMENTATION
#include "miniaudio.h"
#include <stdatomic.h>
#include <math.h>

typedef struct { int kind; unsigned int frame; int alive; } voice;
static ma_device device;
static ma_context context;
static _Atomic unsigned int pending;
static voice voices[12];
static unsigned int noise_seed = 19349663;
static int active;

static float noise(void) {
    noise_seed ^= noise_seed << 13; noise_seed ^= noise_seed >> 17; noise_seed ^= noise_seed << 5;
    return (float)noise_seed / 2147483648.0f - 1.0f;
}
static float tone(float frequency, float t, float decay) {
    return sinf(6.2831853f * frequency * t) * expf(-decay * t);
}
static void render(ma_device *d, void *output, const void *input, ma_uint32 frames) {
    (void)d; (void)input;
    float *out = output;
    unsigned int events = atomic_exchange_explicit(&pending, 0, memory_order_relaxed);
    for (int kind = 0; kind < 6; ++kind) {
        if (events & (1u << kind)) {
            int slot = 0;
            for (int i = 0; i < 12; ++i) if (!voices[i].alive) { slot = i; break; }
            voices[slot] = (voice){kind, 0, 1};
        }
    }
    for (ma_uint32 frame = 0; frame < frames; ++frame) {
        float value = 0;
        for (int i = 0; i < 12; ++i) {
            voice *v = &voices[i];
            if (!v->alive) continue;
            float t = v->frame++ / 48000.0f;
            float sample = 0;
            if (v->kind == 0) {
                // Air shear rises into a low, heavy resonant swish.
                float envelope = sinf(3.1415926f * fminf(t / 0.19f, 1.0f));
                sample = (noise() * 0.10f + tone(95.0f - t * 160.0f, t, 13.0f) * 0.18f) * envelope;
                if (t > 0.20f) v->alive = 0;
            } else if (v->kind == 1) {
                // Inharmonic modes + a short crack + low body resonance.
                sample = 0.18f * tone(117.0f, t, 25.0f)
                       + 0.12f * tone(913.0f, t, 9.0f)
                       + 0.08f * tone(1471.0f, t, 12.0f)
                       + 0.05f * tone(2873.0f, t, 20.0f)
                       + 0.18f * noise() * expf(-100.0f * t);
                sample *= fminf(t * 3000.0f, 1.0f);
                if (t > 0.70f) v->alive = 0;
            } else if (v->kind == 3) {
                // Dry stone fracture: coarse noise with a low body, little ring.
                sample = 0.22f * noise() * expf(-40.0f * t) + 0.18f * tone(140.0f, t, 35.0f);
                if (t > 0.25f) v->alive = 0;
            } else if (v->kind == 4) {
                // Crystal has a long, bright, inharmonic chime.
                sample = 0.12f * tone(1245.0f, t, 5.0f) + 0.08f * tone(2037.0f, t, 7.0f)
                       + 0.04f * tone(3511.0f, t, 12.0f) + 0.09f * noise() * expf(-130.0f * t);
                sample *= fminf(t * 3000.0f, 1.0f);
                if (t > 1.0f) v->alive = 0;
            } else if (v->kind == 5) {
                // Soft polymer absorbs the blade: short muffled knock, no ring.
                sample = 0.24f * tone(82.0f, t, 50.0f) + 0.035f * noise() * expf(-100.0f * t);
                if (t > 0.16f) v->alive = 0;
            } else {
                sample = 0.055f * tone(76.0f, t, 55.0f) + 0.035f * noise() * expf(-90.0f * t);
                if (t > 0.12f) v->alive = 0;
            }
            value += sample;
        }
        value = tanhf(value * 1.25f) * 0.7f;
        out[frame * 2] = value;
        out[frame * 2 + 1] = value;
    }
}
int kestrel_audio_start(void) {
    if (active) return 0;
    // PulseAudio/PipeWire on Linux; CoreAudio on macOS. No host development headers.
    ma_context_config context_config = ma_context_config_init();
    if (ma_context_init(NULL, 0, &context_config, &context) != MA_SUCCESS) return -1;
    ma_device_config config = ma_device_config_init(ma_device_type_playback);
    config.playback.format = ma_format_f32;
    config.playback.channels = 2;
    config.sampleRate = 48000;
    config.periodSizeInFrames = 256;
    config.dataCallback = render;
    if (ma_device_init(&context, &config, &device) != MA_SUCCESS) { ma_context_uninit(&context); return -1; }
    if (ma_device_start(&device) != MA_SUCCESS) { ma_device_uninit(&device); ma_context_uninit(&context); return -1; }
    active = 1;
    return 0;
}
void kestrel_audio_play(int cue) {
    if (cue >= 0 && cue < 6) atomic_fetch_or_explicit(&pending, 1u << cue, memory_order_relaxed);
}
void kestrel_audio_stop(void) {
    if (!active) return;
    ma_device_uninit(&device);
    ma_context_uninit(&context);
    active = 0;
}
