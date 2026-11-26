package org.ytmrs.android;

import android.app.AlertDialog;
import android.app.NativeActivity;
import android.content.ClipData;
import android.content.ComponentName;
import android.content.Intent;
import android.content.pm.ActivityInfo;
import android.content.pm.PackageManager;
import android.database.Cursor;
import android.net.Uri;
import android.os.Handler;
import android.os.Looper;
import android.provider.OpenableColumns;
import android.security.keystore.KeyGenParameterSpec;
import android.security.keystore.KeyProperties;
import android.util.Base64;
import android.webkit.CookieManager;
import android.webkit.WebView;
import android.webkit.WebResourceError;
import android.webkit.WebResourceRequest;
import android.webkit.WebResourceResponse;
import android.os.Bundle;
import android.os.Build;
import android.view.Window;
import android.view.WindowInsets;
import android.graphics.Insets;
import android.provider.DocumentsContract;
import android.widget.LinearLayout;
import android.widget.TextView;
import android.view.View;
import android.webkit.WebViewClient;
import android.widget.Toast;
import android.view.WindowManager;
import java.io.File;
import java.io.InputStream;
import java.io.OutputStream;
import java.io.IOException;
import java.nio.file.Files;
import java.security.KeyStore;
import java.util.ArrayList;
import java.util.List;
import java.util.TimeZone;
import javax.crypto.Cipher;
import javax.crypto.KeyGenerator;
import javax.crypto.SecretKey;
import javax.crypto.spec.GCMParameterSpec;
import org.json.JSONArray;
import org.json.JSONObject;

public final class PlayerActivity extends NativeActivity {
    static { System.loadLibrary("ytm_rs"); }
    private static final String KEY_ALIAS = "org.ytmrs.android.vault";
    private static final int PICK_AUDIO = 1;
    private static final int EXPORT_AUDIO = 2;
    private final Handler main = new Handler(Looper.getMainLooper());
    private File exportFile;
    private long uploadRequest;
    private String mediaTitle = "";
    private String mediaArtist = "";
    private String mediaAlbum = "";
    private String mediaCover = "";
    private long mediaDuration;
    private boolean mediaStarted;
    private AlertDialog signInDialog;
    private MusicSearchDialog searchDialog;
    private AlertDialog lyricsDialog;
    private Runnable unregisterBack;
    private DiscordPresence discordPresence;

    private native void requestFinished(long request, String value);
    private native void jamLink(String link);
    private native void findSuggestions(long revision, String query);
    private native boolean sendBackToPlayer();
    private native void safeAreaChanged(int left, int top, int right, int bottom);

    @Override protected void onCreate(Bundle state) {
        if (state != null && state.getString("exportPath") != null) {
            exportFile = new File(state.getString("exportPath"));
        }
        // NativeActivity looks up its library in the intent component's metadata.
        // Resolve it on the player itself, including after a previous alias is disabled.
        setIntent(new Intent(getIntent()).setClass(this, PlayerActivity.class));
        super.onCreate(state);
        configureSafeArea();
        if (Build.VERSION.SDK_INT >= 33) unregisterBack = BackApi33.register(this);
    }

    private void configureSafeArea() {
        Window window = getWindow();
        View decor = window.getDecorView();
        if (Build.VERSION.SDK_INT >= 30) {
            InsetsApi30.configure(window, decor, this);
        } else {
            decor.setSystemUiVisibility(decor.getSystemUiVisibility()
                | View.SYSTEM_UI_FLAG_LAYOUT_STABLE | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION);
            decor.setOnApplyWindowInsetsListener((view, insets) -> {
                int left = insets.getStableInsetLeft();
                int top = insets.getStableInsetTop();
                int right = insets.getStableInsetRight();
                int bottom = insets.getStableInsetBottom();
                if (Build.VERSION.SDK_INT >= 28 && insets.getDisplayCutout() != null) {
                    android.view.DisplayCutout cutout = insets.getDisplayCutout();
                    left = Math.max(left, cutout.getSafeInsetLeft());
                    top = Math.max(top, cutout.getSafeInsetTop());
                    right = Math.max(right, cutout.getSafeInsetRight());
                    bottom = Math.max(bottom, cutout.getSafeInsetBottom());
                }
                safeAreaChanged(left, top, right, bottom);
                return insets;
            });
        }
        decor.requestApplyInsets();
    }

