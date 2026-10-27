# Local Context Widget — Ürün ve Mühendislik Belgeleri

> **Çalışma adı:** Local Context Widget (LCW). Ürün adı **kesinleşmedi**; bu proje Capsula'dan **tamamen ayrıdır** ve **3D kapsül / görsel harita içermez**.
>
> **Belge sürümü:** 0.1 / 26 Ekim 2026 • **Durum:** Tasarım spesifikasyonu — çalışan uygulama veya doğrulanmış benchmark değil.

## Bir cümlede ürün
Windows'ta sistem tepsisinde yaşayan, global kısayolla açılan; kullanıcının izin verdiği yerel dosyaları **EmbeddingGemma 2** ile indeksleyip, sesli/yazılı **çok turlu doğal konuşma** üzerinden bulmasına yardım eden; yerel LLM, STT, TTS ve embedding modellerini kendi içinde ekleyip yöneten **offline-first masaüstü widget**. Aynı kod tabanı sonra macOS'ta test edilecek.

## Sabitlenmiş kararlar
1. **Windows ilk teslimat; macOS aynı monorepo ve ortak çekirdek üzerinde ikinci hedef.** Windows notification area / system tray, pencere kapanınca gizlenme, tepsi menüsünde gerçek `Quit`; macOS menu bar eşdeğeri.
2. **3D kapsül yok.** Bir kompakt arama/konuşma widget'ı ve ayrı minimal `Settings` penceresi.
3. **EmbeddingGemma 2 arama motorunun zorunlu modeli**: yoksa veya uyumsuzsa semantik indeksleme/arama devre dışı ve UI'da kırmızı durum. LLM/STT/TTS isteğe bağlı, özelliğe göre degradation.
4. Kullanıcı **kendi modellerini import eder**; hiçbiri gizlice indirilmez. Yalnızca doğrulanmış runtime/format/capability eşleşmeleri etkinleştirilir. İstediği modelin kaydı ≠ o modelin çalıştırılacağı garantisi.
5. **LM Studio bağımlılığı yok.** Uygulama model yürütücülerini yerel sidecar/embedded runtime üzerinden yönetir; bulut inference varsayılan olarak hiç yok.
6. **Konuşma ve arama geçmişi SQLite'ta kalıcı**; LLM KV cache hafıza kaynağı değildir. Model boşalsa, uygulama kapansa, OS yeniden başlasa da geçmiş korunur.
7. **Tüm model ağırlıkları ihtiyaç anında yüklenir, TTL ile boşaltılır**; TTL kullanıcı tarafından model bazında ayarlanır. Lightweight orchestration daemon/tray sürekli çalışabilir. Gerçek boşaltım ölçülür.
8. **Klasör erişimi açık izin listesiyle**: `C:\` kökü dahil eklenebilir; korumalı kök uyarısı, önizleme, dışlama, güvenli sınırlama ve iptal sağlanır. İzin kaldırılması indeks verisi için de geçerlidir.
9. Kaynak dosyalar **varsayılan salt okunur**. Model, kullanıcının dosyasını kendiliğinden silemez, değiştiremez veya dışarı gönderemez.
10. **RTX 4080 16 GB** üzerinde EmbeddingGemma 2 için ileride LoRA/contrastive adaptation laboratuvarı planlıdır. İlk MVP'de training yok.

## Nereden başlanmalı?
- **Geliştirici / coding agent:** `AGENTS.md` → `docs/00_INDEX.md` → `docs/20_ADR.md` → `docs/15_ROADMAP.md` → `agent_tasks/00_BOOTSTRAP.md`.
- **Ürün / UX:** `docs/01_PRODUCT_PRD.md`, `docs/02_MVP_SCOPE.md`, `docs/09_UI_UX.md`, `docs/21_USER_JOURNEYS.md`.
- **Çekirdek mühendisliği:** `docs/03_ARCHITECTURE.md` → `docs/04_MODEL_REGISTRY.md` → `docs/05_MODEL_LIFECYCLE.md` → `docs/06_MEMORY_ENGINE.md` → `docs/07_INDEXING_PERMISSIONS.md` → `docs/08_RETRIEVAL_CONVERSATION.md`.
- **Kalite:** `docs/14_TEST_EVAL.md`, `docs/18_RISK_REGISTER.md`, `docs/19_RESEARCH_VERIFICATION.md`.

## Repo hedefi (belgeler uygulanınca)
```text
apps/desktop/              Tauri v2 + React + TypeScript (widget / settings)
crates/core/               Rust orchestration, model registry, indexing, memory, retrieval
crates/platform/           Windows/macOS davranış adaptörleri
runtimes/                  Bundled and managed local inference workers (per platform)
ai_lab/                    Future EmbeddingGemma 2 adaptation/evals
migrations/                Versioned SQLite migrations
models/                    Optional user-imported models in app-data or referenced paths
```
`models/`, gerçek kullanıcı dosyaları, database ve eğitim verileri **git'e eklenmez**. Bu arşiv **dokümantasyon paketidir**, çalışan program iddiası taşımaz.

## Kapsam dışı — ilk MVP
Capsula entegrasyonu; mobil uygulama; 3D cluster visualization; bulut account; web crawling; tüm diskleri kendiliğinden tarama; kullanıcı dosyasını yazma/silme; tüm model formatlarının koşulsuz desteği; dağıtık/senkron veritabanı.

## İlk doğrulama
- Elde bulunan **Gemma 4 E4B Q4_K_M (~6.3 GB GGUF)** yerel dosyası PoC adayı. Sürüm/uyumluluk cihaz üzerinde doğrulanır, dosyanın yeri hardcode edilmez.
- Sistem embedding motoru için resmi `google/embeddinggemma-2` checkpoint'ini ayrı doğrular. Önce local PoC; Rust-native runtime **kanıtlanmadan vaat edilmez**.
- Gerçek performans rakamları için `docs/14_TEST_EVAL.md` protokolü geçerlidir.
