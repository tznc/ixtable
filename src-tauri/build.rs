#[path = "src/release_keys.rs"]
mod release_keys;

/// Beta/stable builds (`IXTABLE_RELEASE=1`, set by release.yml) fail closed on missing or dev
/// keys. Without the opt-in, `tauri build` is a development build.
fn check_release_keys() {
    for var in [
        "IXTABLE_RELEASE",
        "IXTABLE_CLOUD_PUBLIC_KEY_RAW",
        "IXTABLE_CLOUD_PUBLIC_KEY",
    ] {
        println!("cargo:rerun-if-env-changed={var}");
    }
    if std::env::var("IXTABLE_RELEASE").as_deref() != Ok("1") {
        return;
    }
    println!("cargo:rerun-if-changed=../scripts/release/dev-cloud-keys.json");
    let read = |path: &str| {
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("release build: {path}: {e}"))
    };
    let raw = std::env::var("IXTABLE_CLOUD_PUBLIC_KEY_RAW").ok();
    let spki = std::env::var("IXTABLE_CLOUD_PUBLIC_KEY").ok();
    let problems = release_keys::release_key_problems(&release_keys::ReleaseKeys {
        cloud_raw: raw.as_deref(),
        cloud_spki: spki.as_deref(),
        dev_cloud_keys_json: &read("../scripts/release/dev-cloud-keys.json"),
    });
    if !problems.is_empty() {
        panic!(
            "IXTABLE_RELEASE=1 build refused: {} (docs/decisions/desktop-updates.md)",
            problems.join("; ")
        );
    }
}

fn main() {
    check_release_keys();
    if std::env::var_os("CARGO_FEATURE_TEST_BRIDGE").is_some() {
        napi_build::setup();
        // The test bridge DLL is loaded by node.exe, which has no Common Controls v6
        // manifest, so comctl32 v5 is bound and its missing TaskDialogIndirect import
        // (tauri-runtime-wry and rfd dialogs) fails the load with error 127. Delay-load
        // comctl32 so the import resolves only if a dialog is ever shown (never in tests).
        if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
            println!("cargo:rustc-link-arg-cdylib=/DELAYLOAD:comctl32.dll");
            println!("cargo:rustc-link-arg-cdylib=delayimp.lib");
        }
    }
    link_prebuilt_duckdb();
    tauri_build::build()
}

// The app links the prebuilt shared libduckdb (DUCKDB_LIB_DIR) and finds it at run time:
// - Linux: deb, rpm and AppImage install resources in ../lib/ixtable next to bin/, and dev
//   builds and the test bridge use the absolute download directory.
// - macOS: the bundle ships it in Contents/Frameworks (tauri.macos.conf.json).
// - Windows: the DLL sits next to the exe, so it is copied into target/<profile> for dev
//   builds and the test bridge (node loads the bridge with its own directory searched).
fn link_prebuilt_duckdb() {
    println!("cargo:rerun-if-env-changed=DUCKDB_LIB_DIR");
    let Some(dir) = std::env::var_os("DUCKDB_LIB_DIR").map(std::path::PathBuf::from) else {
        return;
    };
    match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("linux") => {
            // WebKitGTK references sqlite3_*, so the linker exports rusqlite's bundled SQLite.
            // sqlite_scanner carries its own SQLite and crashes on close when part of it binds
            // to ours, so hide archive symbols. Not in the test bridge: node needs its
            // napi_register_module_v1, which comes from an archive (rlib).
            if std::env::var_os("CARGO_FEATURE_TEST_BRIDGE").is_none() {
                println!("cargo:rustc-link-arg=-Wl,--exclude-libs,ALL");
            }
            println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN/../lib/ixtable");
            println!("cargo:rustc-link-arg=-Wl,-rpath,{}", dir.display());
        }
        Ok("macos") => {
            println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path/../Frameworks");
            println!("cargo:rustc-link-arg=-Wl,-rpath,{}", dir.display());
        }
        Ok("windows") => {
            let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
            // OUT_DIR is target/<profile>/build/<pkg>/out.
            let profile_dir = out.ancestors().nth(3).expect("cargo OUT_DIR layout");
            let dll = dir.join("duckdb.dll");
            println!("cargo:rerun-if-changed={}", dll.display());
            std::fs::copy(&dll, profile_dir.join("duckdb.dll")).expect("copy duckdb.dll");
        }
        _ => {}
    }
}
