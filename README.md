# dpegfs

**Dosyalarını bir PNG görüntüsünün piksellerinde saklayan, Rust ile yazılmış sanal dosya sistemi.**

`d` → database · `peg` → fotoğraf/görüntü fikri · `fs` → filesystem

[English documentation](docs/README.en.md) · [Disk biçimi](docs/FORMAT.md) · [Geliştirme planı](docs/ROADMAP.md)

Genişlik ve yüksekliği sen seçersin. dpegfs dosya tablosunu ve dosya içeriklerini görüntünün **gerçek RGB kanal değerlerine** yazar. Ortaya çıkan dosya standart bir PNG olarak açılabilir; tekrar dpegfs ile okunduğunda içindeki dosyalar özgün baytlarıyla geri alınır. Veriler PNG açıklamalarında, özel ek parçalarda veya dosyanın sonuna eklenen bir arşivde tutulmaz.

**Durum:** `0.1.0`, çalışan deneysel CLI ve Rust kütüphanesi. İşletim sistemine sürücü olarak bağlanma, POSIX dosya sistemi veya veritabanı sorgu motoru henüz yok. Dosyalar aşağıdaki komutlarla yönetilir.

## Yer kazancı konusunda gerçekçi beklenti

Pikseller yeni bir depolama ortamı sunar; kendi başına ek bilgi kapasitesi yaratmaz. RGB8 görüntüde her piksel **3 bayt** taşır:

```text
ham kapasite = genişlik × yükseklik × 3
kullanılan alan = 128 bayt başlık + dosya tablosu + saklanan dosya içerikleri
boş alan = ham kapasite − kullanılan alan
```

1024 × 1024 görüntü = **3.145.728 bayt = 3 MiB** ham kapasite. Dosya tablosu da bu alana dahildir. `--size` toplam piksel kapasitesini hedefler; net kullanıcı alanını veya PNG'nin bilgisayardaki dosya boyutunu belirtmez.

Varsayılan `auto` modu, dosyayı zlib ile sıkıştırır ve yalnızca daha küçükse sıkıştırılmış halini saklar. Böylece sıkıştırılabilen dosyaların özgün toplamı ham kapasiteyi aşabilir. PNG ayrıca piksel akışını kayıpsız sıkıştırır; boş pikseller sıfır olduğu için boş disk çok küçük görünebilir. Bu durum sınırsız kapasite anlamına gelmez. JPEG, ZIP, video, şifreli veya rastgele verilerde büyük kazanç beklenmemelidir; sonuç özgün dosyadan büyük de olabilir.

