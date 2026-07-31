fn main() {
    // `include_dir!` embeds ../locales at compile time, but Cargo has no way to
    // know that: nothing in src/ references those files as a path Cargo tracks.
    // Without this line, adding or editing a translation would not trigger a
    // rebuild, and the binary would keep serving the strings it was built with
    // — a bug that looks exactly like "my translation is being ignored".
    println!("cargo:rerun-if-changed=../locales");

    tauri_build::build();
}
