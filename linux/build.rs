//! Bake app.contract into OUT_DIR/app.plan and write the Linux entry.

use contract::DataSource;

const DATA: &str = "aula75_data::Aula";
const NEW: &str = "aula75_data::Aula::default()";

fn main() {
    contract::rerun_if_changed(std::path::Path::new("../app.contract"));
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../app.json");
    println!("cargo:rerun-if-changed=../data");
    let plan = contract::compile_path(std::path::Path::new("../app.contract")).unwrap_or_else(|e| panic!("app.contract:{e}"));
    let entries = contract::snapshot::entries(std::path::Path::new("..")).unwrap();
    let baked = contract::bake(plan, aula75_data::Aula::default()).unwrap_or_else(|e| panic!("bake: {e:?}"));
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    std::fs::write(out_dir.join("app.plan"), baked.encode()).unwrap();
    let app_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let target = std::env::var("TARGET").unwrap_or_default();
    let manifest = contract::Manifest::read(&app_dir).unwrap_or_else(|e| panic!("app.json: {e}"));
    let grants = aula75_data::Aula::default().grants().to_string();
    let compat = exact_bake::compatibility_id_sources_with_entries(&app_dir, "linux", &target, &manifest, Some(&grants), None, Some(&entries))
        .unwrap_or_else(|e| panic!("compatibility id: {e}"));
    std::fs::write(out_dir.join("compat.json"), compat.to_json()).unwrap();
    let host = if compat.inputs["store"]["L"] == "0" { "exact_linux" } else { "exact_linux_update" };
    let (hatches, run) = contract::native::rust_hatch_entry(&app_dir, &manifest, &target).unwrap_or_else(|e| panic!("hatches: {e}"));
    std::fs::write(
        out_dir.join("entry.rs"),
        contract::linux_entry(DATA, NEW, compat.inputs["rustMode"].as_str().unwrap(), host, &hatches, run)
            .unwrap_or_else(|e| panic!("entry: {e}")),
    )
    .unwrap();
}
