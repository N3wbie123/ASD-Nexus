fn main() {
    #[cfg(windows)]
    {
        let sdk_um = r"C:\Program Files (x86)\Windows Kits\10\Lib\10.0.26100.0\um\x64";
        let sdk_ucrt = r"C:\Program Files (x86)\Windows Kits\10\Lib\10.0.26100.0\ucrt\x64";
        let msvc_lib = r"C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC\14.44.35207\lib\x64";

        if std::path::Path::new(sdk_um).exists() {
            println!("cargo:rustc-link-search=native={sdk_um}");
        }
        if std::path::Path::new(sdk_ucrt).exists() {
            println!("cargo:rustc-link-search=native={sdk_ucrt}");
        }
        if std::path::Path::new(msvc_lib).exists() {
            println!("cargo:rustc-link-search=native={msvc_lib}");
        }
    }
}
