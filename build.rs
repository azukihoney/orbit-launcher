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
    // Cargo is the only version source; keep generated definitions out of the tree.
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let version = env::var("CARGO_PKG_VERSION").unwrap();
    let numbers: Vec<u16> = ["MAJOR", "MINOR", "PATCH"]
        .iter()
        .map(|part| {
            env::var(format!("CARGO_PKG_VERSION_{part}"))
                .unwrap()
                .parse()
                .expect("Windows version component must fit in 16 bits")
        })
        .collect();
    fs::write(
        out_dir.join("orbit-version.h"),
        format!(
            "#define ORBIT_VERSION \"{version}\\0\"\n#define ORBIT_VERSION_NUM {},{},{},0\n",
            numbers[0], numbers[1], numbers[2]
        ),
    )
    .expect("Write Cargo version definitions");
    let output = out_dir.join("orbit.res");
    assert!(
        Command::new(rc)
            .current_dir("resources")
            .arg("/nologo")
            .arg("/I")
            .arg(&out_dir)
            .arg("/fo")
            .arg(&output)
            .arg("orbit.rc")
            .status()
            .expect("Resource compiler")
            .success()
    );
    println!("cargo:rustc-link-arg={}", output.display());
}
