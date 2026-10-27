# Local Context Widget — Başlangıç Rehberi

> **Bu paketin içinde çalışan uygulama yok.** Bunlar coding agent'lara teslim edilebilecek ürün, mimari, görev ve kalite belgeleridir. Çalışma adı **Local Context Widget**, marka adı henüz kararlaştırılmadı; Capsula değil.

## İndirip nasıl kullanacaksın?
1. Arşivi Windows bilgisayarında yeni, boş bir Git deposuna çıkar.
2. Codex/Claude/Gemini agent'ına şunu ver:

   ```text
   Read AGENTS.md, README.md, docs/20_ADR.md, and agent_tasks/00_BOOTSTRAP.md.
   Start only with P0-01 (repo bootstrap). Follow all tests and security invariants.
   Do not implement later phases, use LM Studio, auto-download models, or add 3D UI.
   Report changed files, tests actually executed, and blockers.
   ```

3. İlk işte veritabanı, runtime, tasarım veya model yükleme gibi sonraki fazların 'yapıldı' sayılmasına izin verme. `agent_tasks/01_RUNTIME_SPIKES.md` ile mevcut **Gemma 4 E4B Q4_K_M** ve resmi **EmbeddingGemma 2** uyumluluk/latency testlerini yap.
4. Her bir işi ayrı dal/commit şeklinde yürüt; görev bittiğinde test kanıtını iste. Faz sırası `docs/15_ROADMAP.md` içinde.
5. UI'ı geliştirdikçe `docs/09_UI_UX.md` üzerinde değişiklik yapabilirsin; bu doküman bilinçli olarak yalnızca yapı/akış ilkelerini sabitliyor.

## En önemli beş sınır
- **EmbeddingGemma 2 zorunlu**, kurulmadıysa kırmızı durum ve semantik arama devre dışı.
- **LLM opsiyonel**; konuşma hafızası yerel SQLite'ta, KV cache'e bağlı değil.
- **LM Studio bağımlılığı yok**; uygulamanın kendi yönettiği yerel runtime, otomatik kapanma süresi ve manuel unload.
- **Windows tray + close-to-hide** zorunlu; macOS menu-bar sonraki platform.
- **`C:\` dahil kaynak izni verilebilir ama kullanıcı onayı, sınırlı önizleme ve veri temizlenerek izin kaldırma zorunlu.**

## Deneysel kalanlar
EmbeddingGemma 2'nin nihai paketli runtime'ı, her TTS/STT modelinin lisans ve cihaz uyumluluğu, Gemma GGUF cold-start hızı ve RTX 4080 LoRA eğitim parametreleri gerçek test olmadan kesinleşmiş sayılmaz. Her biri `docs/19_RESEARCH_VERIFICATION.md` içinde işaretlidir.

## Belgeler
- Ürün/MVP: `docs/01_PRODUCT_PRD.md`, `docs/02_MVP_SCOPE.md`
- Mimarinin kalbi: `docs/03_ARCHITECTURE.md`–`docs/08_RETRIEVAL_CONVERSATION.md`
- Arayüz, platform, güvenlik: `docs/09_UI_UX.md`–`docs/13_API_CONTRACTS.md`
- Test, yol haritası, LoRA: `docs/14_TEST_EVAL.md`–`docs/23_AGENT_HANDOFF.md`
- Agent kuralları `AGENTS.md`, görev kartları `agent_tasks/`, SQLite/JSON örnekleri `database/`, `examples/`.
