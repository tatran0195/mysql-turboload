fn main() {
    println!("cargo:rerun-if-changed=icon.ico");

    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("icon.ico");
        if let Err(e) = res.compile() {
            eprintln!(
                "cargo:warning=Failed to compile Windows resource icon: {}",
                e
            );
        }
    }
}