    private static final class InsetsApi30 {
        static void configure(Window window, View decor, PlayerActivity activity) {
            // The native surface fills the window. Keep the system chrome above it and
            // let egui reserve the visible insets in the layout.
            window.setDecorFitsSystemWindows(false);
            decor.setOnApplyWindowInsetsListener((view, insets) -> {
                Insets safe = insets.getInsets(WindowInsets.Type.systemBars() | WindowInsets.Type.displayCutout());
                activity.safeAreaChanged(safe.left, safe.top, safe.right, safe.bottom);
                return insets;
            });
        }
    }

    @Override public void onBackPressed() {
        handleBack();
    }

    private void handleBack() {
        if (!sendBackToPlayer()) moveTaskToBack(true);
    }

    public void backgroundFromBack() {
        runOnUiThread(() -> {
            if (!isFinishing() && hasWindowFocus()) moveTaskToBack(true);
        });
    }

    @Override protected void onDestroy() {
        if (unregisterBack != null) unregisterBack.run();
        if (searchDialog != null) searchDialog.dismiss();
        if (lyricsDialog != null) lyricsDialog.dismiss();
        if (signInDialog != null) signInDialog.dismiss();
        if (discordPresence != null) discordPresence.close();
        super.onDestroy();
    }

    private static final class BackApi33 {
        static Runnable register(PlayerActivity activity) {
            android.window.OnBackInvokedDispatcher dispatcher = activity.getOnBackInvokedDispatcher();
            android.window.OnBackInvokedCallback callback = activity::handleBack;
            dispatcher.registerOnBackInvokedCallback(android.window.OnBackInvokedDispatcher.PRIORITY_DEFAULT, callback);
            return () -> dispatcher.unregisterOnBackInvokedCallback(callback);
        }
    }

    @Override protected void onSaveInstanceState(Bundle state) {
        if (exportFile != null) state.putString("exportPath", exportFile.getPath());
        super.onSaveInstanceState(state);
    }

