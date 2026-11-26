#include <jni.h>
#include <android/log.h>
#include <chrono>
#include <optional>
#include <string>

#define DISCORDPP_IMPLEMENTATION
#include "discordpp.h"

namespace {
using Clock = std::chrono::steady_clock;
using namespace std::chrono_literals;

struct Presence {
    discordpp::Client client;
    std::optional<discordpp::Activity> activity;
    Clock::time_point nextUpdate{};
    Clock::time_point nextRefresh{};
    Clock::time_point drainUntil{};
    bool changed = false;

    Presence() {
        client.SetApplicationId(YTM_DISCORD_APPLICATION_ID);
    }
};

std::string stringFrom(JNIEnv* env, jbyteArray bytes) {
    std::string text(static_cast<size_t>(env->GetArrayLength(bytes)), '\0');
    if (!text.empty()) {
        env->GetByteArrayRegion(bytes, 0, static_cast<jsize>(text.size()), reinterpret_cast<jbyte*>(text.data()));
    }
    return text;
}
}

extern "C" JNIEXPORT jlong JNICALL
Java_org_ytmrs_android_DiscordPresence_create(JNIEnv*, jclass) {
    auto major = discordpp::Client::GetVersionMajor();
    auto minor = discordpp::Client::GetVersionMinor();
    if (major < 1 || (major == 1 && minor < 10)) {
        __android_log_print(ANDROID_LOG_ERROR, "ytm-discord", "Android Rich Presence requires Social SDK 1.10+");
        return 0;
    }
    return reinterpret_cast<jlong>(new Presence());
}

extern "C" JNIEXPORT void JNICALL
Java_org_ytmrs_android_DiscordPresence_publish(JNIEnv* env, jclass, jlong handle, jbyteArray title,
                                               jbyteArray artist, jbyteArray album, jbyteArray art,
                                               jbyteArray url, jlong start, jlong end) {
    auto& state = *reinterpret_cast<Presence*>(handle);
    auto& activity = state.activity.emplace();
    activity.SetType(discordpp::ActivityTypes::Listening);
    activity.SetStatusDisplayType(discordpp::StatusDisplayTypes::Details);
    activity.SetDetails(stringFrom(env, title));
    auto artistName = stringFrom(env, artist);
    if (artistName.size() >= 2) activity.SetState(artistName);

    auto image = stringFrom(env, art);
    if (!image.empty()) {
        discordpp::ActivityAssets assets;
        assets.SetLargeImage(image);
        auto albumName = stringFrom(env, album);
        if (albumName.size() >= 2) assets.SetLargeText(albumName);
        activity.SetAssets(assets);
    }
    auto songUrl = stringFrom(env, url);
    if (!songUrl.empty()) {
        discordpp::ActivityButton button;
        button.SetLabel("Listen on YouTube Music");
        button.SetUrl(songUrl);
        activity.AddButton(button);
    }
    if (start > 0) {
        discordpp::ActivityTimestamps timestamps;
        timestamps.SetStart(static_cast<uint64_t>(start));
        if (end > start) timestamps.SetEnd(static_cast<uint64_t>(end));
        activity.SetTimestamps(timestamps);
    }
    state.changed = true;
}

extern "C" JNIEXPORT void JNICALL
Java_org_ytmrs_android_DiscordPresence_clear(JNIEnv*, jclass, jlong handle) {
    auto& state = *reinterpret_cast<Presence*>(handle);
    if (state.activity) {
        state.activity.reset();
        state.client.ClearRichPresence();
        state.drainUntil = Clock::now() + 2s;
    }
}

extern "C" JNIEXPORT jboolean JNICALL
Java_org_ytmrs_android_DiscordPresence_tick(JNIEnv*, jclass, jlong handle) {
    auto& state = *reinterpret_cast<Presence*>(handle);
    discordpp::RunCallbacks();
    const auto now = Clock::now();
    if (state.activity && now >= state.nextUpdate && (state.changed || now >= state.nextRefresh)) {
        state.client.UpdateRichPresence(*state.activity, [](const discordpp::ClientResult& result) {
            if (!result.Successful()) {
                __android_log_print(ANDROID_LOG_DEBUG, "ytm-discord", "Presence update failed (%d/%d)",
                                    static_cast<int>(result.Type()), result.ErrorCode());
            }
        });
        state.nextUpdate = now + 2s;
        state.nextRefresh = now + 15s;
        state.changed = false;
    }
    return state.activity.has_value() || now < state.drainUntil;
}

extern "C" JNIEXPORT void JNICALL
Java_org_ytmrs_android_DiscordPresence_destroy(JNIEnv*, jclass, jlong handle) {
    delete reinterpret_cast<Presence*>(handle);
}
