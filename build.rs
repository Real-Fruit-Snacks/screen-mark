fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=assets/icon.png");
    #[cfg(target_os = "windows")]
    {
        // Use the checked-in multi-size assets/icon.ico (16..256px); fall back
        // to wrapping assets/icon.png only if icon.ico is missing.
        if !std::path::Path::new("assets/icon.ico").exists() {
            let png_data =
                std::fs::read("assets/icon.png").expect("Failed to read assets/icon.png");
            let mut ico = Vec::new();
            ico.extend_from_slice(&[0, 0, 1, 0, 1, 0]);
            ico.extend_from_slice(&[0, 0, 0, 0]);
            ico.extend_from_slice(&[1, 0, 32, 0]);
            ico.extend_from_slice(&(png_data.len() as u32).to_le_bytes());
            ico.extend_from_slice(&22u32.to_le_bytes());
            ico.extend_from_slice(&png_data);
            std::fs::write("assets/icon.ico", &ico).expect("Failed to write assets/icon.ico");
        }

        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "screen-mark");
        res.set("CompanyName", "Real-Fruit-Snacks");
        res.set("FileDescription", "Screenshot capture and annotation tool");
        res.set("LegalCopyright", "Copyright 2026 Real-Fruit-Snacks");
        res.compile().expect("winres resource compile failed");
    }
}