    @Override protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        if (intent.getDataString() != null) jamLink(intent.getDataString());
    }

    public String initialLink() { return getIntent().getDataString(); }
    public String timeZone() { return TimeZone.getDefault().getID(); }

    public boolean hasDiscordPresence() { return DiscordPresence.bundled(this); }

    public void setDiscordPresence(String title, String artist, String album, String art, String url, long start, long end) {
        runOnUiThread(() -> {
            if (isDestroyed() || isFinishing()) return;
            if (discordPresence == null && title != null && hasDiscordPresence()) {
                try {
                    discordPresence = new DiscordPresence(this);
                } catch (ReflectiveOperationException | LinkageError error) {
                    android.util.Log.w("ytm-rs", "Couldn't initialize Discord Rich Presence", error);
                    Toast.makeText(this, "Couldn't initialize Discord Rich Presence", Toast.LENGTH_SHORT).show();
                }
            }
            if (discordPresence != null) discordPresence.set(title, artist, album, art, url, start, end);
        });
    }

    public boolean setLauncherIcon(String id) {
        String packageName = getPackageName();
        String prefix = packageName + ".Icon_";
        ComponentName selected = new ComponentName(packageName, prefix + id);
        PackageManager packages = getPackageManager();
        try {
            ActivityInfo[] activities = packages.getPackageInfo(packageName,
                PackageManager.GET_ACTIVITIES | PackageManager.MATCH_DISABLED_COMPONENTS).activities;
            if (activities == null) return false;

            List<ComponentName> enabledOthers = new ArrayList<>();
            ActivityInfo chosen = null;
            for (ActivityInfo info : activities) {
                if (!info.name.startsWith(prefix) || !PlayerActivity.class.getName().equals(info.targetActivity)) continue;
                ComponentName component = new ComponentName(packageName, info.name);
                if (component.equals(selected)) {
                    chosen = info;
                } else if (isAliasEnabled(packages, component, info)) {
                    enabledOthers.add(component);
                }
            }
            if (chosen == null) return false;

            boolean enableSelected = !isAliasEnabled(packages, selected, chosen);
            if (!enableSelected && enabledOthers.isEmpty()) return true;
            if (Build.VERSION.SDK_INT >= 33) {
                LauncherIconApi33.apply(packages, selected, enableSelected, enabledOthers);
            } else {
                if (enableSelected) packages.setComponentEnabledSetting(selected,
                    PackageManager.COMPONENT_ENABLED_STATE_ENABLED, PackageManager.DONT_KILL_APP);
                for (ComponentName component : enabledOthers) {
                    packages.setComponentEnabledSetting(component,
                        PackageManager.COMPONENT_ENABLED_STATE_DISABLED, PackageManager.DONT_KILL_APP);
                }
            }
            return true;
        } catch (PackageManager.NameNotFoundException | RuntimeException error) {
            android.util.Log.w("ytm-rs", "Could not change the launcher icon", error);
            return false;
        }
    }

    private static boolean isAliasEnabled(PackageManager packages, ComponentName component, ActivityInfo info) {
        int state = packages.getComponentEnabledSetting(component);
        return state == PackageManager.COMPONENT_ENABLED_STATE_ENABLED
            || (state == PackageManager.COMPONENT_ENABLED_STATE_DEFAULT && info.enabled);
    }

    private static final class LauncherIconApi33 {
        static void apply(PackageManager packages, ComponentName selected, boolean enableSelected,
                          List<ComponentName> enabledOthers) {
            List<PackageManager.ComponentEnabledSetting> changes = new ArrayList<>();
            if (enableSelected) changes.add(new PackageManager.ComponentEnabledSetting(selected,
                PackageManager.COMPONENT_ENABLED_STATE_ENABLED, PackageManager.DONT_KILL_APP));
            for (ComponentName component : enabledOthers) {
                changes.add(new PackageManager.ComponentEnabledSetting(component,
                    PackageManager.COMPONENT_ENABLED_STATE_DISABLED, PackageManager.DONT_KILL_APP));
            }
            packages.setComponentEnabledSettings(changes);
        }
    }

    public void startupError(String error) {
        runOnUiThread(() -> new AlertDialog.Builder(this)
            .setTitle("ytm-rs could not start").setMessage(error)
            .setPositiveButton(android.R.string.ok, (dialog, button) -> finish()).show());
    }

    public void search(long request, boolean light) {
        runOnUiThread(() -> {
            if (isFinishing() || isDestroyed() || searchDialog != null) {
                requestFinished(request, null);
                return;
            }
            searchDialog = new MusicSearchDialog(this, light, this::findSuggestions, query -> {
                searchDialog = null;
                requestFinished(request, query);
            });
            searchDialog.show();
        });
    }

    private int dp(int points) {
        return Math.round(points * getResources().getDisplayMetrics().density);
    }

    public void showSuggestions(long revision, String results) {
        runOnUiThread(() -> {
            if (searchDialog != null) searchDialog.showSuggestions(revision, results);
        });
    }

    public void signIn(long request, boolean spotify) {
        runOnUiThread(() -> {
            if (isFinishing() || signInDialog != null) {
                requestFinished(request, null);
                return;
            }
            String home = spotify ? "https://open.spotify.com/" : "https://music.youtube.com/";
            String start = spotify
                ? "https://accounts.spotify.com/en/login?continue=https%3A%2F%2Fopen.spotify.com%2F"
                : "https://accounts.google.com/ServiceLogin?service=youtube&continue=https%3A%2F%2Fmusic.youtube.com%2F";
            String requiredCookie = spotify ? "sp_dc" : "SAPISID";
            WebView webView = new WebView(this) {
                @Override protected void onMeasure(int widthSpec, int heightSpec) {
                    int height = Math.min(dp(500), getResources().getDisplayMetrics().heightPixels / 2);
                    if (MeasureSpec.getMode(heightSpec) != MeasureSpec.UNSPECIFIED) {
                        height = Math.min(height, MeasureSpec.getSize(heightSpec));
                    }
                    super.onMeasure(widthSpec, MeasureSpec.makeMeasureSpec(height, MeasureSpec.EXACTLY));
                }
            };
            webView.setFocusableInTouchMode(true);
            webView.getSettings().setJavaScriptEnabled(true);
            webView.getSettings().setDomStorageEnabled(true);
            webView.getSettings().setAllowFileAccess(false);
            webView.getSettings().setAllowContentAccess(false);
            LinearLayout content = new LinearLayout(this);
            content.setOrientation(LinearLayout.VERTICAL);
            TextView status = new TextView(this);
            status.setPadding(dp(20), dp(8), dp(20), dp(8));
            status.setAccessibilityLiveRegion(View.ACCESSIBILITY_LIVE_REGION_POLITE);
            content.addView(status);
            content.addView(webView, new LinearLayout.LayoutParams(-1, -2));
            AlertDialog dialog = new AlertDialog.Builder(this)
                .setTitle(spotify ? "Sign in to Spotify" : "Sign in to YouTube Music")
                .setView(content).setNegativeButton(android.R.string.cancel, null)
                .setNeutralButton("Retry", null).create();
            signInDialog = dialog;
            String[] result = {null};
            boolean[] navigationFailed = {false};
            Runnable slow = () -> status.setText("Sign-in is taking longer than expected. Check your connection or tap Retry.");
            dialog.setOnDismissListener(ignored -> {
                signInDialog = null;
                main.removeCallbacks(slow);
                webView.stopLoading();
                webView.destroy();
                requestFinished(request, result[0]);
                CookieManager.getInstance().removeAllCookies(null);
            });
            webView.setWebViewClient(new WebViewClient() {
                @Override public void onPageStarted(WebView view, String url, android.graphics.Bitmap icon) {
                    navigationFailed[0] = false;
                    status.setText("Loading sign-in…");
                    main.removeCallbacks(slow);
                    main.postDelayed(slow, 30000);
                }

                @Override public void onPageFinished(WebView view, String url) {
                    main.removeCallbacks(slow);
                    Uri page = Uri.parse(url);
                    Uri service = Uri.parse(home);
                    if (url.contains("disallowed_useragent")) {
                        status.setText("This provider blocked sign-in in the app. You can keep browsing signed out.");
                        return;
                    }
                    if (navigationFailed[0]) return;
                    status.setText("");
                    if (!"https".equals(page.getScheme()) || !service.getHost().equals(page.getHost())) return;
                    String cookies = CookieManager.getInstance().getCookie(home);
                    if (cookies != null) {
                        for (String cookie : cookies.split(";")) {
                            String part = cookie.trim();
                            if (!part.startsWith(requiredCookie + "=")) continue;
                            result[0] = spotify ? part.substring(requiredCookie.length() + 1) : cookies;
                            dialog.dismiss();
                            return;
                        }
                    }
                    status.setText("No signed-in session was received. Tap Retry to start again.");
                }

                @Override public void onReceivedError(WebView view, WebResourceRequest webRequest, WebResourceError error) {
                    if (!webRequest.isForMainFrame()) return;
                    navigationFailed[0] = true;
                    main.removeCallbacks(slow);
                    status.setText("Could not load the sign-in page. Check your connection and tap Retry.");
                }

                @Override public void onReceivedHttpError(WebView view, WebResourceRequest webRequest, WebResourceResponse error) {
                    if (!webRequest.isForMainFrame()) return;
                    navigationFailed[0] = true;
                    main.removeCallbacks(slow);
                    status.setText("The sign-in server returned an error (" + error.getStatusCode() + "). Tap Retry to try again.");
                }
            });
            dialog.show();
            // AlertDialog may mark a WebView as non-editable before an HTML field gets focus.
            Window window = dialog.getWindow();
            if (window != null) {
                window.clearFlags(WindowManager.LayoutParams.FLAG_ALT_FOCUSABLE_IM);
                window.setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE
                    | WindowManager.LayoutParams.SOFT_INPUT_STATE_UNCHANGED);
            }
            dialog.getButton(AlertDialog.BUTTON_NEUTRAL).setOnClickListener(ignored -> {
                webView.stopLoading();
                webView.loadUrl(start);
            });
            webView.loadUrl(start);
        });
    }

    public void verifyLyrics(long request) {
        runOnUiThread(() -> {
            if (isFinishing() || lyricsDialog != null) {
                requestFinished(request, null);
                return;
            }
            final String challenge = "https://lyrics.api.dacubeking.com/challenge";
            Handler main = new Handler(Looper.getMainLooper());
            WebView webView = new WebView(this);
            webView.getSettings().setJavaScriptEnabled(true);
            webView.getSettings().setDomStorageEnabled(true);
            webView.getSettings().setUseWideViewPort(true);
            webView.getSettings().setLoadWithOverviewMode(true);
            CookieManager.getInstance().setAcceptThirdPartyCookies(webView, true);
            webView.getSettings().setAllowFileAccess(false);
            webView.getSettings().setAllowContentAccess(false);
            LinearLayout content = new LinearLayout(this);
            content.setOrientation(LinearLayout.VERTICAL);
            int padding = (int) (16 * getResources().getDisplayMetrics().density);
            TextView status = new TextView(this);
            status.setText("Loading secure verification… You can cancel and keep using available lyrics.");
            status.setPadding(padding, padding / 2, padding, padding / 2);
            content.addView(status);
            content.addView(webView, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, padding * 12));
            AlertDialog dialog = new AlertDialog.Builder(this).setTitle("Verify lyrics access")
                .setView(content).setNegativeButton(android.R.string.cancel, null).create();
            lyricsDialog = dialog;
            String[] answer = {null};
            boolean[] widgetLoaded = {false};
            Runnable poll = new Runnable() {
                @Override public void run() {
                    if (lyricsDialog != dialog) return;
                    String url = webView.getUrl();
                    Uri page = url == null ? Uri.EMPTY : Uri.parse(url);
                    if ("https".equals(page.getScheme())
                        && "lyrics.api.dacubeking.com".equals(page.getHost())
                        && (page.getPort() == -1 || page.getPort() == 443)
                        && "/challenge".equals(page.getPath())) {
                        webView.evaluateJavascript("(function(){ if (!window.__ytmrsListening) {"
                            + "window.__ytmrsListening=true; window.addEventListener('message',function(e){"
                            + "if (e.source===window && e.origin===location.origin && e.data && typeof e.data.type==='string'"
                            + "&& e.data.type.indexOf('turnstile-')===0) window.__ytmrsEvent=e.data; }); }"
                            + "var result=window.__ytmrsEvent||null; window.__ytmrsEvent=null;"
                            + "var input=document.querySelector('[name=cf-turnstile-response]');"
                            + "if (!result && input && input.value) result={type:'turnstile-token',token:input.value};"
                            + "return result || {type:window.turnstile ? 'widget-loaded' : 'widget-loading'}; })()", result -> {
                            if (lyricsDialog != dialog || result == null || "null".equals(result)) return;
                            try {
                                JSONObject response = new JSONObject(result);
                                switch (response.optString("type")) {
                                    case "turnstile-token":
                                        answer[0] = response.getString("token");
                                        dialog.dismiss();
                                        break;
                                    case "turnstile-error":
                                        String code = response.optString("error");
                                        if (code.matches("[0-9]{3,8}")) {
                                            answer[0] = "error:Karaoke verification was rejected (code " + code + ").";
                                        } else {
                                            answer[0] = "error:Karaoke verification was rejected.";
                                        }
                                        dialog.dismiss();
                                        break;
                                    case "turnstile-timeout":
                                        answer[0] = "error:Karaoke verification timed out.";
                                        dialog.dismiss();
                                        break;
                                    case "widget-loaded":
                                        if (!widgetLoaded[0]) {
                                            widgetLoaded[0] = true;
                                            status.setText("Complete the security check if it appears.");
                                        }
                                        break;
                                    case "turnstile-expired":
                                        webView.evaluateJavascript("turnstile.reset()", null);
                                        break;
                                    default: break;
                                }
                            } catch (Exception ignored) {}
                        });
                    }
                    main.postDelayed(this, 300);
                }
            };
            Runnable timeout = () -> {
                if (lyricsDialog != dialog) return;
                answer[0] = widgetLoaded[0] ? "error:Karaoke verification timed out."
                    : "error:The karaoke verification widget couldn't load.";
                dialog.dismiss();
            };
            dialog.setOnDismissListener(ignored -> {
                lyricsDialog = null;
                main.removeCallbacks(poll);
                main.removeCallbacks(timeout);
                webView.stopLoading();
                webView.destroy();
                if (answer[0] != null && answer[0].startsWith("error:")) {
                    android.util.Log.w("ytm-rs", answer[0].substring(6));
                }
                requestFinished(request, answer[0]);
            });
            webView.setWebViewClient(new WebViewClient() {
                @Override public void onPageFinished(WebView view, String url) {
                    if (lyricsDialog == dialog && !widgetLoaded[0]) {
                        status.setText("Connecting to the karaoke verification service…");
                    }
                }
                @Override public void onReceivedError(WebView view, WebResourceRequest resource, WebResourceError error) {
                    if (resource.isForMainFrame() && lyricsDialog == dialog) {
                        answer[0] = "error:The karaoke verification page couldn't load.";
                        dialog.dismiss();
                    }
                }
                @Override public void onReceivedHttpError(WebView view, WebResourceRequest resource, WebResourceResponse response) {
                    if (resource.isForMainFrame() && response.getStatusCode() >= 400 && lyricsDialog == dialog) {
                        answer[0] = "error:Karaoke verification service returned HTTP " + response.getStatusCode() + ".";
                        dialog.dismiss();
                    }
                }
            });
            dialog.show();
            webView.loadUrl(challenge);
            main.post(poll);
            main.postDelayed(timeout, 30000);
        });
    }

    public void clearSignIn() {
        runOnUiThread(() -> {
            if (signInDialog == null) CookieManager.getInstance().removeAllCookies(null);
        });
    }

    public void pickAudio(long request) {
        runOnUiThread(() -> {
            if (uploadRequest != 0 || isFinishing()) {
                requestFinished(request, null);
                return;
            }
            Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT)
                .addCategory(Intent.CATEGORY_OPENABLE)
                .setType("audio/*").putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true);
            try {
                uploadRequest = request;
                startActivityForResult(intent, PICK_AUDIO);
            } catch (RuntimeException error) {
                uploadRequest = 0;
                requestFinished(request, null);
            }
        });
    }

    public void exportAudio(String path) {
        runOnUiThread(() -> {
            if (exportFile != null || isFinishing()) return;
            File source = new File(path);
            if (!source.isFile()) {
                Toast.makeText(this, "Download is no longer available", Toast.LENGTH_LONG).show();
                return;
            }
            Intent intent = new Intent(Intent.ACTION_CREATE_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE)
                .setType("audio/mp4").putExtra(Intent.EXTRA_TITLE, source.getName());
            try {
                exportFile = source;
                startActivityForResult(intent, EXPORT_AUDIO);
            } catch (RuntimeException error) {
                exportFile = null;
                Toast.makeText(this, "Could not open the file picker", Toast.LENGTH_LONG).show();
            }
        });
    }

    private void copyExport(File source, Uri destination) {
        String message;
        try (OutputStream output = getContentResolver().openOutputStream(destination, "wt")) {
            if (output == null) throw new IOException("No export stream");
            Files.copy(source.toPath(), output);
            output.flush();
            message = "Song exported to Files";
        } catch (Exception error) {
            try { DocumentsContract.deleteDocument(getContentResolver(), destination); } catch (Exception ignored) {}
            message = "Could not export the song";
        }
        String outcome = message;
        runOnUiThread(() -> Toast.makeText(this, outcome, Toast.LENGTH_LONG).show());
    }

    @Override protected void onActivityResult(int code, int result, Intent data) {
        super.onActivityResult(code, result, data);
        if (code == EXPORT_AUDIO) {
            File source = exportFile;
            exportFile = null;
            if (result == RESULT_OK && source != null && data != null && data.getData() != null) {
                new Thread(() -> copyExport(source, data.getData()), "audio-export").start();
            }
            return;
        }
        if (code != PICK_AUDIO) return;
        long request = uploadRequest;
        uploadRequest = 0;
        if (result != RESULT_OK || data == null) {
            requestFinished(request, null);
            return;
        }
        List<Uri> items = new ArrayList<>();
        ClipData selection = data.getClipData();
        if (selection != null) {
            for (int index = 0; index < selection.getItemCount(); index++) {
                items.add(selection.getItemAt(index).getUri());
            }
        } else if (data.getData() != null) {
            items.add(data.getData());
        }
        new Thread(() -> copyAudio(request, items), "audio-picker").start();
    }

    private void copyAudio(long request, List<Uri> items) {
        JSONArray files = new JSONArray();
        try {
            for (Uri item : items) {
                String name = null;
                try (Cursor cursor = getContentResolver().query(item,
                        new String[]{OpenableColumns.DISPLAY_NAME}, null, null, null)) {
                    if (cursor != null && cursor.moveToFirst()) name = cursor.getString(0);
                }
                if (name == null || name.trim().isEmpty()) continue;
                File parent = Files.createTempDirectory(getCacheDir().toPath(), "upload-").toFile();
                File dest = new File(parent, new File(name).getName());
                try (InputStream stream = getContentResolver().openInputStream(item)) {
                    if (stream == null) continue;
                    Files.copy(stream, dest.toPath());
                    files.put(dest.getAbsolutePath());
                }
            }
        } catch (Exception error) {
            runOnUiThread(() -> Toast.makeText(this, "Some selected files could not be read", Toast.LENGTH_LONG).show());
        }
        requestFinished(request, files.toString());
    }

    public void shareText(String text) {
        runOnUiThread(() -> {
            Intent share = new Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, text);
            startActivity(Intent.createChooser(share, null));
        });
    }

    public void mediaMetadata(String title, String artist, String album, String cover, long duration) {
        runOnUiThread(() -> {
            mediaTitle = title;
            mediaArtist = artist;
            mediaAlbum = album;
            mediaCover = cover;
            mediaDuration = duration;
        });
    }

    public void mediaPlayback(boolean playing, long position) {
        runOnUiThread(() -> {
            if (mediaTitle.isEmpty() || (!mediaStarted && !playing)) return;
            Intent update = new Intent(this, PlaybackService.class).setAction(PlaybackService.UPDATE)
                .putExtra("title", mediaTitle).putExtra("artist", mediaArtist)
                .putExtra("album", mediaAlbum).putExtra("cover", mediaCover)
                .putExtra("duration", mediaDuration).putExtra("playing", playing).putExtra("position", position);
            try {
                if (!PlaybackService.deliver(update) && playing) startForegroundService(update);
                mediaStarted = true;
            } catch (IllegalStateException ignored) {
                // The system can deny a new foreground service while the activity is hidden.
            }
        });
    }

    public void cancelAutomaticResume() {
        runOnUiThread(PlaybackService::cancelAutomaticResume);
    }

    public void closeMedia() {
        runOnUiThread(() -> {
            mediaStarted = false;
            stopService(new Intent(this, PlaybackService.class));
        });
    }

    private SecretKey vaultKey() throws Exception {
        KeyStore keyStore = KeyStore.getInstance("AndroidKeyStore");
        keyStore.load(null);
        SecretKey key = (SecretKey) keyStore.getKey(KEY_ALIAS, null);
        if (key != null) return key;
        KeyGenerator generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore");
        generator.init(new KeyGenParameterSpec.Builder(KEY_ALIAS, KeyProperties.PURPOSE_ENCRYPT | KeyProperties.PURPOSE_DECRYPT)
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE).build());
        return generator.generateKey();
    }

    public synchronized byte[] vaultGet(String key) throws Exception {
        String saved = getSharedPreferences("vault", MODE_PRIVATE).getString(key, null);
        if (saved == null) return null;
        String[] parts = saved.split(":", 2);
        Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
        cipher.init(Cipher.DECRYPT_MODE, vaultKey(),
            new GCMParameterSpec(128, Base64.decode(parts[0], Base64.NO_WRAP)));
        cipher.updateAAD(key.getBytes(java.nio.charset.StandardCharsets.UTF_8));
        return cipher.doFinal(Base64.decode(parts[1], Base64.NO_WRAP));
    }

    public synchronized boolean vaultSet(String key, byte[] value) throws Exception {
        Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
        cipher.init(Cipher.ENCRYPT_MODE, vaultKey());
        cipher.updateAAD(key.getBytes(java.nio.charset.StandardCharsets.UTF_8));
        String saved = Base64.encodeToString(cipher.getIV(), Base64.NO_WRAP) + ":" +
            Base64.encodeToString(cipher.doFinal(value), Base64.NO_WRAP);
        return getSharedPreferences("vault", MODE_PRIVATE).edit().putString(key, saved).commit();
    }

    public synchronized boolean vaultDelete(String key) {
        return getSharedPreferences("vault", MODE_PRIVATE).edit().remove(key).commit();
    }
}