**Neden PNG?** [PNG standardı](https://www.w3.org/TR/png/) kanal değerlerini kayıpsız korur. Geleneksel kayıplı JPEG bu baytları değiştirebilir. `peg` ismin bir parçasıdır; v1 disk biçimi PNG'dir. Oluşturulan görüntü bir veri görüntüsüdür; mevcut bir fotoğrafın görünümünü koruyan steganografi uygulanmaz.

## Kurulum

[Rust ve Cargo](https://www.rust-lang.org/tools/install) **1.89 veya üstü** gerekir. C kütüphanesi veya sistem PNG bağımlılığı gerekmez.

Depoyu klonlayıp derle:

```sh
git clone https://github.com/crypto-su/dpegfs.git
cd dpegfs
cargo build --release --locked
```

Çalıştırılabilir dosya macOS/Linux'ta `target/release/dpegfs`, Windows'ta `target\release\dpegfs.exe` olur. Her yerden `dpegfs` komutunu kullanmak için:

```sh
cargo install --path . --locked
dpegfs --help
```

Kurulum yapmadan da `cargo run --locked -- <komut>` kullanılabilir. Henüz crates.io yayını veya hazır ikili sürüm varsayılmaz.

## Hızlı başlangıç

Proje klasöründe aşağıdaki akış README'nin bir kopyasını PNG içine yazıp geri çıkarır. Hedef klasörün yeni veya boş olması gerekir; mevcut dosyalar varsayılan olarak ezilmez.

```sh
mkdir demo-output

# Yaklaşık 16 MiB ham piksel kapasiteli bir disk oluştur.
cargo run --locked -- create demo-output/disk.png --size 16MiB

# README'yi disk içindeki docs/README.md yoluna yaz.
cargo run --locked -- put demo-output/disk.png README.md --name docs/README.md

# İçeriği ve gerçek kullanım miktarını görüntüle.
cargo run --locked -- list demo-output/disk.png
cargo run --locked -- info demo-output/disk.png

# Dosyayı geri çıkar ve tüm dosyaların bütünlüğünü doğrula.
cargo run --locked -- get demo-output/disk.png docs/README.md demo-output/restored.md
cargo run --locked -- verify demo-output/disk.png
```

macOS/Linux'ta birebir karşılaştırma:

```sh
cmp README.md demo-output/restored.md
```

PowerShell'de iki SHA-256 değerini karşılaştır:

```powershell
Get-FileHash README.md, demo-output/restored.md -Algorithm SHA256
```

Başarılı `verify` çıktısı: `OK: verified disk structure and all 1 file(s)`.

## Komutlar

Aşağıdaki örnekler `cargo install --path . --locked` sonrasındaki kullanımı gösterir.

| İşlem | Komut |
| --- | --- |
| Varsayılan 1024 × 1024 disk | `dpegfs create disk.png` |
| Piksel boyutlarını belirle | `dpegfs create disk.png --width 1920 --height 1080` |
| Toplam kapasiteyi belirle | `dpegfs create disk.png --size 16MiB` |
| Dosya ekle, kaynak adını kullan | `dpegfs put disk.png report.pdf` |
| Disk içindeki yolu belirle | `dpegfs put disk.png report.pdf --name documents/report.pdf` |
| Mevcut sanal dosyayı değiştir | `dpegfs put disk.png report.pdf --name documents/report.pdf --overwrite` |
| Dosya düzeyinde sıkıştırmayı kapat | `dpegfs put disk.png data.bin --compression none` |
| Listele | `dpegfs list disk.png` veya `dpegfs ls disk.png` |
| JSON liste | `dpegfs list disk.png --json` |
| Dosyayı çıkar | `dpegfs get disk.png documents/report.pdf restored.pdf` |
| Mevcut hedef dosyayı değiştir | `dpegfs get disk.png documents/report.pdf restored.pdf --overwrite` |
| Özgün baytları stdout'a yaz | `dpegfs cat disk.png notes.txt` |
| Yeniden adlandır / taşı | `dpegfs rename disk.png notes.txt archive/notes.txt` |
| Dosya sil | `dpegfs remove disk.png archive/notes.txt` |
| Kapasite ve fiziksel boyut | `dpegfs info disk.png` veya `dpegfs info disk.png --json` |
| Tüm dosyaları doğrula | `dpegfs verify disk.png` |

`rename` için `mv`, `remove` için `rm` takma adları da kullanılabilir. Komuta özel yardım: `dpegfs put --help`.

- `--size` ile `--width/--height` birlikte kullanılamaz; genişlik ve yükseklik birlikte verilmelidir. `B`, `KiB`, `MiB`, `GiB` ile ondalık `KB`, `MB`, `GB` birimleri ve tam sayılar kabul edilir. Geçerli birim, uygulamanın boyut sınırını kaldırmaz.
- Kapasiteden boyut seçimi kareye yakın bir görüntü üretir; tam satır ve piksele yuvarlama nedeniyle gerçek kapasite biraz büyük olabilir. Üst sınıra çok yakın değerlerde yuvarlanan görüntü de sınıra uymalıdır.
- Yollar büyük/küçük harfe duyarlı UTF-8 dizgeleridir. `docs/readme.txt` gibi dizinler örtük olarak oluşur; ayrıca `mkdir` gerekmez. Boş dizinler saklanmaz.
- Mutlak yollar, `.`/`..`, boş yol parçaları, `\`, `:`, kontrol karakterleri ve dosya/dizin adı çakışmaları reddedilir. Ad başına en fazla 1024 UTF-8 baytı kullanılır.
- `get` açık bir yerel çıktı yolu ister; onun üst klasörü önceden bulunmalıdır. Toplu çıkarma, dizin içe aktarma ve izin/tarih/symlink saklama yoktur.
- `--compression none`, dosya düzeyindeki zlib'i kapatır; PNG'nin kendi kayıpsız sıkıştırması devam eder.
- `cat` çıktısı ikili olabilir. Disk görüntüsüne stdout yönlendirmesi yapma; kabuk, program başlamadan hedefi kesebilir. Dosya çıkarmak için korumalı `get` komutunu kullan.
- Başarılı komut `0`, çalışma hatası `1`, geçersiz CLI kullanımı `2` çıkış kodu döndürür. Hatalar stderr'e yazılır.

## Kapasiteyi doğru okumak

`info --json` alanları:

| Alan | Anlam |
| --- | --- |
| `capacity_bytes` | Genişlik × yükseklik × 3 |
| `metadata_bytes` | 128 bayt başlık + JSON dosya tablosu |
| `stored_bytes` | Ham veya zlib olarak saklanan dosyaların toplamı |
| `used_bytes` | Metadata + saklanan içerik |
| `free_bytes` | Yeni içerik **ve yeni metadata** için kalan alan |
| `original_bytes` | Dosyaların sıkıştırma öncesi toplamı |
| `png_bytes` | PNG'nin bilgisayardaki gerçek dosya uzunluğu |

`png_bytes`, dosya sisteminin ayırdığı blok miktarı değildir. `PNG vs originals` fiziksel PNG boyutunu özgün dosyaların toplamıyla karşılaştırır; negatif yüzde PNG'nin daha büyük olduğunu gösterir. Görüntü boyutları v1'de sabittir. Silme alanı hemen geri kazandırır; ayrı bir sıkıştır/defrag komutu gerekmez.

## Ölçülmüş örnek

Ölçümü tekrar çalıştır:

```sh
cargo run --locked --release --example measure
```

Örnek, her veri türünü ayrı 1024 × 1024 diskte saklar, baytları geri okuyup karşılaştırır ve geçici dosyaları temizler. Aynı zlib ayarlarıyla doğrudan sıkıştırmayı da karşılaştırmaya katar.

| Veri | Özgün bayt | Doğrudan zlib | dpegfs PNG | Pikseldeki içerik |
| --- | ---: | ---: | ---: | ---: |
| Tekrarlı metin | 1.048.576 | 3.130 | 3.555 | 3.130 |
| Deterministik sözde rastgele veri | 1.048.576 | 1.048.752 | 1.051.761 | 1.048.576 |

Bu sonuçlar mevcut `Cargo.lock` ile alınan örnek ölçümdür; tüm dosyalar için bir oran vaat etmez. Metindeki büyük kazanç sıkıştırmadan gelir. Rastgele örnekte PNG yaklaşık %0,30 daha büyüktür; doğrudan zlib de bu örneklerde PNG'den daha küçüktür. Projenin ilk hedefi **ölçülebilir, doğrulanabilir bir görüntü dosya sistemi** kurmaktır.

## Rust kütüphanesi olarak kullanım

Bu depoyu yerel bağımlılık olarak ekle:

```toml
[dependencies]
dpegfs = { path = "../dpegfs" }
anyhow = "1"
```

```rust
use dpegfs::{Compression, Disk};

fn main() -> anyhow::Result<()> {
    let mut disk = Disk::create("my-disk.png", 1024, 1024)?;
    disk.put("notes/hello.txt", b"Hello, pixels!", Compression::Auto, false)?;
    assert_eq!(disk.read("notes/hello.txt")?, b"Hello, pixels!");
    disk.rename("notes/hello.txt", "archive/hello.txt")?;
    disk.verify()?;
    println!("{:?}", disk.info()?);
    drop(disk); // Yazıcı kilidi bırakılır.

    let snapshot = Disk::open_read_only("my-disk.png")?;
    snapshot.extract("archive/hello.txt", "restored.txt", false)?;
    Ok(())
}
```

`Disk::open` mevcut diski yazılabilir açar. `Disk::open_read_only` yan dosya oluşturmayan bir anlık görüntü okur; sonraki değişiklikleri görmek için yeniden açılır. `put`, `put_file`, `remove` ve `rename` başarılı döndüğünde değişiklik PNG'ye kaydedilmiştir; ayrıca `save` çağrısı gerekmez. API belgeleri: `cargo doc --no-deps --open`.

## Bütünlük, kayıt davranışı ve sınırlar

- Başlık ve kullanılan gövde ayrı SHA-256 özetleriyle korunur. Her dosyanın özgün baytları ayrıca SHA-256 ile doğrulanır. Okuma sırasında boyutlar, yollar, ofsetler, dosya sayıları ve boş alanın sıfırları denetlenir; `verify` her dosyayı da açıp kontrol eder. Özetler bozulmayı tespit eder; onarım, kimlik doğrulama veya şifreleme sağlamaz.
- Değişiklik önce bellekte hazırlanır ve kapasite kontrol edilir. Tam PNG aynı klasörde geçici dosyaya yazılıp diske senkronize edilir; ardından atomik dosya değiştirme ile yayımlanır. Kapasite veya hazırlama/yazma hatası eski görüntüyü değiştirmez. Normal kayıt tüm PNG'yi yeniden yazar.
- Yerel dosya sisteminde dpegfs yazıcıları `disk.png.dpegfs.lock` üzerinde işletim sistemi kilidi kullanır. İkinci yazıcı beklemek yerine hata alır. Kilit işlem kapanınca bırakılır; boş yan dosya kalabilir ve veri içermez. İşlem çalışırken kilit dosyasını silme. PNG'nin tek başına kopyalanması yeterlidir.
- Okuyucular eski veya yeni tam görüntünün anlık kopyasını görür. Başka programların görüntüyü değiştirmesi, ağ dosya sistemlerinin farklı kilit/rename davranışları ve güç kesilmesine karşı tam dayanıklılık kapsam dışıdır. v1 üst klasörü `fsync` etmez; bir journaling sistemi değildir.
- Tam piksel görüntüsü ve dosya içerikleri bellekte tutulur. Bir görüntü en fazla **256 MiB ham RGB**, tek dosya en fazla **256 MiB özgün veri**, her boyut en fazla **16.384 piksel**, disk en fazla **10.000 dosya**, dosya tablosu en fazla **16 MiB** olabilir. Bellek kullanımı ham kapasitenin birkaç katına çıkabilir; bunlar üretim diskleri için ölçek hedefleri değildir.
- Disk ve çıktı yollarında sembolik bağlantılar kabul edilmez. Unix'te birden çok hard link'i olan görüntüler yazılabilir açılamaz; diğer sistemlerde de disk için tek normal yol kullan. Üst klasöre sembolik bağlantı kullanılması normalleştirilir.
- Görüntüyü yeniden boyutlandırma, kırpma, renk düzeltme veya JPEG'e dönüştürme veriyi bozar. Sosyal medya ve mesajlaşma uygulamalarının görüntü dönüştürmesi de aynı etkiyi yapabilir; aktarırken özgün dosyayı gönder. Hassas veri için şifreleme yoktur. Silme güvenli fiziksel silme değildir. Deneysel sürümde önemli dosyaların tek kopyasını tutma.

## Geliştirme

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo test --locked --doc
```

Testler ikili/Unicode/boş dosyaları, gerçek piksel yerleşimini, sıkıştırmayı, kapasite taşmasını, başarısız kayıtları, eşzamanlı yazıcıları, bozulmuş başlık ve payload'ı, eksik PNG/zlib verisini, CLI akışını ve yol denetimlerini kapsar. GitHub Actions Linux/macOS/Windows testleri ile minimum Rust sürümü kontrolü için yapılandırılmıştır; yereldeki test sonucu tüm platformların çalıştırıldığı anlamına gelmez.

```text
src/lib.rs          Sanal dosya sistemi API'si ve işlemler
src/codec.rs        Piksel biçimi, PNG, SHA-256 ve zlib doğrulama
src/storage.rs      Atomik kayıt ve işletim sistemi yazıcı kilidi
src/main.rs         CLI
tests/             Dosya sistemi ve CLI entegrasyon testleri
examples/measure.rs Tekrarlanabilir boyut karşılaştırması
docs/FORMAT.md     Sürüm 1 disk biçimi
docs/ROADMAP.md    Uygulanan kapsam ve sonraki aşamalar
```

Lisans: [MIT](LICENSE).
