// src/theme/fonts.ts — the three font families, bundled (#268).
//
// Self-hosted from the @fontsource packages instead of the Google Fonts stylesheets
// index.html used to load: no network on first paint, no request to Google on every
// launch, and a CSP that can stay `font-src 'self'`. Only the faces the tokens use
// (`FONT_SERIF` / `FONT_SANS` / `FONT_MONO` in ./tokens), in the latin and latin-ext
// subsets, so accented preset and song names still render in the brand face. The
// subset files carry `unicode-range`, so the webview fetches a face only when text
// needs it.

import "@fontsource/inter/latin-400.css";
import "@fontsource/inter/latin-ext-400.css";
import "@fontsource/inter/latin-500.css";
import "@fontsource/inter/latin-ext-500.css";
import "@fontsource/inter/latin-600.css";
import "@fontsource/inter/latin-ext-600.css";

import "@fontsource/source-serif-4/latin-400.css";
import "@fontsource/source-serif-4/latin-ext-400.css";
import "@fontsource/source-serif-4/latin-500.css";
import "@fontsource/source-serif-4/latin-ext-500.css";
import "@fontsource/source-serif-4/latin-400-italic.css";
import "@fontsource/source-serif-4/latin-ext-400-italic.css";
import "@fontsource/source-serif-4/latin-500-italic.css";
import "@fontsource/source-serif-4/latin-ext-500-italic.css";

import "@fontsource/jetbrains-mono/latin-400.css";
import "@fontsource/jetbrains-mono/latin-ext-400.css";
import "@fontsource/jetbrains-mono/latin-500.css";
import "@fontsource/jetbrains-mono/latin-ext-500.css";
