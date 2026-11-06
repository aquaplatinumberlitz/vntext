// Persistent single-threaded Wine worker. Protocol: seven u32 lengths followed
// by UTF-8 strings (text, key, voice package, voice license, output WAV,
// language key, language package). Voice and language must both follow the GUI.
// Replies are i32 status + u32 PCM bytes; readiness reply has bytes=0.
#include "engine_api.h"
#include <io.h>
#include <fcntl.h>

typedef int (__cdecl *unload_fn)(void *);

static void reply(int status, uint32_t bytes) {
    int32_t code = status;
    fwrite(&code, 4, 1, stdout);
    fwrite(&bytes, 4, 1, stdout);
    fflush(stdout);
}

int main(int argc, char **argv) {
    _setmode(_fileno(stdin), _O_BINARY);
    _setmode(_fileno(stdout), _O_BINARY);
    if (argc != 4 || strlen(argv[3]) >= 8193) return 2;
    const char *product = getenv("AITALK_PRODUCT_NAME");
    const char *code = getenv("AITALK_LICENSE_CODE");
    if (!product || !*product || !code || !*code) return 2;
    HMODULE engine = LoadLibraryExA(argv[1], NULL, LOAD_WITH_ALTERED_SEARCH_PATH);
    if (!engine) { fprintf(stderr, "LoadLibraryEx: %lu\n", GetLastError()); return 3; }
#define RESOLVE(type, var, name) \
    type var = (type)(void *)GetProcAddress(engine, name); \
    if (!var) { fprintf(stderr, "Missing export: %s\n", name); FreeLibrary(engine); return 3; }
    RESOLVE(initialize_fn, initialize, "ai_ttsLibraryInitialize");
    RESOLVE(authenticate_fn, authenticate, "ai_ttsLibraryAuthenticate");
    RESOLVE(new_fn, talker_new, "ai_Talker_new");
    RESOLVE(delete_fn, talker_delete, "ai_Talker_delete");
    RESOLVE(terminate_fn, terminate, "ai_ttsLibraryTerminate");
    RESOLVE(load_lang_fn, load_lang, "ai_Talker_loadLangDic");
    RESOLVE(load_voice_fn, load_voice, "ai_Talker_loadVoiceDic");
    RESOLVE(select_fn, select_lang, "ai_Talker_selectLangDic");
    RESOLVE(select_fn, select_voice, "ai_Talker_selectVoiceDic");
    RESOLVE(unload_fn, unload_voice, "ai_Talker_unloadAllVoiceDic");
    RESOLVE(unload_fn, unload_lang, "ai_Talker_unloadAllLangDic");
    RESOLVE(talk_fn, talk, "ai_Talker_talk");
    RESOLVE(get_fs_fn, get_fs, "ai_Talker_getVoiceFs");
    RESOLVE(sink_new_fn, sink_new, "ai_TtsCallbackSink_new");
    RESOLVE(delete_fn, sink_delete, "ai_TtsCallbackSink_delete");
    RESOLVE(set_sink_fn, set_sink, "ai_Talker_setSink");
#undef RESOLVE
    int status = initialize(NULL);
    if (status == 0) status = authenticate(argv[2], code, product);
    void *talker = status == 0 ? talker_new() : NULL;
    if (!talker && status == 0) status = -1001;
    if (status == 0) status = load_lang(talker, "standard", argv[3], 1);
    if (status == 0) status = select_lang(talker, "standard");
    reply(status, 0);
    if (status != 0) {
        if (talker) talker_delete(talker);
        terminate(); FreeLibrary(engine); return 4;
    }
    char selected_package[8193] = {0};
    char selected_language[8193];
    strcpy(selected_language, argv[3]);
    uint32_t lengths[7];
    while (fread(lengths, sizeof(lengths), 1, stdin) == 1) {
        char *fields[7] = {0};
        int valid = 1;
        for (unsigned i = 0; i < 7; i++) {
            if (!lengths[i] || lengths[i] > 32768) { valid = 0; break; }
            fields[i] = malloc((size_t)lengths[i] + 1);
            if (!fields[i] || fread(fields[i], 1, lengths[i], stdin) != lengths[i]) { valid = 0; break; }
            fields[i][lengths[i]] = 0;
            if (strlen(fields[i]) != lengths[i]) { valid = 0; break; }
        }
        if (!valid) { for (unsigned i = 0; i < 7; i++) free(fields[i]); break; }
        ULONGLONG began = GetTickCount64();
        int warm = strcmp(fields[4], "@warm") == 0;
        status = 0;
        if (strlen(fields[2]) >= sizeof(selected_package)
                || strlen(fields[6]) >= sizeof(selected_language)) status = -3;
        if (status == 0 && strcmp(selected_language, fields[6]) != 0) {
            // Selecting the voice alone leaves text analysis in the old dialect.
            // This made Akane (NV) and Aoi (NV) identical at neutral style.
            status = unload_lang(talker);
            selected_language[0] = 0;
            if (status == 0) status = load_lang(talker, fields[5], fields[6], 1);
            if (status == 0) status = select_lang(talker, fields[5]);
            if (status == 0) strcpy(selected_language, fields[6]);
        }
        if (status == 0 && strcmp(selected_package, fields[2]) != 0) {
            if (selected_package[0]) status = unload_voice(talker);
            selected_package[0] = 0;
            if (status == 0) status = load_voice(talker, fields[1], fields[2], 1, fields[3]);
            if (status == 0) status = select_voice(talker, fields[1]);
            if (status == 0) strcpy(selected_package, fields[2]);
        }
        audio_output *out = calloc(1, sizeof(*out));
        ULONGLONG loaded = GetTickCount64();
        void *sink = NULL;
        int fs = status == 0 ? get_fs(talker) : 0;
        if (status == 0 && (!out || fs <= 0 || fs > 192000)) status = -1;
        if (status == 0 && !warm) {
            out->file = fopen(fields[4], "wb+");
            if (!out->file || !wav_header(out->file, (unsigned)fs, 0)) status = -7;
        }
        if (status == 0 && !warm) {
            out->started = GetTickCount64();
            sink = sink_new(on_event, out);
            status = sink ? set_sink(talker, sink) : -1;
        }
        // Encoding 0 is UTF-8; 1 is CP932. Passing UTF-8 with 1 reads mojibake.
        if (status == 0 && !warm) status = talk(talker, fields[0], 0);
        if (sink) { set_sink(talker, NULL); sink_delete(sink); }
        uint32_t bytes = out ? out->bytes : 0;
        if (status == 0 && !warm && (!bytes || out->failed)) status = -12;
        if (out && out->file) {
            if (!wav_header(out->file, (unsigned)fs, bytes)) status = -7;
            if (fclose(out->file) != 0) status = -7;
        }
        ULONGLONG done = GetTickCount64();
        uint32_t times[4] = {(uint32_t)(loaded - began),
            out && out->first_pcm ? (uint32_t)(out->first_pcm - began) : 0,
            (uint32_t)(done - loaded), (uint32_t)(done - began)};
        free(out);
        if (status != 0 && !warm) remove(fields[4]);
        for (unsigned i = 0; i < 7; i++) free(fields[i]);
        reply(status, bytes);
        if (getenv("AIVOICE2_WORKER_METRICS")) {
            fwrite(times, sizeof(times), 1, stdout);
            fflush(stdout);
        }
    }
    talker_delete(talker);
    terminate(); FreeLibrary(engine);
    return 0;
}
