# Editorcito: DAFO competitivo

Estado y evidencia revisados el 4 de agosto de 2026.

## Referencia de mercado

El benchmark principal es **DaVinci Resolve 21**: edición multipista, color,
Fusion, Fairlight, entrega, IA y producto multiplataforma. Adobe Premiere es el
benchmark de ecosistema, búsqueda semántica y colaboración. Final Cut Pro es la
referencia secundaria de UX y rendimiento en Apple Silicon.

Editorcito no es hoy una alternativa completa a esos productos. Es un alpha macOS
con un flujo de producción funcional para entrevistas, podcasts y contenido social.
La posición a validar es:

> Editor local y ligero para convertir entrevistas, podcasts y grabaciones en
> vídeos terminados, con IA revisable y el mismo proyecto en Mac y Windows.

Fuentes oficiales:

- [DaVinci Resolve](https://www.blackmagicdesign.com/products/davinciresolve)
- [Resolve Edit](https://www.blackmagicdesign.com/products/davinciresolve/edit)
- [Resolve Fairlight](https://www.blackmagicdesign.com/products/davinciresolve/fairlight)
- [Premiere: mejoras 2026](https://blog.adobe.com/en/publish/2026/01/20/new-ai-powered-video-editing-tools-premiere-major-motion-design-upgrades-after-effects)
- [Final Cut Pro](https://www.apple.com/final-cut-pro/)
- [Final Cut Pro: notas](https://support.apple.com/en-us/102825)

## Comparativa honesta

| Capacidad | Editorcito | Resolve/Premiere | Brecha |
|---|---|---|---|
| Timeline | Multipista, trim, split, reorder, ripple/roll/slip/slide | Multipista profesional | P1 |
| Tiempo | Timebase racional, drop-frame y VFR detectado | PTS/VFR probado en corpus amplio | P0 |
| Playback | AVFoundation, proxies, thumbnails y waveforms | Cache, GPU y codecs amplios con benchmark | P0 |
| Audio | Ganancia, fades, crossfades y ducking básico | Mezcla, buses, plugins y automatización | P1 |
| Color/VFX | No | Referencia de mercado | P2 |
| IA | Edición estructurada, transcripción on-device y SRT | Imagen, voz, texto y semántica avanzada | P1 |
| Persistencia | JSON, backup, autosave, bins y offline/relink | Proyectos maduros y colaboración | P0 |
| Plataformas | macOS funcional; Windows diseñado | Mac/Windows terminados | P0 |

## DAFO

### Fortalezas

- Flujo vertical claro: importar, cortar, ordenar, reproducir, guardar y exportar.
- UI macOS nativa y AVFoundation reducen latencia e integracion inicial.
- Undo/redo transaccional, backup y recuperacion automatica.
- Acciones IA estructuradas, cancelables y rechazadas si el timeline cambia.
- Selector local/OpenCode Go sin mostrar credenciales.
- Source monitor, I/O, proxies, waveforms, thumbnails y exportación por presets.
- Proyecto local y privado, con UX en español y edición A/V enlazada.
- Nucleo Rust preparado para contrato compartido Mac/Windows.

### Debilidades

- El host Swift aún no usa el núcleo Rust como fuente de verdad.
- VFR está detectado, pero aún no está validado con PTS y sincronización profesional.
- Mixer sin EQ, compresor, limiter, paneo ni medición LUFS.
- Multicámara sin visor de ángulos ni cambio en tiempo real.
- Proxies sin cancelación, limpieza automática ni límite de espacio.
- Sin corpus legal de codecs, benchmarks E2E ni usuarios externos suficientes.
- Color, títulos avanzados, máscaras, tracking y estabilización aún son limitados.

### Oportunidades

- Flujo privado y simple para entrevistas, podcast y contenido social.
- Detección de silencios y rough cut reversibles pueden ahorrar tiempo medible.
- Proyecto Rust comun con adaptadores multimedia nativos por plataforma.
- UX guiada en español, sin la complejidad completa de Resolve.

### Amenazas

- Resolve ofrece gratuitamente una capacidad varios ordenes mayor.
- Premiere y Final Cut ya aplican IA sobre imagen, audio y texto.
- Dos hosts nativos mas Rust pueden dispersar un equipo pequeño.
- Prometer 4K, Windows o privacidad sin medicion dañaria la confianza.
- Compatibilidad de codecs y sincronizacion acumulan decadas de casos limite.

## Prioridades derivadas

### Hecho en el host macOS

1. Timebase racional, drop-frame, multipista y enlace A/V.
2. Source monitor, I/O, insert, overwrite, ripple/roll, snapping y drag entre pistas.
3. Waveforms, thumbnails, fades, crossfades, keyframes y transiciones.
4. Proxies de preview, bins, búsqueda y presets de exportación con cola.
5. Transcripción on-device, captions SRT y captions quemados.

### P0: condición para NLE beta

1. Validar CFR/VFR, PTS y sincronización A/V con corpus legal.
2. Formato de proyecto migrable y portable con media offline/relink probado.
3. Orientaciones, resoluciones y fps heterogéneos correctos en todos los presets.
4. Corpus golden y benchmark reproducible en Mac antes de afirmar compatibilidad.
5. Sesiones largas sin pérdida, crash ni corrupción de autosave.
6. IA con diff previo, confirmación y transcripción local repetible.

### P1: producto útil

- EQ, compresor, limiter, paneo y loudness.
- Visor multicámara, edición por ángulos y sincronización más robusta.
- Detección de silencios, búsqueda por transcript y rough cut reversible.
- Color scopes, LUTs, títulos, máscaras, tracking y estabilización GPU.
- Proxies cancelables, limpieza de caché y cola de exportación recuperable.
- Primer host Windows real con Media Foundation y GPU nativa.

### P2: diferenciación posterior

- Búsqueda semántica visual/audio y rough cut avanzado.
- Color, HDR, mascaras, tracking, plugins y colaboracion.

## Gates

| Periodo | Gate obligatorio |
|---|---|
| Mes 0-2 | 100% de proyectos golden reabren identicos; 3 usuarios terminan un proyecto real |
| Mes 2-5 | A/V <=1 frame; 10.000 seeks con >=99,9% de acierto; 25 exportaciones sin truncado |
| Mes 5-8 | 10 proyectos de 5-30 min; 4K30 con <=0,1% frames perdidos; sesion de 2 h sin crash |
| Mes 8-11 | Mismo proyecto alterna 20 veces Mac/Windows; >=95% del corpus pasa en ambos |
| Mes 10-14 | IA local sin trafico; 100% muestra diff; >=95% exactitud en comandos soportados |
| Mes 14-18 | >=99,5% sesiones sin crash; 50 beta testers; cero perdidas no recuperables |

No se persigue paridad total con Resolve antes de cerrar P0 y demostrar repeticion.
