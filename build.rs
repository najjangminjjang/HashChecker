fn main() {
    println!("cargo:rerun-if-changed=src/hash-checker.ico");
    println!("cargo:rerun-if-changed=app.rc");
    #[cfg(windows)]
    {
        std::fs::write("app.rc", "1 ICON \"src/hash-checker.ico\"\r\n").unwrap();
        embed_resource::compile("app.rc", embed_resource::NONE);
    }
}
