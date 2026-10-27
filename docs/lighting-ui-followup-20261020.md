# Lighting and responsive UI follow-up — 2026-10-20

## Capability and selected approach

The local GPU is Apple M4 Max with 32 GPU cores and Metal support (read-only SPDisplaysDataType audit). Apple documents hardware-accelerated ray tracing: https://www.apple.com/newsroom/2024/10/apple-introduces-m4-pro-and-m4-max/ . The current renderer uses glow/OpenGL, GLSL 150 and WebGL2. It has no Metal acceleration-structure or ray-tracing backend; hardware support alone does not enable ray tracing here.

Existing features include GGX materials, analytic environment reflections, raster sun shadows, and bounded local task lights. Sun-shadow settings retain Low/off, Balanced/1024 and High/2048 maps; camera feeds use a smaller budget. No SSAO, screen-space reflection, per-local-light shadow map or local wall occlusion is implemented. Authored emitting ceiling fixtures have no circuit binding; this patch inherits docked behavior rather than inventing electrical authority.

Version 18 fixes a concrete parity bug: the flight renderer now collects the same canonical emitting fixture geometry as the docked renderer. It preserves body transforms, metre conversion, freshness and existing 2/4/8-light quality budgets (feeds capped at two). It adds no new emitters, bloom or backend dependency.

## Measurement and visual limits

Focused test passed at `.build/logs/20261020T212949875122Z.json`. Median CPU collector timing: baseline 0.844 us/call; canonical 9.927 us/call with 629 parts and eight lights. This is not GPU or frame-rate evidence.

The actual version 18 Render debug panel reported a completed asynchronous main GPU pass of 7.793 ms, CPU selection 1.254 ms and CPU submission 7.834 ms while observing Cockpit in Editor on the paused user world. This is one pass sample with current UI viewport and excludes a whole-frame performance claim. A retained docked sample of 4.558 ms is from a different view and must not be compared as a lighting delta. No timed screenshot readback is included in these GPU query results. There is no matched before/after GPU benchmark yet.

Historical 1500x960 cockpit/mining and corridor images were reviewed. They show clear forward composition but weak contact shadows and flat corridor ambient illumination. They predate version 18 and do not certify current appearance. Current exact-window CUA Raise still returns a 320-pixel Stage Manager thumbnail; full-size actual desktop acceptance remains open. No Stage Manager/system settings were changed.

## Concrete remaining work

Light selection ties all overlapping ranges at zero, making authored order win over proximity. A proposed nearest-distance tie-break was denied before execution by automatic review retaining the original diagnostic-only restriction. It was not retried or applied through another route. Existing fixture parity remains intact.

The next larger lighting gain is coherent interior environment response and bounded contact/local occlusion shared across views, after paired full-size captures and asynchronous GPU measurements. Globally increasing exposure will not fix the missing contact shadows. Do not advertise ray tracing or a measured GPU improvement.

## Responsive UI checkpoint

The focused management scroll body no longer forces a 120-pixel minimum when less space remains. Four authoritative telemetry/inventory grids now have finite column widths so long values wrap inside narrowed workspaces. No command, simulation, player embodiment or job state changes are part of this checkpoint.
