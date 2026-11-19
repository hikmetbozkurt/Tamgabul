# Tamgabul Yardım

Tamgabul, yalnızca izin verdiğin klasörlerdeki dosyaları günlük Türkçe veya İngilizce tariflerle bulan çevrimdışı bir masaüstü widget'ıdır. Hiçbir veri bilgisayarından çıkmaz.

## 1. İlk Kurulum

1. **Paketle birlikte gelmediyse `llama.cpp` (`llama-server`) kur:**
   - Windows: `winget install ggml.llamacpp`
   - macOS: `brew install llama.cpp`
2. **Zorunlu embedding modelini ekle (EmbeddingGemma 2):**
   - **Modeller → Model ekle → Embedding** yolundan yerel `embeddinggemma-2` klasörünü seç (`mmproj-embeddinggemma-2-*.gguf` ile birlikte `embeddinggemma-2-*.gguf`) ya da **Resmî modeli indir** sekmesine geçip kaynak, toplam boyut, Apache-2.0 lisansı ve hedef klasörü açıkça onaylayarak SHA-256 doğrulamalı indir.
   - Tamgabul kaydetmeden önce dosya okunabilirliğini, GGUF başlıklarını, SHA-256 sağlama değerini, çalıştırıcı uyumunu doğrular ve kısa bir deneme çalıştırması yapar.
3. **İsteğe bağlı yerel LLM ekle (önerilen: Gemma 4 E2B-it):**
   - **Modeller → Model ekle → LLM** yolundan yerel GGUF talimat modelini seç (aynı klasördeki `mmproj-*` dosyası tarama sırasında görsel betimlemeyi etkinleştirir) veya onaylı indiriciden sabit Gemma 4 E2B-it + görsel paketini indir.
   - LLM olmadan da yazarak arama çalışır.
4. **Klasörlere izin ver:**
   - **Klasörler → Klasör ekle** ile bir dizin seç, ön izlemeyi (dosya sayısı, tahmini indeks boyutu, otomatik hariç tutulanlar) incele ve **İzin ver ve tara**'ya tıkla.

## 2. Arama Yapma

- Genel kısayol (`Ctrl + Shift + Space` Windows, `Cmd + Shift + Space` macOS) ya da tepsi / menü çubuğu ikonuyla widget'ı istediğin zaman aç.
- Hatırladığın şekilde Türkçe ya da İngilizce sor (örneğin `geçen haftaki fatura pdf` veya `the passport page with my UK visa`).
- **Yerleşik süzgeçler:** *bugün / today, dün / yesterday, geçen hafta / last week, bu ay / this month, PDF, ekran görüntüsü / screenshot, ses kaydı / audio, video* gibi ifadeler tarih ve dosya türü süzgecine dönüşür. Yalnızca süzgeç içeren istekler en yeni eşleşen dosyaları listeler.
- **Önceki sonuçlar:** *geçen bana bulmuştun* veya *the file you showed me* gibi istekler önce kayıtlı arama geçmişine bakar ve klasör iznini yeniden doğrular. Klasör kaldırılmışsa dosyayı göstermeden bunu bildirir.
- **İsteğe bağlı LLM ile:** **Ayarlar → Performans → LLM görevleri** altındaki anahtarlara göre LLM; eş anlamlıları/hariç tutulanları çıkarabilir, en iyi 20 aday arasından uyanları seçebilir (diğer eşleşmeleri **Diğer sonuçlar** altında görünür tutarak), kaynaklı tek cümlelik yanıt ve `[1]` atıflı özet yazabilir, ayrıca tarama sırasında görselleri bir kez betimleyebilir.

## 3. Modeller ve Bellek

Modelin **kurulu/doğrulanmış olması** ile **bellekte çalışıyor olması** ayrıdır:

| Durum | Renk | Anlamı |
|---|---|---|
| `Kurulu değil` (Embedding) / `Hata` / `Uyumsuz` | Kırmızı | Zorunlu embedding eksik ya da bir model doğrulamadan/çalıştırmadan geçemedi. EmbeddingGemma 2 geçerli olana kadar anlamsal arama kapalıdır. |
| `Diskte` / `Doğrulanıyor` / `Yükleniyor` / `Boşaltılıyor` | Sarı | Diskte doğrulandı (`AVAILABLE`) ya da geçiş hâlinde. `Diskte` durumu arama için yeterlidir — gerektiğinde işçi otomatik başlar. |
| `Hazır` / `Boşta` / `Çalışıyor` | Yeşil | Model işçisi şu anda bellekte yüklü. |
| `İsteğe bağlı` (LLM / STT / TTS) | Nötr | İsteğe bağlı yetenek kurulmadı; yazarak arama normal çalışır. |

