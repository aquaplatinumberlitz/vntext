#ifndef AIVOICE_ENGINE_API_H
#define AIVOICE_ENGINE_API_H
#include <windows.h>
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>

typedef int (__cdecl *initialize_fn)(void *);
typedef int (__cdecl *authenticate_fn)(const char *, const char *, const char *);
typedef void *(__cdecl *new_fn)(void);
typedef void (__cdecl *delete_fn)(void *);
typedef int (__cdecl *terminate_fn)(void);
typedef int (__cdecl *load_lang_fn)(void *, const char *, const char *, unsigned int);
typedef int (__cdecl *load_voice_fn)(void *, const char *, const char *, unsigned int, const char *);
typedef int (__cdecl *select_fn)(void *, const char *);
typedef int (__cdecl *talk_fn)(void *, const char *, unsigned int);
typedef int (__cdecl *get_fs_fn)(void *);
typedef struct { uint32_t type; void *data; } tts_event;
typedef struct { void *pcm; uint64_t bytes; void *markers; uint64_t marker_count; } tts_buffer;
_Static_assert(sizeof(tts_event) == 16, "Windows x64 event layout required");
_Static_assert(sizeof(tts_buffer) == 32, "Windows x64 buffer layout required");
typedef int (__cdecl *handler_fn)(void *, tts_event *);
typedef void *(__cdecl *sink_new_fn)(handler_fn, void *);
typedef int (__cdecl *set_sink_fn)(void *, void *);
typedef struct {
    FILE *file;
    uint32_t bytes;
    int failed;
    unsigned char pcm[65536];
    // SinkAdapter::marker copies 80 bytes per marker at 1800D1725.
    unsigned char markers[256 * 80];
    unsigned events[8];
    ULONGLONG started, first_pcm;
} audio_output;

static int __cdecl on_event(void *context, tts_event *event) {
    audio_output *out = context;
    if (!event || event->type > 7) return 2;
    out->events[event->type]++;
    if (event->type == 2 && event->data && getenv("AIVOICE2_TRACE_KANA"))
        fprintf(stderr, "AIKANA: %.12000s\n", (const char *)event->data);
    if (event->type == 3) {
        tts_buffer *buffer = event->data;
        if (!buffer) return 2;
        buffer->pcm = out->pcm;
        buffer->bytes = sizeof(out->pcm);
        buffer->markers = out->markers;
        buffer->marker_count = 256;
    } else if (event->type == 4) {
        const tts_buffer *buffer = event->data;
        if (!buffer || buffer->pcm != out->pcm || buffer->bytes > sizeof(out->pcm)
            || (buffer->bytes & 1) || buffer->bytes > UINT32_MAX - 36u - out->bytes)
            return 2;
        if (buffer->bytes && !out->first_pcm) out->first_pcm = GetTickCount64();
        if (buffer->bytes && getenv("AIVOICE2_WORKER_METRICS")) {
            int32_t chunk = -2000;
            uint32_t size = (uint32_t)buffer->bytes;
            fwrite(&chunk, 4, 1, stdout);
            fwrite(&size, 4, 1, stdout);
            fwrite(buffer->pcm, 1, size, stdout);
            fflush(stdout);
        }
        if (fwrite(buffer->pcm, 1, (size_t)buffer->bytes, out->file) != buffer->bytes) {
            out->failed = 1;
            return 2;
        }
        out->bytes += (uint32_t)buffer->bytes;
    }
    return 0;
}

static int wav_header(FILE *file, unsigned fs, uint32_t bytes) {
    // Windows x64 is little endian. Fixed PCM s16 mono header.
    const unsigned char header[44] = {
        'R','I','F','F',0,0,0,0,'W','A','V','E','f','m','t',' ',
        16,0,0,0,1,0,1,0,0,0,0,0,0,0,0,0,2,0,16,0,
        'd','a','t','a',0,0,0,0
    };
    unsigned char copy[44];
    memcpy(copy, header, sizeof(copy));
    uint32_t values[] = {bytes + 36, fs, fs * 2, bytes};
    const unsigned offsets[] = {4, 24, 28, 40};
    for (unsigned i = 0; i < 4; i++) memcpy(copy + offsets[i], &values[i], 4);
    return fseek(file, 0, SEEK_SET) == 0 && fwrite(copy, 1, sizeof(copy), file) == sizeof(copy);
}


#endif
