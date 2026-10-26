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
#include <stdint.h>
#include <string.h>

typedef struct { int kind; unsigned int frame; int alive; float pitch; float filtered_noise; } voice;
static ma_device device;
static ma_context context;
static _Atomic unsigned int pending;
static _Atomic unsigned char piano_keys[128];
typedef struct { int note; int frame; float velocity; float release; int alive; } piano_voice;
typedef struct {
    voice voices[12];
    piano_voice piano[16];
    unsigned int noise_seed;
    float early_reflections[4096];
    unsigned int reflection_cursor;
} audio_state;
static audio_state playback;
static int active;
#define CUE_COUNT 20
int kestrel_audio_cue_count(void) { return CUE_COUNT; }

static float noise(audio_state *state) {
    state->noise_seed ^= state->noise_seed << 13; state->noise_seed ^= state->noise_seed >> 17; state->noise_seed ^= state->noise_seed << 5;
    return (float)state->noise_seed / 2147483648.0f - 1.0f;
}
static void play_voice(audio_state *state, int kind) {
    int slot = 0;
    for (int i=0;i<12;++i) if (!state->voices[i].alive) {slot=i;break;}
    state->noise_seed = state->noise_seed * 1664525u + 1013904223u;
    float pitch = 0.93f + (float)(state->noise_seed & 1023u) * (0.14f / 1023.0f);
    state->voices[slot]=(voice){kind,0,1,pitch,0.0f};
}
static float tone(float frequency, float t, float decay) {
    return sinf(6.2831853f * frequency * t) * expf(-decay * t);
}
static void piano_on(audio_state *state, int note, unsigned char velocity) {
    int slot = 0;
    for (int i=0;i<16;++i) if (!state->piano[i].alive) {slot=i;break;}
    state->piano[slot]=(piano_voice){note,0,(float)velocity/127.0f,1.0f,1};
}
static void synth_frames(audio_state *state, float *out, ma_uint32 frames) {
    for (ma_uint32 frame = 0; frame < frames; ++frame) {
        float value = 0;
        for (int i = 0; i < 12; ++i) {
            voice *v = &state->voices[i];
            if (!v->alive) continue;
            float t = v->frame++ * v->pitch / 48000.0f;
            float sample = 0;
            if (v->kind == 0) {
                // Air shear rises into a low, heavy resonant swish.
                float envelope = sinf(3.1415926f * fminf(t / 0.19f, 1.0f));
                sample = (noise(state) * 0.10f + tone(95.0f - t * 160.0f, t, 13.0f) * 0.18f) * envelope;
                if (t > 0.20f) v->alive = 0;
            } else if (v->kind == 1) {
                // Inharmonic modes + a short crack + low body resonance.
                sample = 0.18f * tone(117.0f, t, 25.0f)
                       + 0.12f * tone(913.0f, t, 9.0f)
                       + 0.08f * tone(1471.0f, t, 12.0f)
                       + 0.05f * tone(2873.0f, t, 20.0f)
                       + 0.18f * noise(state) * expf(-100.0f * t);
                sample *= fminf(t * 3000.0f, 1.0f);
                if (t > 0.70f) v->alive = 0;
            } else if (v->kind == 2) {
                // Loose dirt: a low, brief scuff without a pitched impact.
                float grit = noise(state);
                v->filtered_noise += 0.045f * (grit - v->filtered_noise);
                sample = (0.075f * v->filtered_noise + 0.003f * grit)
                    * fminf(t * 250.0f, 1.0f) * expf(-24.0f * t);
                if (t > 0.14f) v->alive = 0;
            } else if (v->kind == 3) {
                // Dry stone fracture: coarse noise with a low body, little ring.
                sample = 0.22f * noise(state) * expf(-40.0f * t) + 0.18f * tone(140.0f, t, 35.0f);
                if (t > 0.25f) v->alive = 0;
            } else if (v->kind == 4) {
                // Crystal has a long, bright, inharmonic chime.
                sample = 0.12f * tone(1245.0f, t, 5.0f) + 0.08f * tone(2037.0f, t, 7.0f)
                       + 0.04f * tone(3511.0f, t, 12.0f) + 0.09f * noise(state) * expf(-130.0f * t);
                sample *= fminf(t * 3000.0f, 1.0f);
                if (t > 1.0f) v->alive = 0;
            } else if (v->kind == 5) {
                // Soft polymer absorbs the blade: short muffled knock, no ring.
                sample = 0.24f * tone(82.0f, t, 50.0f) + 0.035f * noise(state) * expf(-100.0f * t);
                if (t > 0.16f) v->alive = 0;
            } else if (v->kind == 6) {
                // A playful rising whistle and soft air swish for each tossed unit.
                sample = 0.11f * tone(420.0f + t * 1100.0f, t, 16.0f) + 0.025f * noise(state) * expf(-22.0f*t);
                sample *= fminf(t*1500.0f,1.0f);
                if (t > 0.24f) v->alive = 0;
            } else if (v->kind == 7) {
                // Soft ground contact: filtered grit and a brief low thud, with no pitched clink.
                v->filtered_noise += 0.055f * (noise(state) - v->filtered_noise);
                sample = 0.24f * v->filtered_noise * expf(-28.0f * t)
                    + 0.11f * tone(82.0f - 95.0f * t, t, 32.0f);
                sample *= fminf(t * 1600.0f, 1.0f);
                if (t > 0.18f) v->alive = 0;
            } else if (v->kind == 8) {
                sample = 0.09f * tone(660.0f,t,20.0f) + 0.06f * tone(990.0f,t,24.0f);
                sample *= fminf(t*1500.0f,1.0f);
                if (t > 0.22f) v->alive = 0;
            } else if (v->kind == 9) {
                // Short overlapping jet grains: smooth turbine body plus exhaust hiss.
                float envelope = sinf(3.1415926f * fminf(t / 0.32f, 1.0f));
                sample = (0.035f * sinf(6.2831853f * 92.0f * t) + 0.045f * noise(state)) * envelope;
                if (t > 0.32f) v->alive = 0;
            } else if (v->kind == 10) {
                // Chest-thump impact under a short, descending throat growl.
                float rasp = noise(state) * (0.08f + 0.05f * sinf(6.2831853f * 32.0f * t));
                sample = 0.25f * tone(112.0f - t * 95.0f, t, 9.0f)
                       + 0.14f * tone(58.0f - t * 22.0f, t, 13.0f)
                       + rasp * expf(-8.0f * t);
                sample *= fminf(t * 900.0f, 1.0f);
                if (t > 0.42f) v->alive = 0;
            } else if (v->kind == 11) {
                // Bowstring snap, wooden limb recoil and a fast arrow hiss.
                sample = 0.19f * tone(285.0f - t*450.0f,t,24.0f)
                    + 0.07f * tone(580.0f,t,38.0f)
                    + 0.055f * noise(state) * expf(-22.0f*t);
                if (t > 0.32f) v->alive = 0;
            } else if (v->kind == 12) {
                // Hard shaft thrust into flesh or armor.
                sample = 0.12f * noise(state)*expf(-45.0f*t)
                    + 0.24f*tone(95.0f,t,22.0f);
                if (t > 0.24f) v->alive = 0;
            } else if (v->kind == 13) {
                // Ballista limbs release over a heavy frame knock.
                sample = 0.3f*tone(76.0f,t,11.0f)
                    + 0.16f*tone(171.0f-t*190.0f,t,17.0f)
                    + 0.10f*noise(state)*expf(-34.0f*t);
                if (t > 0.60f) v->alive = 0;
            } else if (v->kind == 14) {
                // Snapping electrical arcs with a descending capacitor whine.
                float flutter = sinf(6.2831853f*47.0f*t)>0.15f ? 1.0f : 0.2f;
                sample = flutter*(0.12f*noise(state)+0.12f*tone(940.0f-t*1400.0f,t,8.0f))
                    + 0.10f*tone(145.0f,t,14.0f);
                if (t > 0.48f) v->alive = 0;
            } else if (v->kind == 15) {
                // Light shaft impact: a dry, brief crack and soft body thud.
                sample = 0.12f*noise(state)*expf(-95.0f*t)
                    + 0.15f*tone(178.0f,t,42.0f)
                    + 0.05f*tone(840.0f,t,75.0f);
                if (t > 0.18f) v->alive = 0;
            } else if (v->kind == 16) {
                // A heavy blade contact with a short metallic tail.
                sample = 0.19f*noise(state)*expf(-68.0f*t)
                    + 0.28f*tone(76.0f-t*60.0f,t,18.0f)
                    + 0.09f*tone(1250.0f,t,31.0f);
                if (t > 0.36f) v->alive = 0;
            } else if (v->kind == 17) {
                // Bolt penetration: wood snap, low body impact, and a brief rattle.
                sample = 0.20f*noise(state)*expf(-48.0f*t)
                    + 0.25f*tone(91.0f,t,16.0f)
                    + 0.08f*tone(460.0f,t,23.0f);
                if (t > 0.42f) v->alive = 0;
            } else if (v->kind == 18) {
                // A short descending pulse, separate from the arc coil crackle.
                sample = 0.075f * sinf(6.2831853f * (900.0f * t - 2400.0f * t * t)) * expf(-26.0f * t)
                    + 0.012f * noise(state) * expf(-85.0f * t);
                sample *= fminf(t * 1800.0f, 1.0f);
                if (t > 0.16f) v->alive = 0;
            } else if (v->kind == 19) {
                // Bolt contact: a bright snap followed by a short metallic ring.
                sample = 0.36f * noise(state) * expf(-60.0f * t)
                    + 0.22f * tone(1120.0f, t, 24.0f)
                    + 0.12f * tone(420.0f, t, 19.0f);
                if (t > 0.24f) v->alive = 0;
            } else {
                sample = 0.055f * tone(76.0f, t, 55.0f) + 0.035f * noise(state) * expf(-90.0f * t);
                if (t > 0.12f) v->alive = 0;
            }
            value += sample;
        }
        for (int i=0;i<16;++i) {
            piano_voice *v=&state->piano[i];
            if (!v->alive) continue;
            float t=(float)v->frame++/48000.0f;
            float hz=440.0f*powf(2.0f,((float)v->note-69.0f)/12.0f);
            float attack=fminf(1.0f,t*250.0f);
            float decay=0.55f*expf(-1.8f*t)+0.45f*expf(-7.0f*t);
            value+=v->velocity*v->release*attack*decay*
                (0.16f*sinf(6.2831853f*hz*t)+0.058f*sinf(6.2831853f*2.0f*hz*t)
                +0.019f*sinf(6.2831853f*3.0f*hz*t));
            if (v->release<1.0f) v->release*=0.99975f;
            if (v->release<0.001f || t>5.0f) v->alive=0;
        }
        // Short asymmetric room reflections give impacts space without washing out attacks.
        unsigned int cursor = state->reflection_cursor++ & 4095u;
        float left = state->early_reflections[(cursor - 613u) & 4095u];
        float right = state->early_reflections[(cursor - 947u) & 4095u];
        state->early_reflections[cursor] = value + 0.16f * (left + right);
        out[frame * 2] = tanhf((value + left * 0.18f) * 1.2f) * 0.68f;
        out[frame * 2 + 1] = tanhf((value + right * 0.18f) * 1.2f) * 0.68f;
    }
}
static void render(ma_device *d,void *output,const void *input,ma_uint32 frames) {
    (void)d;(void)input;
    unsigned int events=atomic_exchange_explicit(&pending,0,memory_order_relaxed);
    for (int kind=0;kind<CUE_COUNT;++kind) if (events & (1u<<kind)) play_voice(&playback,kind);
    for (int note=0;note<128;++note) {
        unsigned char change=atomic_exchange_explicit(&piano_keys[note],0,memory_order_relaxed);
        if (change==0) continue;
        if (change==1) {
            for (int i=0;i<16;++i) if (playback.piano[i].alive && playback.piano[i].note==note)
                playback.piano[i].release=0.999f;
        } else piano_on(&playback,note,change-1);
    }
    synth_frames(&playback,output,frames);
}
typedef struct { uint64_t sample; int kind; } audio_event;
// The offline path uses the same bank and mixer with its own deterministic state.
void kestrel_audio_synthesize(const audio_event *events,unsigned int count,float *output,unsigned int frames) {
    audio_state state={0};state.noise_seed=19349663;
    unsigned int cursor=0,event=0;
    while (cursor<frames) {
        while (event<count && events[event].sample<=cursor) {play_voice(&state,events[event].kind);++event;}
        unsigned int end=frames;
        if (event<count && events[event].sample<frames) end=(unsigned int)events[event].sample;
        synth_frames(&state,output+cursor*2,end-cursor);cursor=end;
    }
}
int kestrel_audio_start(void) {
    if (active) return 0;
    memset(&playback,0,sizeof(playback));playback.noise_seed=19349663;
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
    if (cue >= 0 && cue < CUE_COUNT) atomic_fetch_or_explicit(&pending, 1u << cue, memory_order_relaxed);
}
void kestrel_audio_note(int note,int velocity) {
    if (note>=0 && note<128 && velocity>=0 && velocity<=127)
        atomic_store_explicit(&piano_keys[note],(unsigned char)(velocity+1),memory_order_relaxed);
}
void kestrel_audio_stop(void) {
    if (!active) return;
    ma_device_uninit(&device);
    ma_context_uninit(&context);
    active = 0;
}
