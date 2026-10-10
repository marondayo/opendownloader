//! Embeds the icon and version info into the Windows executable.

fn main() {
    println!("cargo:rerun-if-changed=../../packaging/icons/opendownloader.ico");
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../packaging/icons/opendownloader.ico")
            .set("ProductName", "OpenDownloader")
            .set("FileDescription", "OpenDownloader")
            .set("LegalCopyright", "GPL-3.0-or-later");
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed the Windows icon: {e}");
        }
    }
}
