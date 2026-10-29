fn main() {
    tauri_build::build();
    mobile_assets();
}

fn mobile_assets() {
    use std::{
        env, fs,
        path::{Path, PathBuf},
    };
    fn visit(root: &Path, dir: &Path, prefix: &str, out: &mut String) {
        if !dir.is_dir() {
            return;
        }
        let mut entries: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                visit(root, &path, prefix, out)
            } else {
                let relative = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                let route = if relative == "index.html" {
                    "/".into()
                } else {
                    format!("{prefix}/{relative}")
                };
                let mime = match path.extension().and_then(|s| s.to_str()).unwrap_or("") {
                    "html" => "text/html; charset=utf-8",
                    "js" => "text/javascript; charset=utf-8",
                    "css" => "text/css; charset=utf-8",
                    "webp" => "image/webp",
                    "svg" => "image/svg+xml",
                    _ => continue,
                };
                out.push_str(&format!(
                    "{route:?} => Some((include_bytes!({:?}), {mime:?})),\n",
                    path.to_string_lossy()
                ));
            }
        }
    }
    let project = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .parent()
        .unwrap()
        .to_path_buf();
    let mobile = project.join("packages/mobile/dist");
    let images = project.join("packages/gui/public/valorant");
    println!("cargo:rerun-if-changed={}", mobile.display());
    println!("cargo:rerun-if-changed={}", images.display());
    let mut out =
        "pub fn asset(path: &str) -> Option<(&'static [u8], &'static str)> { match path {\n"
            .to_string();
    if !mobile.join("index.html").is_file() {
        assert_ne!(
            env::var("PROFILE").as_deref(),
            Ok("release"),
            "Build the mobile frontend before creating a release: bun run build:mobile"
        );
        out.push_str("\"/\" => Some((\"手机访问开发页面尚未构建\".as_bytes(), \"text/html; charset=utf-8\")),\n");
    } else {
        visit(&mobile, &mobile, "", &mut out);
    }
    visit(&images, &images, "/valorant", &mut out);
    out.push_str("_ => None } }\n");
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("mobile_assets.rs"),
        out,
    )
    .unwrap();
}
