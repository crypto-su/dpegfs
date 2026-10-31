# Geliştirme planı

İlk hedef, veriyi gerçekten piksellerde tutan, dosyaları birebir geri okuyabilen ve hata durumunda mevcut görüntüyü koruyan küçük bir Rust dosya sistemi kurmaktır. Alan kazancı ayrı ölçülür; görüntüye dönüştürmenin sıkıştırma oranını kendiliğinden artırdığı varsayılmaz.

## Aşama 1 — Çalışan temel (0.1.0)

- [x] Rust kütüphanesi ve komut satırı aracı.
- [x] Seçilebilir genişlik/yükseklik veya hedef ham kapasite.
- [x] Standart RGB8 PNG'nin gerçek kanal değerlerinde veri saklama.
- [x] Sürümlü başlık, dosya tablosu, ofsetler ve kapasite denetimi.
- [x] Dosya yazma, okuma, listeleme, çıkarma, silme ve yeniden adlandırma.
- [x] Dosya bazında isteğe bağlı zlib; kazanç yoksa ham saklama.
- [x] Başlık, gövde ve dosya SHA-256 doğrulaması.
- [x] Geçici dosya üzerinden atomik kayıt ve yazıcı kilidi.
- [x] Yanlış yol, bozuk veri, kapasite taşması, başarısız kayıt ve eşzamanlı yazıcı testleri.
- [x] Türkçe/İngilizce kullanım, biçim belgesi ve tekrar çalıştırılabilir boyut karşılaştırması.
- [x] GitHub Actions test ve kod denetimi yapılandırması.

Bu aşamanın kabul ölçütleri: ikili dosyaların bayt bayt geri gelmesi, kapasiteyi aşan yazmada önceki diskin değişmemesi, içerik bozulmasının tespit edilmesi ve ikinci yazıcının reddedilmesi. Yerel testler bunları doğrular. GitHub Actions sonuçları yayımlanmış bir depoda ayrıca görülmelidir.

## Aşama 2 — Ölçüm ve ölçek

- [ ] Çeşitli gerçek veri kümelerinde ZIP/zstd gibi sıradan arşivlerle boyut, CPU ve bellek karşılaştırması.
- [ ] Tam görüntüyü belleğe almadan okuma/yazma stratejisi ve sınırların yeniden değerlendirilmesi.
- [ ] Blok tahsisi, değişen aralıkların takibi ve büyütme/küçültme tasarımı.
- [ ] Çok sayıda küçük dosyada metadata maliyeti ve tekrarlayan içeriklerin değerlendirilmesi.
- [ ] Dizin içe aktarma ve korumalı toplu çıkarma.
- [ ] Biçim değişiklikleri için sürüm geçişi ve uyumluluk testleri.

PNG'nin tek sıkıştırılmış piksel akışında küçük bir yazma bile yeniden kodlama gerektirebilir. Blok güncelleme ve performans tasarımı bu maliyeti ölçerek yapılmalı; v1'de tam PNG yazıldığı açıkça belgelenmiştir.

## Aşama 3 — İşletim sistemine bağlama

- [ ] Çekirdek API'yi rastgele erişim, dosya tutamaçları ve parçalı yazmaya genişletme.
- [ ] Linux FUSE ile mount/unmount ve dosya işlemleri.
- [ ] macOS ve Windows bağlama seçeneklerini ayrı değerlendirme.
- [ ] İzinler, zaman damgaları, dizin işlemleri, hata kodları ve eşzamanlılık sözleşmesi.
- [ ] Kapanma, kopan mount ve başarısız flush senaryoları.

FUSE eklemek tek başına bu prototipi üretim dosya sistemine dönüştürmez; dosya sistemi davranışlarının ayrı test edilmesi gerekir.

## Aşama 4 — Dayanıklılık ve isteğe bağlı özellikler

- [ ] Klasör senkronizasyonu, güç kesilmesi testleri ve kurtarma protokolü.
- [ ] Hata düzeltme/yedek blokların kapasite maliyetiyle birlikte tasarlanması.
- [ ] Anahtar yönetimi tasarlandıktan sonra isteğe bağlı kimliği doğrulanmış şifreleme.
- [ ] Uzun süreli biçim uyumluluğu ve dış okuyucu doğrulaması.
- [ ] Gereksinim oluşursa dosya sisteminden ayrı bir indeks/veritabanı katmanı.

Bu maddeler sonraki geliştirmelerdir; mevcut sürümün özellikleri olarak sunulmaz. Öncelik sırası: doğruluk → ölçüm → ölçek → işletim sistemi entegrasyonu.
