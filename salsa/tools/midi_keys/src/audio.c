// Only the desktop audio backend is needed. It works with PulseAudio or
// PipeWire's PulseAudio service; no host headers or command-line tools are used.
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

typedef void (*piano_render)(void *, float *, unsigned int);
typedef struct {
    ma_context context;
    ma_device device;
    piano_render render;
    void *state;
} piano_audio;

static void data_callback(ma_device *device, void *out, const void *in, ma_uint32 frames) {
    (void)in;
    piano_audio *audio = device->pUserData;
    audio->render(audio->state, out, frames);
}

int piano_audio_start(void **handle, piano_render render, void *state) {
    piano_audio *audio = calloc(1, sizeof(*audio));
    if (!audio) return MA_OUT_OF_MEMORY;
    audio->render = render;
    audio->state = state;
    ma_backend backend = ma_backend_pulseaudio;
    ma_context_config context_config = ma_context_config_init();
    context_config.pulse.pApplicationName = "MIDI Keys Piano";
    ma_result result = ma_context_init(&backend, 1, &context_config, &audio->context);
    if (result != MA_SUCCESS) { free(audio); return result; }
    ma_device_config config = ma_device_config_init(ma_device_type_playback);
    config.playback.format = ma_format_f32;
    config.playback.channels = 2;
    config.sampleRate = 48000;
    config.periodSizeInFrames = 256;
    config.periods = 2;
    config.dataCallback = data_callback;
    config.pUserData = audio;
    result = ma_device_init(&audio->context, &config, &audio->device);
    if (result == MA_SUCCESS) {
        result = ma_device_start(&audio->device);
        if (result != MA_SUCCESS) ma_device_uninit(&audio->device);
    }
    if (result != MA_SUCCESS) {
        ma_context_uninit(&audio->context);
        free(audio);
        return result;
    }
    *handle = audio;
    return MA_SUCCESS;
}

void piano_audio_stop(void *handle) {
    piano_audio *audio = handle;
    // Joins the callback thread before Rust frees its renderer.
    ma_device_uninit(&audio->device);
    ma_context_uninit(&audio->context);
    free(audio);
}

const char *piano_audio_error(int code) { return ma_result_description(code); }
