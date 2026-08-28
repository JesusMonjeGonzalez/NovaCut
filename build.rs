fn main() {
    // Incrusta el icono y los metadatos de versión solo en builds Windows.
    // Si falta la herramienta (p. ej. windres en compilación cruzada), se
    // avisa y se continúa sin icono en el ejecutable.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        if let Err(error) = winresource::WindowsResource::new()
            .set_icon("assets/icon.ico")
            .set("FileDescription", "NovaCut - editor de vídeo nativo")
            .set("ProductName", "NovaCut")
            .compile()
        {
            eprintln!("aviso: no se pudo incrustar el icono: {error}");
        }
    }
}
