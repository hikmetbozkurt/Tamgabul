// Third-party software and models used by Tamgabul, with their licenses.
// Versions and licenses were read from Cargo.lock / package.json metadata
// (cargo metadata, node_modules/*/package.json) and the GGUF headers.
// The complete dependency list is in Cargo.lock and package-lock.json.

export type LicenseItem = { name: string; license: string; url: string; kind: "model" | "runtime" | "library" };

export const LICENSES: LicenseItem[] = [
  { name: "EmbeddingGemma 2 (GGUF, ggml-org)", license: "Apache-2.0", url: "https://huggingface.co/ggml-org/embeddinggemma-2-GGUF", kind: "model" },
  { name: "Gemma 4 E2B-it (GGUF, ggml-org)", license: "Apache-2.0", url: "https://huggingface.co/ggml-org/gemma-4-E2B-it-GGUF", kind: "model" },
  { name: "llama.cpp (llama-server)", license: "MIT", url: "https://github.com/ggml-org/llama.cpp", kind: "runtime" },
  { name: "SQLite (bundled via rusqlite)", license: "Public domain", url: "https://www.sqlite.org/copyright.html", kind: "library" },
  { name: "Tauri + plugins (dialog, autostart, global-shortcut)", license: "Apache-2.0 OR MIT", url: "https://tauri.app/", kind: "library" },
  { name: "React, React DOM", license: "MIT", url: "https://react.dev/", kind: "library" },
  { name: "rusqlite", license: "MIT", url: "https://github.com/rusqlite/rusqlite", kind: "library" },
  { name: "image", license: "MIT OR Apache-2.0", url: "https://github.com/image-rs/image", kind: "library" },
  { name: "Symphonia (audio decoding)", license: "MPL-2.0", url: "https://github.com/pdeljanov/Symphonia", kind: "library" },
  { name: "pdf-extract", license: "MIT", url: "https://github.com/jrmuizel/pdf-extract", kind: "library" },
  { name: "zip", license: "MIT", url: "https://github.com/zip-rs/zip2", kind: "library" },
  { name: "BLAKE3", license: "CC0-1.0 OR Apache-2.0", url: "https://github.com/BLAKE3-team/BLAKE3", kind: "library" },
  { name: "unicode-normalization", license: "(MIT OR Apache-2.0) AND Unicode-3.0", url: "https://github.com/unicode-rs/unicode-normalization", kind: "library" },
  { name: "windows-rs", license: "MIT OR Apache-2.0", url: "https://github.com/microsoft/windows-rs", kind: "library" },
  { name: "ureq, serde, sha2, base64, getrandom, win32job, ts-rs, thiserror", license: "MIT OR Apache-2.0 (ts-rs: MIT)", url: "https://crates.io/", kind: "library" },
  { name: "Inter (font)", license: "OFL-1.1", url: "https://rsms.me/inter/", kind: "library" },
];
