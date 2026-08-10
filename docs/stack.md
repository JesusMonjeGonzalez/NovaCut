# Editorcito: arquitectura multiplataforma

La carpeta del repositorio conserva el nombre histórico `NovaCut`, pero el producto y el crate se llaman **Editorcito**.

## Baseline compartido

Editorcito separa el modelo y la planificación de medios de las APIs nativas de cada sistema operativo.

| Capa | Responsabilidad |
|---|---|
| Rust core | Proyecto JSON, timeline, cache global de frames, grafo de render y descripción de exportación |
| Adaptador macOS | Decode/encode con AVFoundation y VideoToolbox; render y presentación con Metal |
| Adaptador Windows | Decode/encode con Media Foundation; render y presentación con Direct3D 12 |
| UI nativa | SwiftUI/AppKit en macOS; una UI nativa de Windows por definir |

El core no afirma realizar decode o encode de producción. `decode.rs` es hoy un baseline de metadatos/frames para integrar backends, y `encoder.rs` solo declara una solicitud validada de exportación. Las implementaciones de AVFoundation, Media Foundation, Metal y Direct3D 12 pertenecen a adaptadores de plataforma.

## Artefactos Rust

`Cargo.toml` apunta explícitamente a `src/core/lib.rs` y puede producir:

- `rlib`, para consumo desde Rust.
- `staticlib`, para enlace estático desde hosts nativos.
- `cdylib`, para integración dinámica cuando la aplicación lo requiera.

El baseline Rust usa únicamente `serde`, `serde_json` y `parking_lot`. No enlaza todavía FFmpeg ni SDKs multimedia de plataforma.

## Estructura relevante

```text
NovaCut/                       # nombre histórico de la carpeta
├── Cargo.toml
├── docs/
│   └── stack.md
└── src/
    ├── core/
    │   ├── lib.rs
    │   ├── decode.rs
    │   ├── encoder.rs         # API declarativa; no codifica medios
    │   ├── frame_buffer.rs    # cache RAM con límite global
    │   ├── timeline.rs
    │   ├── project.rs
    │   └── render_graph.rs
    └── ui/                    # host macOS existente
```

## Flujo por plataforma

### macOS

1. AVFoundation/VideoToolbox obtiene superficies de video y audio.
2. El host traduce tiempo, assets y operaciones al modelo Rust.
3. Metal procesa el grafo y presenta los frames.
4. AVFoundation/VideoToolbox materializa una descripción de exportación del core.

### Windows

1. Media Foundation obtiene superficies de video y audio.
2. El host usa la misma API y serialización del core Rust.
3. Direct3D 12 procesa el grafo y presenta los frames.
4. Media Foundation materializa una descripción de exportación del core.

## Límites actuales

- El cache implementado es RAM; no existe aún cache persistente en disco.
- El grafo representa conexiones y parámetros, pero no ejecuta shaders.
- Los backends multimedia y la capa FFI estable aún deben implementarse.
- Los objetivos 4K/8K y reproducción en tiempo real requieren mediciones después de integrar los backends nativos.
