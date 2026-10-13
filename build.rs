fn main() {
    println!("cargo:rerun-if-changed=assets/luma.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/luma.ico");
        resource.set("ProductName", "Luma Motion");
        resource.set("FileDescription", "Luma Motion — desktop video studio");
        resource
            .compile()
            .expect("Não foi possível compilar o ícone Windows.");
    }
}
