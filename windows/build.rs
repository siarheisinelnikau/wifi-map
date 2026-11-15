fn main() {
    println!("cargo:rerun-if-changed=assets/wifi-map.ico");
    println!("cargo:rerun-if-changed=assets/app.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/wifi-map.ico");
        resource.set_manifest_file("assets/app.manifest");
        resource.set("ProductName", "WifiMap");
        resource.set("FileDescription", "WifiMap — Wi-Fi floor-plan surveys");
        resource
            .compile()
            .expect("compile Windows identity resources");
    }
}
