use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};
use std::time::Duration;

use jni::objects::{GlobalRef, JByteArray, JObject, JString, JValue};
use jni::sys::{jboolean, jint, jlong};
use jni::{JNIEnv, JavaVM};
use winit::platform::android::activity::AndroidApp;

pub mod lyric_audio;
pub(crate) static ALIGN_APP: Mutex<Option<AndroidApp>> = Mutex::new(None);

struct Activity {
    vm: JavaVM,
    instance: GlobalRef,
}

static JAVA: Mutex<Option<Activity>> = Mutex::new(None);
type EventLoopWake = Arc<dyn Fn() + Send + Sync>;
static EVENT_LOOP_WAKE: Mutex<Option<EventLoopWake>> = Mutex::new(None);
static SEARCH_BACKEND: Mutex<Option<crate::backend::Backend>> = Mutex::new(None);
static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();
static SAFE_AREA_PX: Mutex<[i32; 4]> = Mutex::new([0; 4]);
static BACK_REQUESTS: AtomicUsize = AtomicUsize::new(0);
static REQUEST_ID: AtomicI64 = AtomicI64::new(1);
type RequestSender = crossbeam_channel::Sender<Option<String>>;
static REQUESTS: LazyLock<Mutex<HashMap<i64, RequestSender>>> = LazyLock::new(Default::default);

pub fn initialize(app: &AndroidApp) {
    *ALIGN_APP.lock().unwrap() = Some(app.clone());
    let path = app.internal_data_path().expect("Android internal data directory");
    std::fs::create_dir_all(&path).expect("create Android data directory");
    let _ = DATA_DIR.set(path);
    // AndroidActivity owns the VM and activity. Keep a JNI global reference for the
    // duration of the Rust event loop so worker threads can call Java UI methods.
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }.expect("Android Java VM");
    let instance = {
        let env = vm.attach_current_thread().expect("attach current thread");
        let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
        env.new_global_ref(activity).expect("retain Android activity")
    };
    *JAVA.lock().unwrap() = Some(Activity { vm, instance });
    if let Some(link) = string_method("initialLink")
        && let Some(code) = crate::deeplink::parse(&link)
    {
        crate::deeplink::push(code);
    }
}

pub fn done() {
    *ALIGN_APP.lock().unwrap() = None;
    *EVENT_LOOP_WAKE.lock().unwrap() = None;
    BACK_REQUESTS.store(0, Ordering::Relaxed);
    *SEARCH_BACKEND.lock().unwrap() = None;
    REQUESTS.lock().unwrap().clear();
    *JAVA.lock().unwrap() = None;
}

pub fn data_dir() -> &'static Path {
    DATA_DIR.get().expect("Android initialized")
}

pub(super) fn with_activity<T>(f: impl FnOnce(&mut JNIEnv<'_>, &JObject<'_>) -> jni::errors::Result<T>) -> anyhow::Result<T> {
    let entry = JAVA.lock().unwrap();
    let activity = entry.as_ref().ok_or_else(|| anyhow::anyhow!("Android activity is closed"))?;
    let mut env = activity.vm.attach_current_thread()?;
    let result = env.with_local_frame(24, |env| {
        let result = f(env, activity.instance.as_obj());
        if env.exception_check()? {
            env.exception_clear()?;
            return Err(jni::errors::Error::JavaException);
        }
        result
    });
    Ok(result?)
}

fn string_method(method: &str) -> Option<String> {
    with_activity(|env, activity| {
        let value = JString::from(env.call_method(activity, method, "()Ljava/lang/String;", &[])?.l()?);
        if value.is_null() {
            return Ok(None);
        }
        env.get_string(&value).map(|s| Some(s.into()))
    }).ok().flatten()
}

pub fn time_zone() -> String {
    string_method("timeZone").unwrap_or_else(|| "UTC".into())
}

