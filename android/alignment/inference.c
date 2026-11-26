/* Small C boundary for the pinned ONNX Runtime Android library. */
#include "onnxruntime_c_api.h"
#include <android/log.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    const OrtApi *api;
    OrtEnv *env;
    OrtSession *session;
    OrtMemoryInfo *memory;
} Engine;

static int failed(const OrtApi *api, OrtStatus *status) {
    if (!status) return 0;
    __android_log_print(ANDROID_LOG_WARN, "LyricAlign", "%s", api->GetErrorMessage(status));
    api->ReleaseStatus(status);
    return 1;
}
void lyrics_engine_free(Engine *e) {
    if (!e) return;
    if (e->memory) e->api->ReleaseMemoryInfo(e->memory);
    if (e->session) e->api->ReleaseSession(e->session);
    if (e->env) e->api->ReleaseEnv(e->env);
    free(e);
}

Engine *lyrics_engine_new(const void *model, size_t len) {
    const OrtApi *api = OrtGetApiBase()->GetApi(ORT_API_VERSION);
    Engine *e = calloc(1, sizeof(*e));
    OrtSessionOptions *opts = NULL;
    if (!api || !e) { free(e); return NULL; }
    e->api = api;
    int error = failed(api, api->CreateEnv(ORT_LOGGING_LEVEL_WARNING, "lyric-sync", &e->env)) ||
        failed(api, api->CreateSessionOptions(&opts)) ||
        failed(api, api->SetIntraOpNumThreads(opts, 2)) ||
        failed(api, api->AddSessionConfigEntry(opts, "session.intra_op.allow_spinning", "0")) ||
        failed(api, api->SetSessionGraphOptimizationLevel(opts, ORT_ENABLE_EXTENDED)) ||
        failed(api, api->CreateSessionFromArray(e->env, model, len, opts, &e->session)) ||
        failed(api, api->CreateCpuMemoryInfo(OrtArenaAllocator, OrtMemTypeDefault, &e->memory));
    if (opts) api->ReleaseSessionOptions(opts);
    if (error) { lyrics_engine_free(e); return NULL; }
    return e;
}

int lyrics_logits(Engine *e, float *audio, size_t n, float *out, size_t capacity) {
    const OrtApi *api = e->api;
    OrtValue *input = NULL, *output = NULL;
    OrtTensorTypeAndShapeInfo *shape = NULL;
    const char *in_name = "input_values", *out_name = "logits";
    int64_t dims[] = {1, (int64_t)n};
    size_t count = 0;
    float *data = NULL;
    int error = failed(api, api->CreateTensorWithDataAsOrtValue(e->memory, audio, n * sizeof(float),
                  dims, 2, ONNX_TENSOR_ELEMENT_DATA_TYPE_FLOAT, &input)) ||
        failed(api, api->Run(e->session, NULL, &in_name, (const OrtValue *const *)&input, 1, &out_name, 1, &output)) ||
        failed(api, api->GetTensorTypeAndShape(output, &shape)) ||
        failed(api, api->GetTensorShapeElementCount(shape, &count)) ||
        count > capacity || count % 32 != 0 ||
        failed(api, api->GetTensorMutableData(output, (void **)&data));
    if (!error) memcpy(out, data, count * sizeof(float));
    if (shape) api->ReleaseTensorTypeAndShapeInfo(shape);
    if (output) api->ReleaseValue(output);
    if (input) api->ReleaseValue(input);
    return error ? -1 : (int)(count / 32);
}

