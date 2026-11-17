#[cfg(target_os = "android")]
mod imp {
    use jni::objects::{JObject, JString, JValue};
    use jni::JavaVM;
    use std::path::PathBuf;

    fn with_context<F: FnOnce(&mut jni::JNIEnv, &JObject)>(f: F) {
        let ctx = ndk_context::android_context();
        let Ok(vm) = (unsafe { JavaVM::from_raw(ctx.vm() as *mut _) }) else {
            return;
        };
        let Ok(mut env) = vm.attach_current_thread() else {
            return;
        };
        let context = unsafe { JObject::from_raw(ctx.context() as jni::sys::jobject) };
        f(&mut env, &context);
    }

    fn sdk_int(env: &mut jni::JNIEnv) -> i32 {
        env.get_static_field("android/os/Build$VERSION", "SDK_INT", "I")
            .and_then(|v| v.i())
            .unwrap_or(33)
    }

    pub fn request_media_permission() {
        with_context(|env, context| {
            let sdk = sdk_int(env);
            let perms: &[&str] = if sdk >= 33 {
                &[
                    "android.permission.READ_MEDIA_AUDIO",
                    "android.permission.POST_NOTIFICATIONS",
                ]
            } else {
                &["android.permission.READ_EXTERNAL_STORAGE"]
            };
            for perm in perms {
                let Ok(name) = env.new_string(perm) else { continue };
                let granted = env
                    .call_method(
                        context,
                        "checkSelfPermission",
                        "(Ljava/lang/String;)I",
                        &[JValue::Object(&name)],
                    )
                    .and_then(|v| v.i())
                    .unwrap_or(-1);
                if granted == 0 {
                    continue;
                }
                let Ok(arr) = env.new_object_array(1, "java/lang/String", JObject::null())
                else {
                    continue;
                };
                let _ = env.set_object_array_element(&arr, 0, &name);
                let _ = env.call_method(
                    context,
                    "requestPermissions",
                    "([Ljava/lang/String;I)V",
                    &[JValue::Object(&JObject::from(arr)), JValue::Int(1001)],
                );
            }
        });
    }

    pub fn external_files_dir() -> Option<PathBuf> {
        let mut result = None;
        with_context(|env, context| {
            let Ok(obj) = env.call_method(
                context,
                "getExternalFilesDir",
                "(Ljava/lang/String;)Ljava/io/File;",
                &[JValue::Object(&JObject::null())],
            ) else {
                return;
            };
            let Ok(file) = obj.l() else { return };
            let Ok(path) = env
                .call_method(&file, "getAbsolutePath", "()Ljava/lang/String;", &[])
                .and_then(|v| v.l())
            else {
                return;
            };
            let jstr = JString::from(path);
            match env.get_string(&jstr) {
                Ok(s) => {
                    result = Some(PathBuf::from(String::from(s)));
                }
                Err(_) => {}
            };
        });
        result
    }

    pub fn open_url(url: &str) {
        with_context(|env, context| {
            let Ok(u) = env.new_string(url) else { return };
            let Ok(uri) = env.call_static_method(
                "android/net/Uri",
                "parse",
                "(Ljava/lang/String;)Landroid/net/Uri;",
                &[JValue::Object(&u)],
            ) else {
                return;
            };
            let Ok(uri_obj) = uri.l() else { return };
            let Ok(action) = env.new_string("android.intent.action.VIEW") else {
                return;
            };
            let Ok(intent) = env.new_object(
                "android/content/Intent",
                "(Ljava/lang/String;)V",
                &[JValue::Object(&action)],
            ) else {
                return;
            };
            let _ = env.call_method(
                &intent,
                "setData",
                "(Landroid/net/Uri;)Landroid/content/Intent;",
                &[JValue::Object(&uri_obj)],
            );
            let _ = env.call_method(
                &intent,
                "addFlags",
                "(I)Landroid/content/Intent;",
                &[JValue::Int(0x10000000)],
            );
            let _ = env.call_method(
                context,
                "startActivity",
                "(Landroid/content/Intent;)V",
                &[JValue::Object(&intent)],
            );
        });
    }

    pub fn keep_screen_on(on: bool) {
        with_context(|env, context| {
            if let Ok(window) = env
                .call_method(context, "getWindow", "()Landroid/view/Window;", &[])
                .and_then(|v| v.l())
            {
                if on {
                    let _ = env.call_method(
                        &window,
                        "addFlags",
                        "(I)V",
                        &[JValue::Int(0x00000080)],
                    );
                } else {
                    let _ = env.call_method(
                        &window,
                        "clearFlags",
                        "(I)V",
                        &[JValue::Int(0x00000080)],
                    );
                }
            }
        });
    }
}

#[cfg(not(target_os = "android"))]
mod imp {
    use std::path::PathBuf;
    pub fn request_media_permission() {}
    pub fn external_files_dir() -> Option<PathBuf> {
        None
    }
    pub fn open_url(url: &str) {
        log::info!("open url: {url}");
    }
    pub fn keep_screen_on(_on: bool) {}
}

pub use imp::*;
