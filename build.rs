use std::{env, fs, path::PathBuf, process::Command};
fn main() {
    println!("cargo:rerun-if-changed=resources/orbit.rc");
    println!("cargo:rerun-if-changed=resources/app.manifest");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let kits = PathBuf::from(env::var("ProgramFiles(x86)").expect("Windows SDK location"))
        .join("Windows Kits/10/bin");
    let mut versions: Vec<_> = fs::read_dir(&kits)
        .expect("Windows SDK")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.join("x64/rc.exe").exists())
        .collect();
    versions.sort();
    let rc = versions
        .last()
        .expect("Windows resource compiler")
        .join("x64/rc.exe");
    let output = PathBuf::from(env::var("OUT_DIR").unwrap()).join("orbit.res");
    assert!(
        Command::new(rc)
            .current_dir("resources")
            .arg("/nologo")
            .arg("/fo")
            .arg(&output)
            .arg("orbit.rc")
            .status()
            .expect("Resource compiler")
            .success()
    );
    println!("cargo:rustc-link-arg={}", output.display());
}