pub fn set_launcher_icon(id: &str) -> anyhow::Result<()> {
    let applied = with_activity(|env, activity| {
        let id = env.new_string(id)?;
        env.call_method(activity, "setLauncherIcon", "(Ljava/lang/String;)Z", &[JValue::Object(&id)])?.z()
    })?;
    anyhow::ensure!(applied, "Android couldn't apply the launcher icon");
    Ok(())
}

pub fn startup_failed(error: &str) {
    log::error!("Android startup: {error}");
    let _ = with_activity(|env, activity| {
        let message = env.new_string(error)?;
        env.call_method(activity, "startupError", "(Ljava/lang/String;)V", &[JValue::Object(&message)])?;
        Ok(())
    });
}

pub fn request(method: &str, flag: Option<bool>, timeout: Duration) -> anyhow::Result<Option<String>> {
    let request = REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    let (send, receive) = crossbeam_channel::bounded(1);
    REQUESTS.lock().unwrap().insert(request, send);
    let result = with_activity(|env, activity| {
        if let Some(flag) = flag {
            env.call_method(activity, method, "(JZ)V", &[JValue::Long(request), JValue::Bool(jboolean::from(flag))])?;
        } else {
            env.call_method(activity, method, "(J)V", &[JValue::Long(request)])?;
        }
        Ok(())
    }).and_then(|()| receive.recv_timeout(timeout).map_err(Into::into));
    REQUESTS.lock().unwrap().remove(&request);
    result
}

pub fn share_text(text: &str) {
    let _ = with_activity(|env, activity| {
        let text = env.new_string(text)?;
        env.call_method(activity, "shareText", "(Ljava/lang/String;)V", &[JValue::Object(&text)])?;
        Ok(())
    });
}

pub fn clear_sign_in() {
    let _ = with_activity(|env, activity| env.call_method(activity, "clearSignIn", "()V", &[]).map(|_| ()));
}

pub fn pick_uploads() -> Option<Vec<PathBuf>> {
    let result = request("pickAudio", None, Duration::from_secs(600)).ok()??;
    serde_json::from_str(&result).ok()
}

pub fn vault_get(key: &str) -> anyhow::Result<Option<Vec<u8>>> {
    with_activity(|env, activity| {
        let key = env.new_string(key)?;
        let result = env.call_method(activity, "vaultGet", "(Ljava/lang/String;)[B", &[JValue::Object(&key)])?.l()?;
        if result.is_null() {
            return Ok(None);
        }
        env.convert_byte_array(JByteArray::from(result)).map(Some)
    })
}

pub fn vault_set(key: &str, secret: &[u8]) -> anyhow::Result<()> {
    let saved = with_activity(|env, activity| {
        let key = env.new_string(key)?;
        let secret = env.byte_array_from_slice(secret)?;
        env.call_method(activity, "vaultSet", "(Ljava/lang/String;[B)Z", &[JValue::Object(&key), JValue::Object(&secret)])?.z()
    })?;
    anyhow::ensure!(saved, "could not save the Android credential");
    Ok(())
}

pub fn vault_delete(key: &str) -> anyhow::Result<()> {
    let deleted = with_activity(|env, activity| {
        let key = env.new_string(key)?;
        env.call_method(activity, "vaultDelete", "(Ljava/lang/String;)Z", &[JValue::Object(&key)])?.z()
    })?;
    anyhow::ensure!(deleted, "could not delete the Android credential");
    Ok(())
}

#[unsafe(no_mangle)]
extern "system" fn Java_org_ytmrs_android_PlayerActivity_requestFinished(
    mut env: JNIEnv<'_>, _: JObject<'_>, request: jlong, value: JString<'_>,
) {
    let send = REQUESTS.lock().unwrap().remove(&request);
    if let Some(send) = send {
        let value = if value.is_null() { None } else { env.get_string(&value).ok().map(Into::into) };
        let _ = send.send(value);
    }
}

#[unsafe(no_mangle)]
extern "system" fn Java_org_ytmrs_android_PlayerActivity_jamLink(
    mut env: JNIEnv<'_>, _: JObject<'_>, link: JString<'_>,
) {
    if let Ok(link) = env.get_string(&link) {
        let link: String = link.into();
        if let Some(code) = crate::deeplink::parse(&link) {
            crate::deeplink::push(code);
            wake();
        }
    }
}

