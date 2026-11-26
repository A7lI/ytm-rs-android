#include <algorithm>
#include <cmath>
#include "whisper.h"

#ifdef _WIN32
#define YTM_EXPORT __declspec(dllexport)
#else
#define YTM_EXPORT __attribute__((visibility("default")))
#endif

extern "C" {
using Abort = bool (*)(void *);
using Segment = void (*)(void *, int, int, const char *);

YTM_EXPORT int ytm_transcribe(const void *model, size_t model_size,
        const float *audio, int samples, void *state, Abort abort, Segment segment) {
    auto config = whisper_context_default_params();
    config.use_gpu = false;
    whisper_context *ctx = whisper_init_from_buffer_with_params(const_cast<void *>(model), model_size, config);
    if (!ctx) return -1;

    auto options = whisper_full_default_params(WHISPER_SAMPLING_GREEDY);
    options.n_threads = 3;
    options.language = "auto";
    options.no_context = true;
    options.temperature_inc = 0.0f;
    options.no_speech_thold = 0.5f;
    options.suppress_nst = true;
    options.print_progress = options.print_realtime = options.print_timestamps = options.print_special = false;
    options.abort_callback = abort;
    options.abort_callback_user_data = state;
    int result = whisper_full(ctx, options, audio, samples);
    if (result == 0 && !abort(state)) {
        auto eot = whisper_token_eot(ctx);
        for (int i = 0; i < whisper_full_n_segments(ctx); ++i) {
            if (whisper_full_get_segment_no_speech_prob(ctx, i) > 0.50f) continue;
            int count = 0;
            float confidence = 0.0f;
            for (int t = 0; t < whisper_full_n_tokens(ctx, i); ++t) {
                auto token = whisper_full_get_token_data(ctx, i, t);
                if (token.id >= eot) continue;
                confidence += token.plog;
                ++count;
            }
            // Drop unreliable phrases and keep silent gaps. The result is labeled as
            // a transcript because music and overlapping vocals can still cause errors.
            if (!count || confidence / count < -0.95f) continue;
            int start = int(whisper_full_get_segment_t0(ctx, i) * 10);
            int end = std::min(samples / 16, int(whisper_full_get_segment_t1(ctx, i) * 10));
            if (end > start) segment(state, start, end, whisper_full_get_segment_text(ctx, i));
        }
    }
    whisper_free(ctx);
    return result;
}
}

