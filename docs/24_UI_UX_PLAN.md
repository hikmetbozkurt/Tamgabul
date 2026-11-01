# 24 — UI/UX Tasarım ve Uygulama Planı

**Durum:** Tasarım tamam (v1) · UI kodu U0–U8 tamam (MOCK veriyle), U9'un bir kısmı açık — bkz. §11 · **Tarih:** 2 Kasım 2026
**Tasarım tuvali:** https://claude.ai/artifact/P7D6wU3f8FeKtoY4F8ors3 (özel; sadece sahibi açabilir)
**Referans görsel:** `docs/design/reference-expanded.png` — genişletilmiş uygulamanın hedef görünümü.

Bu belge `docs/09_UI_UX.md` içindeki **işlev sözleşmelerini değiştirmez** (durum anlamları, gizlilik görünürlüğü, model ekle/kaldır, klasör izni/kaldırma akışları). Değişen yalnızca yerleşim, görsel dil ve pencere yapısıdır; 09'daki "UX change control" buna izin verir.

---

## 1. Kararlar

| # | Karar | Neden |
|---|---|---|
| D1 | **İki yüzey, tek React kodu.** `widget` penceresi (560×500, sağ alt / macOS'ta üst orta) ve `app` penceresi (varsayılan 1366×956, en küçük 1024×700). Aynı bileşenler, aynı durum, aynı IPC. | Kullanıcı widget'ın tam uygulamaya genişlemesini istiyor. |
| D2 | **Ayrı Settings penceresi yok.** 09'daki sekmeler uygulamanın sayfalarına taşındı: Models → **Models**, Sources → **Folders**, Performance + Privacy + About → **Settings**. | Referans görselde bunlar sol menüde. |
| D3 | **Görsel dil = referans görsel.** Renk, köşe, yazı ve bileşenler §3–4'te. Tuvaldeki "Tasarım sistemi" panosu tek doğru kaynak. | Kullanıcı birebir aynı görünüm istiyor. |
| D4 | **Yazı tipi Inter, uygulamaya gömülü** (`@fontsource/inter`, 400–800). İnternetten yüklenmez. Yedek: `Segoe UI Variable`, `system-ui`. | Windows ve macOS'ta aynı görünüm; ağ çağrısı yok. |
| D5 | **Özel başlık çubuğu.** `decorations: false` + `data-tauri-drag-region`. Sağda durum menüsü + küçült / büyüt / kapat. **Kapat (X) = tepsiye gizle**; gerçek Çıkış sadece tepsi menüsünde. App'te "küçült" ikonu = widget'a dön. | ADR 010; referans görseldeki başlık çubuğu. |
| D6 | **Durum anlamı değişmedi (ADR 011).** Referans görseldeki "TTS · Çevrimdışı" kırmızısı uygulanmaz: kurulmamış isteğe bağlı model = **gri halka**; kırmızı sadece zorunlu eksik / hata / uyumsuz. "Diskte, yüklü değil" için **turuncu** eklendi. Her nokta yanında yazı var. | 09'daki işlev sözleşmesi; ADR gerektirir. |
| D7 | **Sağ panel (Sistem Durumu + Klasörler) sadece Search sayfasında.** Bölümler daraltılabilir. Pencere < 1200 px → otomatik daralır; < 900 px → gizlenir, başlıktaki durum menüsünden açılır. | Diğer sayfalar tam genişlik ister. |
| D8 | **Sohbet, sohbet botu değil.** Asistan balonu sadece bulunan kaynaklardan konuşur. Her yanıtta şunlardan biri olur: sonuç kartı, netleştirme seçenekleri ya da "bulamadım + öneriler". | PRD: "no hallucinated file claims". |
| D10 | **Arayüz dili varsayılan İngilizce.** Settings → Language'da bayraklı (🇬🇧 / 🇹🇷, SVG) TR/EN seçimi. Tüm metinler `i18n/en.json` + `tr.json`; eksik anahtar İngilizceye düşer. Tepsi menüsü de dile uyar. | Kullanıcı kararı (2 Kasım 2026). |
| D11 | **Geniş uygulama açılınca widget gizlenir**; aynı anda tek yüzey görünür. | Kullanıcı kararı. |
| D12 | **Önizle = sağdan açılan panel** (ayrı pencere değil). Esc kapatır. | Kullanıcı kararı. |
| D9 | **Önce sahte veri (mock).** Tüm ekranlar typed mock IPC ile yapılır, `MOCK ONLY` rozeti görünür. Gerçek motor P1–P4 bitince bağlanır. | docs/23 kuralı. |

---

## 2. Ekran envanteri

| ID | Ekran | Tuval panosu | Gösterdiği durumlar | IPC (docs/13) |
|---|---|---|---|---|
| W1 | Widget — arama ve netleştirme | `Main` | sonuç, netleştirme çipleri, durum satırı | `submit_utterance`, `open_result`, `get_app_health` |
| W2 | Widget — ilk açılış | `Onboarding` | EG2 kırmızı, arama kapalı | `get_app_health` |
| W3 | Widget — bas-konuş | `Voice` | kayıt, süre, düzeltilebilir transkript | `submit_audio` |
| A1 | Search — başlangıç | `AppSearchHome` | boş hâl, öneri çipleri, son aramalar | `list_history` |
| A2 | Search — sonuç | `AppExpanded` (referans) | sohbet + sonuç kartı + sağ panel | `submit_utterance`, `open_result`, `list_models`, `list_scopes` |
| A3 | Search — netleştirme / sonuç yok | `AppSearchClarify` | LLM yok uyarısı, seçim çipleri, "bulamadım", daraltılmış panel | `submit_utterance` |
| A4 | Memory | `AppMemory` | oturum listesi, zaman çizelgesi, sil, geçmiş kaydı aç/kapa | `list_history`, `delete_history` |
| A5 | Models | `AppModels` | EG2 sabit kart; Hazır / Diskte / Uyumsuz; TTL | `list_models`, `load_model`, `unload_model`, `set_model_policy` |
| A6 | Model ekle | `AppModelAdd` | tür seçimi, dosya, doğrulama adımları, iptal | `register_model`, `validate_model` |
| A7 | Folders | `AppFolders` | güncel / taranıyor / duraklatıldı / yol erişilemez; elle vs boştayken | `list_scopes`, `start_index_job`, `cancel_job` |
| A8 | C:\ uyarısı + kaldırma | `AppFolderRoot` | ön izleme, otomatik hariçler, onay; üst/alt klasör kaldırma seçimi | `preview_scope`, `grant_scope`, `revoke_scope` |
| A9 | Settings | `AppSettings` | kısayol çakışması, dil, konum, mikrofon modu, performans, gizlilik, hakkında | yeni: `get_settings`, `set_settings` |
| A10 | Onboarding (geniş) | `AppOnboarding` | 4 adım, EG2 zorunlu | `get_app_health` |

**Henüz çizilmedi, tokenlarla yapılacak:** Önizle çekmecesi (sonuç kartındaki "Önizle"), yükleniyor iskeletleri, hata bildirimi (toast), tepsi menüsü (yerel menü, çizim gerekmez).

---

## 3. Tasarım tokenları

`apps/desktop/src/styles/tokens.css` olarak girilecek. Değerler tuvaldeki "Tasarım sistemi" panosuyla aynı.

```css
:root {
  /* yüzey */
  --bg: #FAFAF9;  --surface: #FFFFFF;  --main: #FBFBFA;  --sidebar: #F8F8F7;
  --chip: #F4F4F3;  --icon-tile: #F2F2F1;  --nav-active: #E7ECF5;
  --bubble-user: #E3EAF8;  --bubble-ai: #F1F1F0;  --file-tile: #E6EDFB;
  --line: #E4E4E2;  --line-soft: #ECECEA;  --line-row: #F0F0EE;  --line-input: #E2E2E0;
  /* metin */
  --ink: #1A1D24;  --ink-2: #3A3D44;  --muted: #6B7079;  --muted-2: #8A8E96;
  /* vurgu */
  --primary: #1F2533;  --accent: #4F6FD8;  --spark: #8B7FE0;
  /* durum: nokta / yumuşak arka plan / yazı */
  --ok: #2EAA4F;   --ok-soft: #E9F6EC;   --ok-text: #1E7A3A;
  --warn: #D9952B; --warn-soft: #FBF1E1; --warn-text: #8A5A00;
  --err: #E5484D;  --err-soft: #FDECEC;  --err-text: #B42328;  --err-line: #F0C9CB;
  --neutral: #8A8E96;
  --toggle-on: #2E9E4F;  --folder-fill: #FCE7B8;  --folder-line: #E0A03A;
  /* köşe */
  --r-pill: 999px; --r-card: 14px; --r-panel: 12px; --r-btn: 12px;
  --r-nav: 10px;   --r-field: 10px; --r-icon: 8px;  --r-dialog: 16px;
  /* gölge */
  --shadow-bar: 0 2px 8px rgba(20,22,30,.06);
  --shadow-card: 0 1px 4px rgba(20,22,30,.04);
  --shadow-dialog: 0 20px 60px rgba(20,22,30,.22);
  --shadow-widget: 0 12px 40px rgba(20,22,30,.14);
  /* yazı */
  --font: "Inter", "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif;
  /* hareket */
  --ease: cubic-bezier(.2,.7,.2,1); --dur: 200ms;
}
@media (prefers-reduced-motion: reduce) { :root { --dur: 0ms; } }
```

**Yazı ölçeği:** hero 52/800 (−1.2 harf aralığı) · sayfa başlığı 28/700 · kart başlığı 19/700 · bölüm 16.5/600 · gövde 17 · ikincil 15.5 · etiket 14.5 · küçük 13.5 · rozet 12.5/600.
**Boşluk:** 4 · 8 · 12 · 16 · 20 · 24 · 32 · 48. Sayfa iç boşluğu 40/48 px.
**Ölçüler:** başlık çubuğu 54 · sol menü 206 · sağ panel 310 · menü öğesi 50 · arama çubuğu 74 (hero) / 62 (alt) / 52 (widget) · buton 44 (app) / 36–40 (kart içi) · çip 38 · ikon kutusu 34 / 40 / 44 / 52 / 70.

---

## 4. Bileşenler

`apps/desktop/src/ui/` altında. Her biri `/gallery` geliştirici sayfasında görünür.

| Bileşen | Varyantlar | Kullanıldığı yer |
|---|---|---|
| `AppShell` | menülü / menüsüz (onboarding) | A1–A10 |
| `TitleBar` | app / widget; durum menüsü; pencere butonları | tümü |
| `SideNav` + `NavItem` | aktif / pasif; altta gizlilik kartı | A1–A9 |
| `StatusPanel` | açık / daraltılmış; Sistem Durumu, Klasörler | A1–A3 |
| `SearchBar` | hero (74) / takip (62, kıvılcım ikonlu) / widget (52) / kapalı | A1–A3, W1–W2 |
| `Chip` | öneri / meta (ikonlu) / seçim (seçili = koyu çerçeve) | A1–A3, W1 |
| `Bubble` | kullanıcı (+avatar, saat) / asistan (+kıvılcım avatar) / "bulamadım" (kesik çizgi) | A2, A3 |
| `ResultCard` | tam (snippet + meta çipler + Önizle/Aç) / kompakt (widget) / geçmiş satırı | A2, W1, A4 |
| `Button` | primary (koyu) / secondary (çerçeve) / danger (kırmızı yazı) / danger-solid / soft (gri, tam genişlik) / icon / icon-round | tümü |
| `StatusDot` + `StatusLabel` | ok / warn / err / neutral-halka — **yazısız kullanılamaz** | tümü |
| `StatusPill` | aynı 4 ton, yumuşak arka planla | A5, A4 |
| `Toggle` | açık / kapalı, `role="switch"` | A7, A9, A4 |
| `Segmented` | 2–4 seçenek, `radiogroup` | A6, A7, A9 |
| `Select`, `TextField`, `Kbd` | — | A9, A6 |
| `ModelCard` | zorunlu (geniş, ayrıntı ızgarası) / standart / hata çerçeveli | A5 |
| `FolderRow` | güncel / taranıyor (ilerleme) / duraklatıldı / erişilemez; açılır işlem satırı | A7, sağ panel |
| `StatTile` | sayı + etiket | A7 |
| `Dialog` | onay / akış; odak kilidi; Esc kapatır | A6, A8 |
| `ChecklistRow` | tamam / sürüyor (iptal) / hata | A6 |
| `Timeline` | oturum adımları | A4 |
| `Banner` | bilgi (turuncu nokta) — ör. "LLM yüklü değil" | A3 |
| `EmptyState`, `Skeleton`, `Toast` | — | tümü |

---

## 5. Etkileşim kuralları

- **Klavye:** `Ctrl+Shift+Space` widget'ı açar/gizler (değiştirilebilir). Widget'ta `Esc` gizler. Arama kutusunda `Enter` gönderir. `↑/↓` sonuçlarda gezer, `Enter` açar. Netleştirme çiplerine `1–9` kısayolu. App'te `Ctrl+K` aramaya odaklar, `Ctrl+1…5` sayfalar arası geçer.
- **Odak halkası:** 2 px `--accent`, 2 px boşluk. Fareyle tıklamada görünmez (`:focus-visible`).
- **Hareket:** 180–220 ms; panel daralma, diyalog açılma, widget↔app geçişi. Azaltılmış harekette kapalı.
- **Erişilebilirlik:** gerçek `button` / `a` / `input`; ikon-butonlarda `aria-label`; metin kontrastı ≥ 4.5:1; %200 ölçekte yatay kaydırma yok; hedef en az 36 px (widget) / 40 px (app).
- **Boş / yükleniyor / hata:** her sayfanın üçü de var. Hata = sebep + tek düğme (Tekrar dene / Yeniden bağla / Değiştir). Yükleniyor = iskelet; 300 ms altında gösterilmez.
- **Gizlilik görünürlüğü:** başlıktaki "Yerel olarak çalışıyor" her zaman görünür; mikrofon açıkken widget'ta ve tepsi ikonunda kayıt göstergesi.
- **Dil:** tüm metinler i18n anahtarı; TR varsayılan, EN yedek.

---

## 6. Kod yapısı

```text
apps/desktop/src/
  styles/        tokens.css, base.css (Inter @font-face, reset)
  ui/            §4 bileşenleri (sadece görünüm, IPC yok)
  shell/         AppShell, TitleBar, SideNav, StatusPanel, pencere yardımcıları
  features/
    search/      A1–A3 + W1 mantığı (oturum durumu, netleştirme)
    memory/      A4
    models/      A5–A6
    folders/     A7–A8
    settings/    A9
    onboarding/  A10 + W2
    voice/       W3
  state/         React context + useReducer (ek kütüphane yok); health/models/scopes önbelleği
  ipc/           typed komutlar + event dinleyici + MOCK katmanı (bindings/ tipleriyle)
  i18n/          tr.json, en.json
  widget.tsx     widget penceresinin girişi
  app.tsx        app penceresinin girişi
  gallery.tsx    yalnız geliştirmede: tüm bileşenler
```

Sayfa geçişi küçük bir iç yönlendirici ile (`state` + URL hash). Yeni bağımlılıklar: `@fontsource/inter`, `@tauri-apps/plugin-dialog` (dosya/klasör seçici), `@tauri-apps/plugin-global-shortcut`, `@playwright/test` (ekran görüntüsü testi).

---

## 7. Gereken IPC tipleri (Rust → ts-rs)

UI fazları bu tipleri **sahte veriyle** kullanır; Rust tarafı P1–P4'te gerçek veriyi doldurur. Hepsi `crates/core/src/ipc.rs` içine, docs/13 isimleriyle.

| Faz | Tip / komut |
|---|---|
| U2 | `SearchRequest`, `SearchResponse { turn_id, kind: results \| clarify \| not_found, message_key, choices[], results[] }`, `ResultCard { result_ref, title, display_path, project, file_kind, snippet, highlights[], modified_ago, opened }`, `list_scopes → ScopeSummary[]` |
| U3 | `ModelEntry { model_id, category, display_name, status, tone, required, runtime, dim, memory_bytes?, validated_at?, ttl, error_code? }`, `ValidationStep` event |
| U4 | `ScopePreview { token, file_count, est_bytes, cap, risk_flags[], auto_excludes[] }`, `RevokePolicy`, `index_progress` event |
| U5 | `HistorySession`, `HistoryTurn` |
| U6 | `AppSettings { language, hotkey, widget_anchor, mic_mode, autostart, history_retention }`, `HotkeyConflict` |

---

## 8. Fazlar

Sıra: **U0 → U1 → (U2 · U3 · U4 · U5 · U6 paralel olabilir) → U7 → U8 → U9.** Her faz sahte veriyle biter; gerçek motor bağlantısı ilgili backend fazında yapılır.

### U0 — Tasarım sistemi temeli
- `tokens.css`, Inter gömme, `ui/` bileşenleri, `/gallery` sayfası.
- Playwright ile `/gallery` ekran görüntüsü testi (Windows'ta temel görüntü).
- **Kabul:** gallery tuvaldeki "Tasarım sistemi" panosuyla gözle aynı; tüm bileşenler klavyeyle kullanılabilir; ağ isteği yok.

### U1 — Uygulama kabuğu ve iki pencere
- `tauri.conf.json`: `widget` + `app` pencereleri; özel başlık çubuğu; sürükleme bölgesi.
- Widget ↔ app geçişi; X = gizle; pencere boyut/konum hatırlama (`app_settings`).
- SideNav, sayfa yönlendirme, boş sayfa iskeletleri.
- P5-01 ile birleşir: tepsi menüsü, tek örnek, global kısayol + çakışma tespiti.
- **Kabul:** iki pencere de açılıp kapanıyor; X sonrası tepsi ikonundan geri geliyor; Çıkış süreçleri kapatıyor; konum yeniden açılışta korunuyor.

### U2 — Search (A1, A2, A3, W1)
- Hero arama, öneri çipleri, son aramalar; sohbet akışı; sonuç kartı (Önizle / Aç / klasörde göster); netleştirme çipleri; "bulamadım" önerileri; takip arama çubuğu.
- Sağ panel (açık/daraltılmış) ve genişliğe göre davranış (D7).
- Widget kompakt hâli.
- **Kabul:** A2 ekranı referans görselle yan yana karşılaştırmada aynı; üç yanıt türü de (sonuç / netleştirme / bulamadım) mock ile görünüyor; klavyeyle baştan sona kullanılabiliyor.

### U3 — Models (A5, A6)
- Model kartları, durum pill'leri, TTL seçimi, Yükle/Boşalt/Doğrula/Değiştir/Kaldır.
- Model ekle diyaloğu: tür, dosya seçici, doğrulama adımları (event akışı), iptal.
- **Kabul:** 11 durumun hepsi doğru renk + yazıyla görünüyor; AVAILABLE asla yeşil değil; doğrulama bitmeden Kaydet kapalı.

### U4 — Folders (A7, A8)
- Klasör listesi, istatistikler, indeksleme modu, satır işlemleri.
- C:\ diyaloğu (ön izleme, hariçler, onay kutusu), üst/alt klasör kaldırma diyaloğu.
- **Kabul:** "Durdur" ile "Kaldır ve temizle" ayrı; C:\ onaysız eklenemiyor; erişilemez yol kırmızı + "Yeniden bağla".

### U5 — Memory (A4)
- Gün gruplu oturum listesi, arama, zaman çizelgesi, oturum/tüm geçmiş silme, geçmiş kaydı anahtarı.
- **Kabul:** silme onaylı; "Aramaya devam et" oturumu Search'te açıyor.

### U6 — Settings (A9)
- Kısayol değiştirme (çakışma uyarısı), dil, widget konumu, mikrofon modu, Windows ile başlat, performans listesi, gizlilik, hakkında.
- **Kabul:** dil değişince tüm metin anında değişiyor; ölçülmemiş değerler "Ölçülmedi" yazıyor.

### U7 — Onboarding (A10, W2)
- İlk açılışta EG2 yoksa app onboarding'de açılır; widget W2'yi gösterir.
- **Kabul:** J01 akışı baştan sona; EG2 hazır olmadan "Aramaya başla" kapalı.

### U8 — Ses arayüzü (W3)
- Bas-konuş butonu durumları (STT yok → kapalı + açıklama), kayıt görünümü, düzeltilebilir transkript, TTS oynat/durdur.
- **Kabul:** mikrofon izni reddedilince yazarak arama sorunsuz; kayıt göstergesi hep görünür.

### U9 — Cila ve kalite
- Önizle çekmecesi, toast, iskeletler; %200 ölçek; TR/EN tam; azaltılmış hareket.
- Tasarım karşılaştırma: her ekranın Playwright görüntüsü tuvaldeki panoyla yan yana; fark listesi kapanana kadar.
- **Kabul:** erişilebilirlik denetimi temiz; tüm ekranlarda boş/yükleniyor/hata hâlleri var.

---

## 9. Her UI işinde "bitti" tanımı

- Ekran tuvaldeki panoyla eşleşiyor (ekran görüntüsü eklenir).
- Sadece tokenlar kullanılıyor; elle yazılmış renk/köşe yok.
- Klavye, ekran okuyucu etiketi, odak sırası kontrol edildi.
- Boş / yükleniyor / hata hâlleri var.
- Metinler i18n anahtarında (TR + EN).
- Sahte veri kullanılıyorsa ekranda `MOCK ONLY` rozeti var.
- `npm run typecheck`, `npm test`, `cargo test --workspace` geçiyor.

---

## 10. Açık sorular

1. ~~Menü dili~~ → D10: her şey İngilizce, Settings'te bayraklı TR/EN.
2. **Koyu tema:** MVP'de yok. Sonra eklenecekse tokenlar buna hazır.
3. ~~App açıkken widget~~ → D11: gizlenir.
4. ~~Önizle~~ → D12: sağdan panel.

---

## 11. Uygulama durumu (2 Kasım 2026)

| Faz | Durum | Not |
|---|---|---|
| U0 Tasarım sistemi | ✅ | `styles/tokens.css`, Inter gömülü (`@fontsource/inter`), `ui/`, `?surface=gallery` |
| U1 Kabuk + iki pencere | ✅ | `widget` (şeffaf, köşeli) + `app` (çerçevesiz); özel başlık çubuğu; X = gizle; tepsi menüsü (dile göre); tek örnek; global kısayol + çakışma; autostart; app pencere konumu hatırlanır |
| U2 Search | ✅ MOCK | başlangıç, sonuç, netleştirme (1–9 kısayolu), bulunamadı, LLM-yok uyarısı, önizleme paneli, sağ panel (daraltılabilir) |
| U3 Models | ✅ MOCK | 4 kart, 11 durum, TTL, yükle/boşalt, model ekle akışı (adımlar, iptal) |
| U4 Folders | ✅ MOCK | liste, istatistik, elle/boştayken, C:\ ön izleme + onay, üst/alt kaldırma, yeniden bağla |
| U5 Memory | ✅ MOCK | gün grupları, filtre, zaman çizelgesi, oturum/tümünü sil, geçmiş anahtarı |
| U6 Settings | ✅ | dil (bayrak), kısayol yakalama + çakışma, konum, mikrofon modu, autostart, performans, gizlilik, hakkında, demo senaryo seçici |
| U7 Onboarding | ✅ MOCK | EG2 yoksa app ilk açılışta `#/welcome`; widget W2 |
| U8 Ses arayüzü | ✅ MOCK | bas-konuş görünümü, süre, düzeltilebilir transkript; **mikrofon kullanılmaz** (STT P6) |
| U9 Cila | ⏳ kısmen | erişilebilirlik temel, i18n tam, azaltılmış hareket ✅; Windows'ta %200 ölçek ve gözle tasarım karşılaştırması bekliyor |

**Veri katmanı:** `ipc/api.ts` sözleşmesi; `ipc/mock.ts` (MOCK ONLY, localStorage ile iki pencere arasında ortak) ve `ipc/real.ts` (health + settings gerçek, geri kalanı `NOT_IMPLEMENTED` → arayüzde dürüst "Not available yet"). Settings → Developer → Demo data kapatılınca gerçek (boş) durum görünür.

**Tasarım tuvali** hâlâ Türkçe metinli eski taslaktır; görsel kaynak artık kod + bu belgedir.
