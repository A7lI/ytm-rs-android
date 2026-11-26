# ytm-rs 0.3.3 for Android

This is an unofficial Android port of [grapedjuice/ytm-rs v0.3.3](https://github.com/grapedjuice/ytm-rs/tree/v0.3.3). It uses Android NativeActivity and the upstream Rust audio engine and service integrations. The APK targets ARM64 and x86-64 devices with Android 8 or later. Its package ID is `org.ytmrs.android`.

## Android build 0.3.3-android.9

- A swipe over a shelf stays on its first axis until release. Touch scrolling stops at release and at content edges.
- Settings shows only the Settings search. In portrait, music search results stack the top result above the song list.

## Android build 0.3.3-android.5

- Portrait albums and playlists use a centered cover and full-width title, with compact action rows and the tracks below.
- Android's window insets keep the search bar, panels, and bottom navigation clear of system bars and display cutouts.
- If the existing lyrics providers return no results, the app checks LRCLIB for a duration-matched entry. On a phone, the Lyrics tab shows a retry action when no lyrics are found.

## Android build 0.3.3-android.4

- Android's Back gesture, navigation button, and physical Back key close app panels or go to the previous page. At the root, Back sends the app to the background.
- The sign-in dialog accepts the soft keyboard and resizes when it opens.
- The player remembers playback across a temporary audio-focus interruption and resumes when Android restores focus. Pausing explicitly cancels automatic resume.

## Android build 0.3.3-android.3

- An Android swipe can scroll a horizontal shelf and its vertical page at the same time. A swipe stays with the shelf where it started.
- Vertical lists accept touch drags immediately, and synced lyrics can be dragged up and down.

## Android build 0.3.3-android.2

- Portrait Now Playing has artwork, track details, a seek bar, centered controls, and touch tabs for the track, lyrics, queue, and comments.
- Noto Sans Arabic is bundled for Arabic titles, artists, and lyrics. The app's interface labels remain in English.
- Search uses Android keyboard input, suggestions from the existing backend, and recent queries stored on the device. Recent searches can be cleared in the dialog.
- Export to Files is available in a downloaded track's menu. The app's offline copy stays in app storage.
- Sign-in dialogs show loading and failure feedback and provide Retry. Embedded sign-in depends on the provider accepting Android WebView.
- The Android playback service handles audio focus and headphone disconnection. The native event loop services the playback queue and media callbacks even when the Android surface is suspended.

Desktop tray, global keyboard shortcuts, and update installation have no implementation in this port. Lock-screen playback, live sign-in, and Android UI behavior still need device testing.

The original license is in `LICENSE`.

## Build

Build on x86-64 Linux with the Rust toolchain pinned by the dependency requirements, Android SDK platform 35, Build Tools 35.0.1, NDK 28.2.13676358, Java JDK 17 or later, and Zip. Install both Android Rust targets.

```bash
rustup target add aarch64-linux-android x86_64-linux-android
export ANDROID_HOME=/path/to/android-sdk
export ANDROID_KEYSTORE=/path/to/signing-keystore
export ANDROID_KEY_ALIAS=your-key-alias
android/build-apk.sh
```

APK signing prompts for the keystore password by default. The preview signing key is excluded from this source package.

### Discord listening activity

To include Rich Presence, set `DISCORD_ANDROID_AAR` to the absolute path of
`discord_partner_sdk.aar` from Discord Social SDK 1.10 or later. Set
`DISCORD_APPLICATION_ID` to the public Application ID from Discord Developer
Portal. The source package excludes the SDK. The build includes both Android
ABIs from the AAR and strips native debug symbols.

In the app, turn on **Settings → Connections → Discord** while signed in to
the Discord app on the same phone. ytm-rs sends playback details to Discord
while playing and clears the activity when playback pauses or the toggle is
turned off.