pub mod media;

pub fn set_event_loop_wake(wake: Option<EventLoopWake>) {
    *EVENT_LOOP_WAKE.lock().unwrap() = wake;
}

pub fn wake() {
    let wake = EVENT_LOOP_WAKE.lock().unwrap().clone();
    if let Some(wake) = wake {
        wake();
    }
}

pub fn set_search_backend(backend: crate::backend::Backend) {
    *SEARCH_BACKEND.lock().unwrap() = Some(backend);
}

#[unsafe(no_mangle)]
extern "system" fn Java_org_ytmrs_android_PlayerActivity_findSuggestions(
    mut env: JNIEnv<'_>, _: JObject<'_>, revision: jlong, query: JString<'_>,
) {
    let Ok(query) = env.get_string(&query) else { return };
    let query: String = query.into();
    let backend = SEARCH_BACKEND.lock().unwrap().clone();
    let Some(backend) = backend else { return };
    let api = backend.api();
    backend.spawn(async move {
        let items = crate::ytm::suggestions(&api, &query, api.account().is_some()).await.unwrap_or_default();
        let items = items.into_iter().take(7).map(|item| item.text).collect::<Vec<_>>();
        let Ok(json) = serde_json::to_string(&items) else { return };
        let _ = with_activity(|env, activity| {
            let result = env.new_string(&json)?;
            env.call_method(activity, "showSuggestions", "(JLjava/lang/String;)V",
                &[JValue::Long(revision), JValue::Object(&result)])?;
            Ok(())
        });
    });
}

pub fn export_audio(path: &Path) {
    let _ = with_activity(|env, activity| {
        let path = env.new_string(path.to_string_lossy())?;
        env.call_method(activity, "exportAudio", "(Ljava/lang/String;)V", &[JValue::Object(&path)])?;
        Ok(())
    });
}

#[unsafe(no_mangle)]
extern "system" fn Java_org_ytmrs_android_PlayerActivity_sendBackToPlayer(
    _: JNIEnv<'_>, _: JObject<'_>,
) -> jboolean {
    let wake = EVENT_LOOP_WAKE.lock().unwrap().clone();
    if let Some(wake) = wake {
        back_request();
        wake();
        return 1;
    }
    0
}

pub fn back_request() {
    BACK_REQUESTS.fetch_add(1, Ordering::Relaxed);
}

pub fn take_back_requests() -> usize {
    BACK_REQUESTS.swap(0, Ordering::Relaxed)
}

pub fn background_from_back() {
    let _ = with_activity(|env, activity|
        env.call_method(activity, "backgroundFromBack", "()V", &[]).map(|_| ()));
}

#[unsafe(no_mangle)]
extern "system" fn Java_org_ytmrs_android_PlayerActivity_safeAreaChanged(
    _: JNIEnv<'_>, _: JObject<'_>, left: jint, top: jint, right: jint, bottom: jint,
) {
    let insets = [left, top, right, bottom].map(|px| px.max(0));
    let changed = {
        let mut current = SAFE_AREA_PX.lock().unwrap();
        std::mem::replace(&mut *current, insets) != insets
    };
    if changed {
        wake();
    }
}

pub fn apply_safe_area(ctx: &egui::Context, raw: &mut egui::RawInput) {
    let Some(native_scale) = raw.viewports.get(&raw.viewport_id).and_then(|info| info.native_pixels_per_point)
    else {
        return; // There is no surface during a background playback pass.
    };
    let scale = native_scale * ctx.zoom_factor();
    if scale.is_finite() && scale > 0.0 {
        let [left, top, right, bottom] = SAFE_AREA_PX.lock().unwrap().map(|px| px as f32 / scale);
        raw.safe_area_insets = Some(egui::SafeAreaInsets(egui::epaint::MarginF32 { left, top, right, bottom }));
    }
}
