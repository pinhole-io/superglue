//! Initialize `rustls-platform-verifier` before the first HTTPS call.
//!
//! reqwest on Android verifies certificates through that crate. The crate
//! panics if `android::init_with_env` has not run. The panic aborts the
//! process because it crosses the JNI boundary.

use std::sync::atomic::{AtomicBool, Ordering};

use jni::JNIEnv;
use jni::objects::JObject;
use jni::objects::JValue;
use jni22::EnvUnowned;
use jni22::Outcome;

static READY: AtomicBool = AtomicBool::new(false);

/// Set up the Android certificate verifier once a process `Context` exists.
///
/// A missing `Context` leaves the flag clear so a later JNI call can retry.
pub fn ensure_initialized(env: &mut JNIEnv) {
    if let Ok(app) = application(env) {
        if !app.is_null() {
            // Native threads do not inherit the VPN network. Bind the process
            // so DNS and TLS use the same network as the rest of the app.
            log_bind(env, &app);
            if !READY.load(Ordering::Acquire) && init_verifier(env.get_raw(), app.as_raw()).is_ok()
            {
                READY.store(true, Ordering::Release);
            }
        }
    }
    // A failed lookup can leave a pending exception. The JNI call must return clean.
    let _ = env.exception_clear();
}

fn log_bind(env: &mut JNIEnv, app: &JObject) {
    let message = match bind_process_network(env, app) {
        Ok(true) => "bind-network ok".to_string(),
        Ok(false) => "bind-network returned false".to_string(),
        Err(error) => format!("bind-network error: {error}"),
    };
    android_log_line(&message);
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
}

fn android_log_line(msg: &str) {
    #[link(name = "log")]
    unsafe extern "C" {
        fn __android_log_write(prio: i32, tag: *const u8, text: *const u8) -> i32;
    }
    let tag = b"superglue\0";
    let mut bytes = msg.replace('\0', " ").into_bytes();
    bytes.push(0);
    unsafe {
        __android_log_write(6, tag.as_ptr(), bytes.as_ptr());
    }
}

fn bind_process_network(env: &mut JNIEnv, app: &JObject) -> Result<bool, jni::errors::Error> {
    let name = env.new_string("connectivity")?;
    let manager = env
        .call_method(
            app,
            "getSystemService",
            "(Ljava/lang/String;)Ljava/lang/Object;",
            &[JValue::Object(&name)],
        )?
        .l()?;
    if manager.is_null() {
        return Ok(false);
    }
    let network = env
        .call_method(
            &manager,
            "getActiveNetwork",
            "()Landroid/net/Network;",
            &[],
        )?
        .l()?;
    if network.is_null() {
        return Ok(false);
    }
    env.call_method(
        &manager,
        "bindProcessToNetwork",
        "(Landroid/net/Network;)Z",
        &[JValue::Object(&network)],
    )?
    .z()
}

fn application<'local>(env: &mut JNIEnv<'local>) -> Result<JObject<'local>, jni::errors::Error> {
    let class = env.find_class("android/app/ActivityThread")?;
    env.call_static_method(
        class,
        "currentApplication",
        "()Landroid/app/Application;",
        &[],
    )?
    .l()
}

fn init_verifier(raw_env: *mut jni::sys::JNIEnv, raw_app: jni::sys::jobject) -> Result<(), ()> {
    // jni 0.21 and jni 0.22 each pin a different `jni-sys`. The pointers are
    // the same JNI values. The cast crosses that crate boundary.
    let raw_env = raw_env.cast::<std::ffi::c_void>().cast();
    let raw_app = raw_app.cast::<std::ffi::c_void>().cast();
    // Safety: `raw_env` is the JNIEnv for this native call on this thread.
    // `raw_app` is a local ref in that same frame. `init_with_env` copies it
    // into a global ref. Neither wrapper deletes the local ref.
    let mut unowned = unsafe { EnvUnowned::from_raw(raw_env) };
    let outcome = unowned
        .with_env(|env| {
            let context = unsafe { jni22::objects::JObject::from_raw(env, raw_app) };
            rustls_platform_verifier::android::init_with_env(env, context)
        })
        .into_outcome();
    match outcome {
        Outcome::Ok(()) => Ok(()),
        _ => Err(()),
    }
}
