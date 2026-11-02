# Geliştirme

## Gereken araçlar (Windows 10/11)
- Rust stable (`rustup`), MSVC: Visual Studio Build Tools → "Desktop development with C++"
- Node.js 20+ (22 önerilir)
- WebView2 Runtime (Windows 11'de hazır; Windows 10'da yoksa Microsoft'tan kur)

## İlk kurulum
```powershell
cd apps\desktop
npm install
```

## Çalıştır
```powershell
cd apps\desktop
npm run tauri dev        # gerçek uygulama: tepsi ikonu + sağ altta widget
```
- Widget açılır. Genişlet ikonu → tam uygulama (widget gizlenir). Başlık çubuğundaki "—" → widget'a döner.
- X her iki pencerede de **tepsiye gizler**. Gerçek çıkış: tepsi ikonu → Quit.
- Kısayol: `Ctrl + Shift + Space` (Settings'ten değiştirilebilir; çakışırsa uyarı çıkar).

Tarayıcıda (Rust çekirdeği olmadan) sadece arayüz:
```powershell
npm run dev
# http://localhost:1420/?surface=app      tam uygulama
# http://localhost:1420/?surface=widget   widget
```

## Veri
Arayüz yalnızca Rust çekirdeğinden gelen **gerçek** durumu gösterir; sahte/demo veri yok (ADR 018). Altyapısı henüz gelmemiş bölümler (arama P2, klasörler P1, geçmiş P3, model yükleme P4, ses P6) "Not available yet" der.

## Modeller
Modeller repoya girmez. Önerilen yer `C:\AI\models\`:
- `C:\AI\models\embeddinggemma-2\` → `ggml-org/embeddinggemma-2-GGUF`, **BF16** dosyası (zorunlu)
- `C:\AI\models\gemma-4-e4b\` → Gemma 4 E4B `Q4_K_M` GGUF (isteğe bağlı)
Çalıştırıcı: llama.cpp `llama-server` (P0-02'de doğrulanacak).

## Dil
Arayüz varsayılan **İngilizce**. Settings → Language: bayraklı English / Türkçe. Metinler `apps/desktop/src/i18n/en.json` ve `tr.json` içinde; iki dosyada aynı anahtarlar olmalı (test kontrol eder).

## Test
```powershell
# repo kökünde
cargo test --workspace            # Rust testleri + TS tiplerini üretir (apps/desktop/src/bindings)
cargo clippy --workspace --all-targets -- -D warnings
cd apps\desktop
npm run typecheck
npm test                          # i18n, durum kuralları, ekran render testleri
```
Rust'taki bir IPC tipi değişince `cargo test -p tamgabul-core` çalıştır ve `src/bindings/` değişikliğini commit et.

## Yapı
```text
crates/core/            model durumu, sağlık kuralı, ayarlar, IPC tipleri (dto.rs), SQLite migration
crates/platform/        tepsi komutları, widget konumu kuralı, platform bilgisi
apps/desktop/src-tauri/ Tauri v2: komutlar, iki pencere, tepsi, kısayol, autostart
apps/desktop/src/
  styles/   tokens.css (renk, köşe, yazı), base, ui, app, widget
  ui/       ikonlar, bayraklar, ortak bileşenler
  shell/    başlık çubuğu, sol menü, durum paneli, yönlendirme
  features/ search, memory, models, folders, settings, onboarding, voice
  surfaces/ AppSurface, WidgetSurface
  ipc/      api.ts (sözleşme), core.ts (Rust çekirdeği), bridge.ts
  state/    ayarlar, veri, bildirim
  i18n/     en.json, tr.json
database/001_initial.sql   tek şema kaynağı (core bunu gömer)
docs/24_UI_UX_PLAN.md      tasarım kararları, tokenlar, fazlar, durum
```

## Veritabanı
Uygulama verisi klasöründe `tamgabul.db` (Windows: `%APPDATA%\dev.tamgabul.app\`). Ayarlar `app_settings` tablosunda `ui` anahtarıyla. Bekleyen migration varsa önce `tamgabul.db.bak-v<N>` yedeği alınır.
