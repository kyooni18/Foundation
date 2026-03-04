``` CSS
/* =========================================================
 * Misskey Floating UI (Light & Airy) - cleaned
 * - 글래스(팝업/시트) 더 투명 + 블러 유지
 * - 드롭 섀도우 강화
 * - 포커스 링 얇게(2px)
 * - 바텀시트 sqircle
 * - 메뉴 패널 스위치(트랙/썸 정렬) 정리본
 * ========================================================= */

/* ------------------------------
 * 0) Variables
 * ------------------------------ */
:root {
  /* theme fallbacks */
  --mk-accent: var(--MI_THEME-accent, var(--accent, #e36749));
  --mk-bg: var(--MI_THEME-bg, var(--bg, #f5f5f5));
  --mk-panel: var(--MI_THEME-panel, var(--panel, #ffffff));
  --mk-fg: var(--MI_THEME-fg, var(--fg, #333));
  --mk-divider: var(--MI_THEME-divider, var(--divider, rgba(0, 0, 0, 0.10)));

  /* round/gap/blur */
  --mk-radius-sm: 20px;
  --mk-radius-md: 24px;
  --mk-radius-lg: 28px;
  --mk-gap: 12px;
  --mk-blur: blur(9px);
  --MI-blur: var(--mk-blur);

  /* shadows */
  --mk-shadow-sm: 0 6px 18px rgba(0, 0, 0, 0.10), 0 2px 6px rgba(0, 0, 0, 0.06);
  --mk-shadow-md: 0 14px 38px rgba(0, 0, 0, 0.12), 0 4px 12px rgba(0, 0, 0, 0.07);
  --mk-shadow-lg: 0 26px 90px rgba(0, 0, 0, 0.16), 0 10px 28px rgba(0, 0, 0, 0.10);

  /* focus ring */
  --mk-focus: var(--MI_THEME-focus, rgba(227, 103, 73, 0.18));

  /* glass (fallback) */
  --mk-glass: rgba(255, 255, 255, 0.32);
  --mk-glass-strong: rgba(255, 255, 255, 0.48);

  /* bottom sheet sqircle */
  --mk-sq: 36px;

  /* switches (menu panel) */
  --mk-sw-sq: 12px;
  --mk-sw-inset: 2px;
  --mk-sw-track-a: 0.46;
  --mk-sw-track-a-dark: 0.42;
  --mk-sw-glow: color-mix(in srgb, var(--mk-accent) 18%, transparent);
}

/* glass from panel color when supported */
@supports (background: color-mix(in srgb, white, transparent)) {
  :root {
    --mk-glass: color-mix(in srgb, var(--mk-panel) 66%, transparent);
    --mk-glass-strong: color-mix(in srgb, var(--mk-panel) 80%, transparent);
    --mk-focus: var(--MI_THEME-focus, color-mix(in srgb, var(--mk-accent) 22%, transparent));
    --mk-sw-glow: color-mix(in srgb, var(--mk-accent) 18%, transparent);
  }
}

/* reduced motion */
@media (prefers-reduced-motion: reduce) {
  * {
    animation: none !important;
    transition: none !important;
    scroll-behavior: auto !important;
  }
}

/* ------------------------------
 * 1) Base background
 * ------------------------------ */
html, body {
  color: var(--mk-fg) !important;
  background: var(--mk-bg) !important;
  -webkit-font-smoothing: antialiased;
  text-rendering: optimizeLegibility;
}

@supports (background: color-mix(in srgb, white, transparent)) {
  html, body {
    background:
      radial-gradient(900px 450px at 12% -10%, color-mix(in srgb, var(--mk-accent) 9%, transparent), transparent 60%),
      radial-gradient(900px 450px at 88% 0%, color-mix(in srgb, var(--MI_THEME-link, #44a4c1) 7%, transparent), transparent 60%),
      var(--mk-bg) !important;
    background-attachment: fixed !important;
  }
}

/* ------------------------------
 * 2) Timeline cards
 * ------------------------------ */
:where(main) :where(article, div.xcSej) {
  background: var(--mk-panel) !important;
  border: 1px solid var(--mk-divider) !important;
  border-radius: var(--mk-radius-md) !important;
  margin: var(--mk-gap) 0 !important;
  box-shadow: var(--mk-shadow-sm) !important;
  overflow: clip;
  transition: transform 140ms ease, box-shadow 140ms ease, filter 140ms ease;
}
@supports not (overflow: clip) {
  :where(main) :where(article, div.xcSej) { overflow: hidden !important; }
}
:where(main) :where(article, div.xcSej):hover {
  transform: translateY(-1px);
  box-shadow: var(--mk-shadow-md) !important;
}
:where(main) :where(article, div.xcSej):active { transform: translateY(0px); }
:where(main) :where(article, div.xcSej):focus-within {
  box-shadow: 0 0 0 2px var(--mk-focus), var(--mk-shadow-md) !important;
}
:where(main) :where(article, div.xcSej) :where(article, div.xcSej) {
  margin: 0 !important;
  box-shadow: none !important;
  border-radius: var(--mk-radius-sm) !important;
}

/* ------------------------------
 * 3) Popups / acrylic
 * ------------------------------ */
:where(._panel) { border-radius: var(--mk-radius-md) !important; }

:where(._popup, ._acrylic) {
  border-radius: var(--mk-radius-lg) !important;
  border: 1px solid var(--mk-divider) !important;
  background: var(--mk-glass-strong) !important;
  -webkit-backdrop-filter: var(--mk-blur);
  backdrop-filter: var(--mk-blur);
  box-shadow: var(--mk-shadow-lg) !important;
}

:where(._panel.mkw-post-form) {
  border-radius: var(--mk-radius-lg) !important;
  border: 1px solid var(--mk-divider) !important;
  box-shadow: var(--mk-shadow-md) !important;
  overflow: hidden !important;
}

:where(._popup, ._panel.mkw-post-form, ._acrylic) > header {
  background: var(--mk-glass) !important;
  -webkit-backdrop-filter: var(--mk-blur);
  backdrop-filter: var(--mk-blur);
  border-bottom: 1px solid var(--mk-divider) !important;
}

@supports not ((-webkit-backdrop-filter: blur(1px)) or (backdrop-filter: blur(1px))) {
  :where(._popup, ._acrylic) { background: var(--mk-panel) !important; }
}

/* ------------------------------
 * 4) Bottom sheet / menus (glass + more transparent) + sqircle
 * ------------------------------ */
:where(
  ._modal, ._dialog, ._sheet, ._bottomSheet, ._drawer,
  ._menu, ._contextMenu, ._menuPanel, ._popupMenu,
  [role="dialog"], [data-modal="true"]
) {
  background: rgba(255, 255, 255, 0.52) !important;  /* 더 투명 */
  -webkit-backdrop-filter: var(--mk-blur) !important;
  backdrop-filter: var(--mk-blur) !important;
  border: 1px solid rgba(0, 0, 0, 0.05) !important;
  box-shadow: 0 20px 70px rgba(0, 0, 0, 0.16), 0 8px 24px rgba(0, 0, 0, 0.10) !important;

  border-radius: 0 !important;
  overflow: visible !important;

  -webkit-mask-image:
    radial-gradient(var(--mk-sq) at var(--mk-sq) var(--mk-sq), #000 98%, transparent 100%),
    radial-gradient(var(--mk-sq) at calc(100% - var(--mk-sq)) var(--mk-sq), #000 98%, transparent 100%),
    radial-gradient(var(--mk-sq) at var(--mk-sq) calc(100% - var(--mk-sq)), #000 98%, transparent 100%),
    radial-gradient(var(--mk-sq) at calc(100% - var(--mk-sq)) calc(100% - var(--mk-sq)), #000 98%, transparent 100%),
    linear-gradient(#000, #000);
  -webkit-mask-composite: source-over, source-over, source-over, source-over, source-in;
}

/* (선택) 패널 내부 콘텐츠가 모서리에서 삐져나오면만 유지 */
:where(
  ._modal, ._dialog, ._sheet, ._bottomSheet, ._drawer,
  ._menu, ._contextMenu, ._menuPanel, ._popupMenu,
  [role="dialog"], [data-modal="true"]
) *{
  -webkit-mask-image: inherit;
  -webkit-mask-composite: inherit;
}

/* separators */
:where(
  ._modal, ._dialog, ._sheet, ._bottomSheet, ._menu, ._contextMenu, [role="dialog"]
) :where(hr, .divider, ._divider, ._hr) { opacity: 0.55 !important; }

/* overlay */
:where(. _modalBg, ._overlay, ._backdrop, ._dim, ._scrim, [data-backdrop="true"]) {
  background: rgba(0, 0, 0, 0.18) !important;
}

/* ------------------------------
 * 5) Buttons / inputs
 * ------------------------------ */
:where(main, ._panel, ._popup, ._acrylic) :where(button._button, a._button) {
  border-radius: var(--mk-radius-sm) !important;
  transition: transform 120ms ease, box-shadow 120ms ease, filter 120ms ease;
}
:where(main, ._panel, ._popup, ._acrylic) :where(button._button, a._button):hover {
  transform: translateY(-1px);
  box-shadow: var(--mk-shadow-sm);
}
:where(main, ._panel, ._popup, ._acrylic) :where(button._button, a._button):active {
  transform: translateY(0px);
  box-shadow: none;
}

:where(button._button, a._button, input, textarea, select):focus-visible {
  outline: 2px solid var(--mk-accent) !important;
  outline-offset: 2px !important;
}

:where(input:not([type="checkbox"]):not([type="radio"]):not([type="range"]), textarea, select) {
  border-radius: var(--mk-radius-sm) !important;
}
:where(input:not([type="checkbox"]):not([type="radio"]):not([type="range"]), textarea, select):focus {
  outline: none !important;
  border-color: var(--mk-accent) !important;
  box-shadow: 0 0 0 2px var(--mk-focus) !important;
}

/* ------------------------------
 * 6) Scrollbar
 * ------------------------------ */
*::-webkit-scrollbar { width: 12px; height: 12px; }
*::-webkit-scrollbar-thumb {
  background: var(--MI_THEME-scrollbarHandle, rgba(0, 0, 0, 0.20));
  border-radius: 999px;
  border: 4px solid transparent;
  background-clip: content-box;
}
*::-webkit-scrollbar-thumb:hover {
  background: var(--MI_THEME-scrollbarHandleHover, rgba(0, 0, 0, 0.35));
  background-clip: content-box;
}

/* ------------------------------
 * 7) Sidebar / nav as floating glass
 * ------------------------------ */
:where(aside, nav) {
  border-radius: var(--mk-radius-lg) !important;
  border: 1px solid var(--mk-divider) !important;
  box-shadow: var(--mk-shadow-md) !important;
  background: var(--mk-glass) !important;
  -webkit-backdrop-filter: var(--mk-blur);
  backdrop-filter: var(--mk-blur);
  overflow: hidden !important;
}

/* ------------------------------
 * 8) Switches (menu panel priority fix)
 * - 목표: 패널 안에서 "검은 썸"이 중앙/여백/모양이 맞게
 * ------------------------------ */
:where(._sheet, ._bottomSheet, ._menu, ._contextMenu, ._menuPanel, ._popupMenu, ._popup, ._acrylic, [role="dialog"], [data-modal="true"])
:where(._switch, .switch, [role="switch"]) {
  position: relative !important;
  overflow: hidden !important;
  border-radius: 0 !important;

  background: rgba(255,255,255,var(--mk-sw-track-a)) !important;
  -webkit-backdrop-filter: var(--mk-blur) !important;
  backdrop-filter: var(--mk-blur) !important;

  border: 1px solid color-mix(in srgb, var(--mk-divider) 65%, transparent) !important;
  box-shadow: 0 10px 28px rgba(0,0,0,0.16) !important;

  -webkit-mask-image:
    radial-gradient(var(--mk-sw-sq) at var(--mk-sw-sq) var(--mk-sw-sq), #000 98%, transparent 100%),
    radial-gradient(var(--mk-sw-sq) at calc(100% - var(--mk-sw-sq)) var(--mk-sw-sq), #000 98%, transparent 100%),
    radial-gradient(var(--mk-sw-sq) at var(--mk-sw-sq) calc(100% - var(--mk-sw-sq)), #000 98%, transparent 100%),
    radial-gradient(var(--mk-sw-sq) at calc(100% - var(--mk-sw-sq)) calc(100% - var(--mk-sw-sq)), #000 98%, transparent 100%),
    linear-gradient(#000, #000);
  -webkit-mask-composite: source-over, source-over, source-over, source-over, source-in;
}

/* on state (keep existing on logic; only visuals) */
:where(._sheet, ._bottomSheet, ._menu, ._contextMenu, ._menuPanel, ._popupMenu, ._popup, ._acrylic, [role="dialog"], [data-modal="true"])
:where(
  ._switch[aria-checked="true"],
  .switch[aria-checked="true"],
  [role="switch"][aria-checked="true"],
  ._switch.on, .switch.on,
  ._switch.active, .switch.active,
  ._switch.checked, .switch.checked
){
  background: rgba(227,103,73,0.52) !important;
  border-color: color-mix(in srgb, var(--mk-accent) 28%, transparent) !important;
  box-shadow: 0 0 0 2px var(--mk-sw-glow), 0 14px 34px rgba(0,0,0,0.18) !important;
}

/* internal thumb element if present */
:where(._sheet, ._bottomSheet, ._menu, ._contextMenu, ._menuPanel, ._popupMenu, ._popup, ._acrylic, [role="dialog"], [data-modal="true"])
:where(._switch, .switch, [role="switch"])
:where(._thumb, .thumb, .handle, ._handle, .knob, ._knob) {
  height: calc(100% - (var(--mk-sw-inset) * 2)) !important;
  aspect-ratio: 1 / 1 !important;
  position: absolute !important;
  top: 50% !important;
  transform: translateY(-50%) !important;
  left: var(--mk-sw-inset) !important;

  border-radius: 0 !important;
  background: rgba(0,0,0,0.82) !important;
  box-shadow: 0 10px 24px rgba(0,0,0,0.28) !important;

  -webkit-mask-image:
    radial-gradient(var(--mk-sw-sq) at var(--mk-sw-sq) var(--mk-sw-sq), #000 98%, transparent 100%),
    radial-gradient(var(--mk-sw-sq) at calc(100% - var(--mk-sw-sq)) var(--mk-sw-sq), #000 98%, transparent 100%),
    radial-gradient(var(--mk-sw-sq) at var(--mk-sw-sq) calc(100% - var(--mk-sw-sq)), #000 98%, transparent 100%),
    radial-gradient(var(--mk-sw-sq) at calc(100% - var(--mk-sw-sq)) calc(100% - var(--mk-sw-sq)), #000 98%, transparent 100%),
    linear-gradient(#000, #000);
  -webkit-mask-composite: source-over, source-over, source-over, source-over, source-in;
}

/* pseudo thumb if used */
:where(._sheet, ._bottomSheet, ._menu, ._contextMenu, ._menuPanel, ._popupMenu, ._popup, ._acrylic, [role="dialog"], [data-modal="true"])
:where(._switch, .switch, [role="switch"])::before,
:where(._sheet, ._bottomSheet, ._menu, ._contextMenu, ._menuPanel, ._popupMenu, ._popup, ._acrylic, [role="dialog"], [data-modal="true"])
:where(._switch, .switch, [role="switch"])::after{
  border-radius: 0 !important;
  -webkit-mask-image:
    radial-gradient(var(--mk-sw-sq) at var(--mk-sw-sq) var(--mk-sw-sq), #000 98%, transparent 100%),
    radial-gradient(var(--mk-sw-sq) at calc(100% - var(--mk-sw-sq)) var(--mk-sw-sq), #000 98%, transparent 100%),
    radial-gradient(var(--mk-sw-sq) at var(--mk-sw-sq) calc(100% - var(--mk-sw-sq)), #000 98%, transparent 100%),
    radial-gradient(var(--mk-sw-sq) at calc(100% - var(--mk-sw-sq)) calc(100% - var(--mk-sw-sq)), #000 98%, transparent 100%),
    linear-gradient(#000, #000);
  -webkit-mask-composite: source-over, source-over, source-over, source-over, source-in;
}

:where(._sheet, ._bottomSheet, ._menu, ._contextMenu, ._menuPanel, ._popupMenu, ._popup, ._acrylic, [role="dialog"], [data-modal="true"])
:where(._switch, .switch, [role="switch"]):focus-visible{
  outline: 2px solid color-mix(in srgb, var(--mk-accent) 55%, transparent) !important;
  outline-offset: 2px !important;
}

/* ------------------------------
 * 9) Dark mode adjustments
 * ------------------------------ */
@media (prefers-color-scheme: dark) {
  :root {
    --mk-glass: rgba(20, 20, 22, 0.50);
    --mk-glass-strong: rgba(20, 20, 22, 0.66);

    --mk-shadow-sm: 0 8px 22px rgba(0, 0, 0, 0.42), 0 2px 8px rgba(0, 0, 0, 0.28);
    --mk-shadow-md: 0 18px 46px rgba(0, 0, 0, 0.50), 0 6px 18px rgba(0, 0, 0, 0.34);
    --mk-shadow-lg: 0 32px 110px rgba(0, 0, 0, 0.58), 0 12px 34px rgba(0, 0, 0, 0.40);
  }

  @supports (background: color-mix(in srgb, white, transparent)) {
    :root {
      --mk-glass: color-mix(in srgb, var(--mk-panel) 64%, transparent);
      --mk-glass-strong: color-mix(in srgb, var(--mk-panel) 78%, transparent);
    }
  }

  :where(
    ._modal, ._dialog, ._sheet, ._bottomSheet, ._drawer,
    ._menu, ._contextMenu, ._menuPanel, ._popupMenu,
    [role="dialog"], [data-modal="true"]
  ){
    background: rgba(20, 20, 22, 0.58) !important;
    border: 1px solid rgba(255, 255, 255, 0.07) !important;
    box-shadow: 0 30px 110px rgba(0, 0, 0, 0.62), 0 12px 34px rgba(0, 0, 0, 0.40) !important;
  }

  :where(. _modalBg, ._overlay, ._backdrop, ._dim, ._scrim, [data-backdrop="true"]) {
    background: rgba(0, 0, 0, 0.30) !important;
  }

  :where(._sheet, ._bottomSheet, ._menu, ._contextMenu, ._menuPanel, ._popupMenu, ._popup, ._acrylic, [role="dialog"], [data-modal="true"])
  :where(._switch, .switch, [role="switch"]) {
    background: rgba(20,20,22,var(--mk-sw-track-a-dark)) !important;
    border-color: rgba(255,255,255,0.10) !important;
    box-shadow: 0 18px 46px rgba(0,0,0,0.46) !important;
  }
}

```