- **Boşta boşaltma (TTL):** **Modeller** sayfasından her model için boşta kalma süresini seç (`İş bitince hemen`, `5 dk`, `10 dk`, `30 dk`, `1 saat` veya `Asla`).
- **Elle boşaltma:** Model kartındaki **Boşalt**, **Tümünü boşalt** veya tepsi / menü çubuğundaki **Modelleri boşalt** seçeneğini kullan. Boşaltma `llama-server` sürecini kapatır ve RAM/VRAM'i işletim sistemine geri verir.

## 4. Klasör Gizliliği ve Güvenlik

- **Salt okunur tarama:** Tamgabul kaynak dosyalarını asla değiştirmez veya silmez.
- **Otomatik ve klasör bazlı hariç tutulanlar:** Gizli klasörler, `.git`, `node_modules`, derleme/önbellek klasörleri, sembolik bağlar / junction'lar (asla takip edilmez) ve yalnızca bulutta duran dosyalar otomatik atlanır. Bir klasörün `…` menüsündeki **Hariç tutulanlar** düğmesiyle alt klasörleri dosyalarına dokunmadan hemen aramadan çıkarabilirsin.
- **Sürücü kökleri (`C:\` veya `/`) ve ağ klasörleri (`\\sunucu\paylaşım`):** Açık risk onayı kutucuğu işaretlenerek eklenebilir. Sürücü kökleri işletim sistemi klasörlerini otomatik hariç tutar; ağ paylaşımları varsayılan olarak elle taranır ve erişilemezken sonuçları gizlenir.
- **Klasör izleme ve yeniden bağlama:** İzleme açıldığında dosya değişiklikleri durduktan ~10 saniye sonra fark taraması yapılır. Bir klasörü taşıdıysan veya adını değiştirdiysen **Yeniden bağla** ile değişmeyen dosyaları yeniden vektörlemeden yeni yerine bağlayabilirsin.
- **Klasör kaldırma:**
  - **Sadece izlemeyi durdur:** Yeni tarama yapmaz, mevcut indeksi aranabilir tutar.
  - **Kaldır ve temizle:** Klasörü aramadan ve geçmişten hemen engeller, ardından türetilmiş indeks kayıtlarını siler.
  - **İzinli üst klasör içindeki alt klasör:** **Engelle ve temizle** (üst klasör üzerinden de taranmaz) veya **Sadece ayrı izni kaldır** (üst klasör üzerinden aranmaya devam eder) seçeneklerinden birini seç.

## 5. Sistem Tepsisi / Menü Çubuğu ve Kısayol

- Pencereleri (`X`) ile kapatmak uygulamayı tepsiye (Windows) veya menü çubuğuna (macOS) gizler.
- Tüm model işçi süreçlerini sonlandırıp çıkmak için tepsi / menü çubuğu menüsünden **Çıkış** seçeneğini kullan.
- Genel kısayol başka bir uygulama tarafından kullanılıyorsa **Ayarlar → Genel** çakışmayı gösterir; başka bir tuş birleşimi atayabilirsin.

## 6. Sorun Giderme

- **Arama kutusu devre dışı / kırmızı durum:** **Modeller** sayfasını kontrol et. EmbeddingGemma 2 doğrulamadan geçip `Diskte` veya yüklü durumda olmalıdır.
- **`llama-server` bulunamadı:** `llama.cpp` kur veya `llama-server` ikilisinin uygulama yanında, `PATH` içinde ya da `TAMGABUL_LLAMA_SERVER` ortam değişkeninde olduğundan emin ol.
- **Türkçe ekran görüntülerinde/taramalarda yazı bulunamıyor (Windows):** *Windows Ayarları → Saat ve dil → Dil* bölümünden Türkçe OCR özelliğini kur. (macOS'te Vision OCR yerleşiktir; gerçek donanım doğrulaması bekliyor.)
- **Sesle arama / sesli okuma (isteğe bağlı STT ve TTS):** Whisper `.bin` GGML modelleri için `whisper-server` (indekslenen ses dosyalarındaki konuşmaları da yazıya çevirir) ve/veya `.onnx` + `.onnx.json` Piper sesleri için `piper` kur (`tr_TR-dfki-medium` veri kümesi CC-BY-NC-SA-4.0 ticari olmayan lisanstır). Bunlar olmadan da yazarak arama her zaman çalışır.
- **Hata raporu için güvenli dışa aktarım:** **Ayarlar → Gizlilik → Tanılama dosyası kaydet** seçeneğini kullan. Çıktı yalnızca sürümleri, sayıları, durumları ve hata kodlarını içerir; dosya yolu, dosya adı, sorgu veya konuşma metni yer almaz.
