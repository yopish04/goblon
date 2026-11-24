# StudioZea

Aplikasi Windows untuk prepress cetak kaos, dua jenis cetak:

- **DTF**: masukkan gambar → hapus background → crop, ukuran, DPI → halftone (LPI, sudut,
  bentuk) → PNG siap cetak (plus export banyak dan gang sheet).
- **Sablon (screen printing)**: separasi warna → satu film per tinta (underbase, warna,
  highlight putih) dengan tanda registrasi, simulasi hasil cetak dan urutan cetak.

Saat dibuka, **halaman awal** menanyakan jenis cetaknya (DTF atau Sablon). Pilihan bisa
diingat ("Ingat pilihan saya"), dan halaman itu selalu bisa dibuka lagi lewat logo
StudioZea. Di dalam editor, saklar **DTF | Sablon** di bar atas memindah jenis cetak kapan
saja; gambar, crop, coretan dan hasil AI tetap.

## Sablon (separasi warna)

Cara kerja mengikuti software separasi garmen (Separation Studio, UltraSeps, T-Seps):
underbase putih di bawah warna pada kaos gelap, warna dicetak dari terang ke gelap, hitam
hampir terakhir, highlight putih terakhir, semua film satu sudut (bawaan 25°, 55 LPI,
titik elips). Bedanya, separasi dihitung dari **model cetak**:

- **Tinta otomatis**: k-means di ruang warna OKLab, lalu dipilih tinta yang bisa
  "mencapai" semua warna desain (sendiri, campur kaos/putih, atau campur dua tinta).
  Dengan cara ini warna yang luasnya kecil (mis. biru di desain yang dominan api) tetap
  mendapat tinta. Tinta diambil dari anggota cluster yang lebih jenuh: rata-rata sebuah
  cluster lebih kusam dari warna yang diwakilinya, dan itu penyebab utama warna "turun"
  setelah separasi. Warna yang sama dengan kaos tidak perlu tinta.
- **Model cetak**: kaos → underbase putih → tinta berurutan, masing-masing dengan
  opasitas (plastisol biasa ±60 %: tanpa underbase, merah di kaos hitam tampak gelap dan
  kusam). Untuk setiap warna, komputer mencari kombinasi underbase + tinta yang hasil
  cetaknya paling mirip desain. Karena itu underbase otomatis padat di bawah warna terang
  dan jenuh, tipis di bawah warna gelap, dan highlight putih hanya dipakai di bagian yang
  memang perlu (terlalu banyak membuat warna pucat seperti kapur).
- **Titik di atas titik**: karena semua film satu sudut dan satu LPI, titik-titiknya
  bertumpuk di tempat yang sama: film dengan cakupan 30 % mencetak di dalam titik film
  60 %. Model Simulasi menghitung warna rata-rata dengan cara itu (bukan seolah titik
  tersebar acak), sehingga hasil cetak dan angka kemiripan sesuai kenyataan. Setiap warna
  yang ada di desain dihitung sendiri (ketelitian 7 bit per kanal) tanpa interpolasi
  antar-solusi, supaya gradasi tidak bergaris atau bercincin.
- **Metode**: *Simulasi* (titik halftone per tinta, untuk desain foto/gradasi), *Index*
  (kotak kecil, satu tinta per titik dengan error diffusion, tanpa moire), *Spot* (warna
  rata tanpa titik, untuk desain vektor). Underbase di-*choke* (bawaan 0,1 mm) supaya
  putihnya tidak mengintip di tepi. Titik/lubang di bawah 0,15 mm dibersihkan.
- **Pratinjau jujur**: tampilan *Hasil cetak* adalah simulasi dari titik film yang
  sebenarnya dengan model yang sama, bukan gambar asli. **Kemiripan** menunjukkan selisih
  warna rata-rata desain dan simulasi (ΔE; di bawah 3 hampir tidak terlihat, di bawah 6
  mirip). Pilihan *Film 1, 2, …* di toolbar menampilkan film satu per satu (hitam = tinta).
- **Set tinta**: *Otomatis dari desain* (seperti di atas) atau set tetap berisi tinta
  standar workshop, disesuaikan dengan warna kaos: *6 warna dasar* (kuning, merah,
  magenta, biru, cyan, hijau; + abu di kaos gelap, + hitam di kaos putih/berwarna),
  *6 warna dasar, abu dicetak dulu* (abu sebelum underbase, kaos gelap), *Hemat*
  (kuning, merah, biru) dan *Gelap / api / vintage* (emas, oranye, merah, marun, abu).
  Kaos gelap otomatis mendapat underbase dan highlight putih, tinta yang warnanya sama
  dengan kaos tidak dipakai. Film tetap dihitung dengan model cetak, jadi tidak perlu
  "Select Color" dan "Boost" manual. *Tinta saya*: daftar tinta (urutan, posisi
  underbase, boost) yang disimpan sendiri, mis. sesuai stok gudang.
- **Film kosong**: tinta yang mencetak kurang dari 0,3 % luas desain (bisa diatur) tidak
  dibuatkan film; tinta lain dihitung ulang tanpa tinta itu (hemat screen). Di daftar
  tinta ditandai "tidak dipakai".
- **Gaya titik** (Simulasi): *Satu sudut* (bawaan; titik bertumpuk, paling aman dari
  moire), *Roset* (tiap film sudut berbeda, 15–75° dari underbase; model menghitung titik
  yang jatuh bebas) dan *Interlock* (warna berselang menurut hue memakai titik yang
  dibalik, jadi warna bersebelahan dicetak berdampingan, tidak menindih: gradasi antar
  warna seperti toska → biru jadi bersih). Bandingkan angka Kemiripan untuk memilih.
- **Expand warna** (px): film warna dibuat sedikit lebih lebar (trapping), selain choke
  underbase. **Boost warna** (%): warna dibuat lebih jenuh dari desain; **boost per
  tinta** (angka di daftar tinta): titik tinta itu lebih rapat, seperti "Boost Red".
- **Tab Warna**: metode, set tinta, jumlah warna (1–12, untuk Otomatis), daftar tinta
  (urutan = urutan cetak; klik kotak warna untuk mengganti tinta sesuai stok, ↑ ↓
  mengubah urutan, termasuk underbase, × menghapus, *+ Tinta*, *Otomatis lagi* /
  *Kembali ke set*, *Simpan sbg Tinta saya*), underbase (otomatis/ya/tidak), choke,
  highlight putih, expand warna, film kosong, boost warna, gaya titik, LPI dan sudut
  film, bentuk titik, Lindungi teks (tulisan dicetak padat dengan satu tinta), tepi film.
- **Export film**: `nama.png` (simulasi hasil cetak), `nama_01_underbase-putih.png`,
  `nama_02_...png` … (hitam di atas putih sesuai DPI, dengan 4 tanda registrasi dan nama
  film di tepi), dan `nama_urutan-cetak.txt` (dengan sudut film dan film yang titiknya
  dibalik). Tekstur vintage ikut melubangi semua film.

Engine: `engine/src/sablon.rs`, tes `engine/tests/sablon.rs`. CLI:
`studiozea-cli desain.jpg hasil.png --lebar-cm 30 --sablon 6 [--metode simulasi|index|spot]
[--set-tinta auto|dasar|dasar-abu-dulu|hemat|gelap] [--gaya-titik satu-sudut|roset|interlock]
[--expand 2] [--buang 0.3] [--boost 10] [--lpi-film 55] [--sudut 25]` menulis simulasi dan
film-filmnya.

## Download (Windows 10/11)

Halaman download publik: **https://github.com/yopish04/studiozea-update/releases/latest**.
Setelah terpasang, aplikasi memberi tahu sendiri bila ada versi baru (klik **Update
sekarang**); lihat [docs/UPDATE.md](docs/UPDATE.md) untuk cara merilis versi baru.

Build uji juga ada di tab **[Releases](../../releases)** repo ini:

| File | Untuk |
|---|---|
| `StudioZea_x.y.z_x64-setup.exe` | installer (disarankan) |
| `StudioZea_x.y.z_x64_en-US.msi` | installer MSI |
| `StudioZea-portable.exe` | langsung jalan tanpa install |

Installer dibuat oleh GitHub Actions (`.github/workflows/windows.yml`), **tidak** lagi di
setiap push (runner Windows memakai menit Actions 2× di repo privat). Push biasa hanya
menjalankan uji golden engine di Linux. Untuk membuat installer + release baru:

- tab **Actions → Build Windows app → Run workflow** (pilih branch) → release `build-N`, atau
- push tag versi, mis. `git tag v0.2.0 && git push origin v0.2.0` → release `v0.2.0`.

Sebelum installer dibuild, uji golden engine dijalankan di Linux **dan** Windows.
Karena installernya belum ditandatangani (code signing), Windows SmartScreen bisa
menampilkan "Windows protected your PC". Klik **More info → Run anyway**.

## Cara pakai

**Tata letak**: bar atas berisi *Buka gambar*, menu **Proyek** (buka/simpan), menu
**Preset**, Undo/Redo, tab **Desain / Export banyak / Gang sheet** dan tombol cek update.
Di kanan ada panel dengan tab ikon **Gambar · Kaos · Ukuran · Halftone · Tekstur**; tombol
**Export PNG** dan peringatan selalu terlihat di bawah panel. Penjelasan tiap pengaturan
muncul saat kursor diarahkan ke ikon **(?)**. Muat di layar laptop 1366 × 768.
**Pengaturan** (ikon ⚙ di kanan atas): tema, bahasa, kehalusan AI dan perangkat AI.
**Bahasa**: English (bawaan) atau Bahasa Indonesia;
tersimpan. Hanya teks yang berganti: nilai yang disimpan di proyek dan preset (mis. `hitam`,
`bulat`) tetap sama, jadi file lama tetap terbuka, dan hasil cetak tidak berubah.
**Tema**: *Ikuti Windows* (bawaan), *Terang* (Porcelain + Neon Orange)
atau *Gelap* (Forest Graphite + Acid Mint); tersimpan. Hanya tampilan yang berubah: area
pratinjau tetap abu-abu netral tanpa warna (abu-abu muda di tema terang, abu-abu tua di tema
gelap; warna di sekitar desain memengaruhi cara mata melihat warnanya) dan hasil cetak sama
persis.

0. **Preset saya & proyek** (menu *Preset* dan *Proyek* di bar atas):
   - Pengaturan terakhir (kaos, ukuran, DPI, halftone, tekstur…) **otomatis diingat** saat
     aplikasi dibuka lagi (file `studiozea.json` di folder pengaturan Windows,
     `%APPDATA%\com.studiozea.dtf`). File rusak disimpan sebagai `studiozea.json.rusak` dan
     aplikasi mulai dengan pengaturan default.
   - **Preset saya**: *Simpan pengaturan sebagai preset…* menyimpan semua pengaturan dengan nama sendiri
     (mis. "Kaos hitam vintage 30 cm"); pilih dari daftar untuk memakainya lagi; × = hapus;
     *Reset* = kembali ke default. Crop tidak ikut preset (khusus per gambar).
   - **Proyek** (`.szproj`): *Simpan proyek…* menyimpan lokasi gambar + crop + semua
     pengaturan; *Buka proyek…* (atau seret file `.szproj` ke jendela) membuka semuanya lagi.
     Kalau gambar sudah dipindah, dicari file dengan nama sama di folder proyek.
1. **Gambar**: buka atau seret file (PNG/JPG/WebP/TIFF). Auto-trim memotong ke area
   desain.
   **Perbesar AI** (untuk gambar kecil / blur, mis. desain kiriman pelanggan): AI
   Real-ESRGAN Compact membesarkan gambar 4× (atau 2× bila cukup) dan menajamkan garis,
   tulisan dan tekstur, langsung di komputer ini tanpa internet (±7–10 detik untuk gambar
   900×1200 px di prosesor 4 inti). Tombolnya disorot, dan peringatan "resolusi rendah"
   memberi tautan cepat, bila resolusi efektif di bawah 150 DPI. *Kehalusan AI* (menu ⚙):
   **Halus** (bersihkan noise / kotak-kotak JPG), **Seimbang** (bawaan), **Tekstur**
   (pertahankan tekstur halus / aus). Hasilnya langsung dipakai (crop, coretan penghapus dan
   pengaturan ikut) dan disimpan di cache aplikasi (`%LOCALAPPDATA%\com.studiozea.dtf\ai`,
   24 hasil terakhir), bukan file baru di samping file asli: gambar, skala dan kehalusan
   yang sama lagi langsung diambil dari cache. **Ctrl Z** kembali ke gambar asli, **Ctrl Y**
   ke hasil AI lagi; *pakai gambar asli* juga bisa. File baru (`nama_ai4x.png`) baru dibuat
   saat proyek disimpan, di samping file proyek. File asli tidak pernah berubah. Ada progres
   dan tombol *Batal*. AI menebak detail: tulisan kecil tetap dicek dulu sebelum cetak.
   **Perangkat AI** (menu ⚙): *Otomatis* (bawaan) memakai kartu grafis (AMD RX, NVIDIA RTX/GTX,
   Intel; lewat DirectX 12 / Vulkan, tanpa driver tambahan) bila ada, dan jauh lebih cepat.
   Saat pertama dipakai, kartu grafis dites dulu: hasilnya dibandingkan dengan prosesor dan
   hanya dipakai bila sama. Kartu yang gagal tes atau bermasalah di tengah jalan (driver,
   memori penuh, terputus) tidak dipakai lagi (diingat di `ai_gpu.json`; *coba lagi* untuk
   mengetes ulang) dan sisa pekerjaan diselesaikan prosesor dengan hasil yang sama.
   *Prosesor*: selalu prosesor. Pesan hasil menyebut perangkat yang dipakai.
   Crop seperti Photoshop: mode **Asli** → **Crop** → kotak muncul di seluruh
   gambar; tarik sudut/sisi untuk mengubah ukuran (Shift di sudut = rasio tetap), tarik
   bagian dalam untuk menggeser, tarik di luar kotak untuk kotak baru. **Enter** (atau klik
   ganda di dalam kotak) = terapkan, **Esc** = batal, **Hapus crop** = kembali utuh.
   **Putar / cermin**: ⟲ / ⟳ memutar desain 90° (ukuran cetak fisik tetap: lebar dan tinggi
   bertukar), **Cermin** membalik kiri-kanan. Diterapkan sebelum halftone, jadi sudut raster
   tetap seperti yang diatur. Ikut tersimpan di proyek.
   **Penghapus** (tombol di toolbar, di tab Gambar, atau tombol **E**): tarik kuas di gambar untuk
   menghapus bagian yang tidak ingin dicetak. *Kembalikan* = kuas yang mengembalikan
   piksel asli (mis. bagian desain yang ikut terhapus oleh hapus background). Ukuran kuas
   dengan slider atau **[** / **]** (dalam piksel gambar, juga ditampilkan dalam mm cetak).
   Bisa dipakai di tampilan Asli, Tanpa latar, Hasil maupun Split; setiap coretan satu
   langkah Ctrl Z. Tepi kuas tajam (DTF tidak mencetak semi-transparan). Coretan ikut
   tersimpan di proyek, tidak ikut preset. **Esc** = selesai.
2. **Kaos** (hapus background): pilih kaos hitam, putih, atau warna. *Otomatis*: di kaos hitam,
   hitam jadi transparan; di kaos putih, putih jadi transparan. Latar polos lain (putih
   kertas, navy, marun, kuning…) dideteksi dari tepi gambar dan dihapus otomatis di semua
   jenis kaos. *Paksa*: paksa hapus latar dari warna tepi.
   **Kaos otomatis** (bawaan nyala): saat gambar dibuka, jenis kaos dipilih sesuai warna
   latar desain (latar hitam → kaos hitam, putih → kaos putih, navy/marun/… → kaos warna
   itu), karena di DTF latar desain = warna kaos. Gambar transparan tidak mengubah pilihan.
   Tetap bisa diganti manual; proyek (.szproj) memakai kaos yang tersimpan di proyek. Bila
   kaos berganti, pesan di bawah panel punya tombol **Batalkan** (kembali ke kaos sebelumnya,
   mis. saat mengerjakan pesanan kaos hitam lalu membuka desain berlatar putih); kaos yang
   sudah sama tidak memunculkan pesan.
3. **Ukuran** (resize) (juga **Margin export** dalam mm: ruang transparan di sekeliling
   file PNG, ikut di Export dan Export banyak; pratinjau tidak berubah): lebar atau tinggi dalam **cm, inci atau piksel** (rasio tetap),
   tombol ukuran cepat (Logo dada 9 cm, Saku 10 cm, A5, Anak 20 cm, A4, A3, 30/35/40 cm),
   DPI 150–600, dan **metode resize**: *Lanczos* (paling tajam, default) atau *Bicubic*
   (lebih halus, seperti Photoshop). Ditampilkan ukuran hasil dalam px/cm/inci dan
   resolusi efektif gambar sumber. Urutan: hapus background → crop → trim → resize.
4. **Halftone**: preset (Referensi 35 LPI rosette, Tajam, Foto/FM, Vintage grain, GCR,
   Pop art titik besar/bintang, Retro segitiga/garis). **Pola** dipilih dari kotak contoh
   yang dirender langsung oleh engine: 13 bentuk (bulat, elips, kotak, berlian, garis,
   silang, gelombang, arsir, titik kasar analog, **bintang**, **segitiga**, **rantai**).
   **Ukuran (LPI)** 3–150 dengan slider (3–10 = titik besar pop art, jarak titik dalam mm
   ditampilkan). **Sudut** bebas 0–360° (Auto = 22,5° seperti referensi). Di kaos
   putih/berwarna: **Rosette** (C/Y sudut, M sudut + 45°) atau **titik di atas titik**.
   Grid Auto/Rapi/Presisi, channel hitam K, tepi tajam.
   Pola lain (menu **Pola**): *FM / stokastik*, *Grain* halus/sedang/kasar, dan:
   - **Halftone interlock (rapi)**: titik bulat seragam, semua warna di grid yang sama
     (45°, bisa diubah), terang dan gelap saling mengunci seperti papan catur di 50 %;
     di ujung nada titik tidak diganti bintik acak (FM) melainkan titik terkecil yang
     masih tercetak, jadi seluruh desain tetap rapi.
   - **Dither Bayer (teratur)**: pola kotak-kotak kecil gaya retro/pixel.
   - **Dither difusi (Floyd–Steinberg)**: titik menyebar mengikuti detail, tajam di tepi.
   Dither memakai titik minimal 0,15 mm (bukan 1 piksel seperti dither Photoshop), jadi
   tetap aman untuk DTF.
   **Halftone Nyala / Mati**: *Mati* = tanpa titik dan tanpa setengah transparan: tiap piksel
   dicetak penuh dengan warna aslinya (bila minimal 50 % tidak transparan) atau tidak sama
   sekali; bintik/lubang < 0,15 mm dibersihkan. Cocok untuk desain yang hanya memakai
   tekstur vintage. *Buang warna mirip kaos* (default 12 %): bayangan hampir hitam dan noise
   JPG di kaos hitam tidak dicetak (hemat tinta dan underbase, tampilan sama). Tekstur vintage bisa dipakai dengan atau tanpa halftone.
   **Lindungi teks** (opsi, bawaan mati): AI (detektor teks PP-OCRv4 di dua skala: tulisan
   kecil dan logo besar; ±1 detik sekali per gambar, tanpa internet) mencari tulisan di desain; hurufnya dicetak **padat dengan warna
   aslinya** (seperti spot color), bagian lain tetap halftone, jadi tulisan kecil tidak pecah
   jadi titik dan tidak perlu dibuat ulang. Per kata, warna dipisah dua (huruf vs latar;
   bagian yang menyentuh tepi kotak kata = latar), jadi latar di sekitar tulisan tetap
   halftone. *Teks dicetak*: **Padat** (seperti di atas; paling tajam) atau **Titik halus**:
   huruf tetap halftone dengan pola dan bentuk titik yang sama tapi lebih rapat (2× LPI desain,
   45–75 LPI, maks. DPI/4), jadi huruf bergradasi / metalik tetap terbaca tanpa terlihat
   seperti foto, dan tetap 8 warna murni. Titik halus hanya untuk pola AM dan Interlock (pola
   lain sudah bertitik halus, teksnya padat). Tulisan yang terlewat (sangat kecil, miring) atau
   bagian yang salah diatur dengan **Kuas teks** (tab Halftone → *Area teks*, atau tombol di
   toolbar pratinjau): *Teks* / *Bukan teks*; terpisah dari Penghapus. Selama kuas teks aktif
   (atau tombol mata ditekan) area teks diberi **arsir biru** di pratinjau (hanya tampilan).
   Tekstur vintage tetap mengenai tulisan. Juga berlaku di Export banyak (deteksi per
   gambar). Mati = hasil sama persis seperti tanpa fitur ini.
   **Area halftone**: *Semua* (semua warna jadi titik warna murni, seperti referensi) atau
   *Gradasi saja*: blok warna tetap solid dengan warna asli, titik hanya di bagian yang
   memudar ke warna kaos (glow, bayangan, asap, pinggiran). Slider 1–100 % mengatur luas
   area titik. Alpha tetap 0/255 (underbase putih tajam).
5. **Tekstur vintage**: pilih tekstur dari pustaka (thumbnail). Kotak **+ Tambah…** memilih
   satu atau banyak file sekaligus (PNG/JPG/TIFF/WebP/BMP, maks. 300 MB per file); file disalin
   ke **pustaka saya** (★, folder `%APPDATA%\com.studiozea.dtf\tekstur`) dan muncul lagi setiap
   aplikasi dibuka, jadi tidak perlu upload ulang. File yang sama (nama + ukuran) tidak
   disalin dua kali. Tombol × di tekstur ★ menghapusnya dari pustaka (klik "Hapus?" untuk
   memastikan).
   **Tumpuk sampai 3 tekstur**: setelah memilih tekstur, klik **+ Lapisan** lalu pilih
   tekstur kedua (dan ketiga). Tiap lapisan punya pengaturan sendiri (cara, kekuatan,
   ukuran, putar, geser); lapisan baru mulai diputar/digeser supaya tekstur yang sama tidak
   bertumpuk persis. Lubang semua lapisan digabung, pudar ditumpuk. Pilih **Tanpa tekstur**
   pada sebuah lapisan untuk membuangnya.
   Cara: *Lubang* (area gelap tekstur jadi lubang di tinta), *Campur* (lubang + pudar,
   paling alami), *Pudar* (nada memudar saja). Slider Kekuatan, Pudar dan Ukuran, putar
   90°, cermin, balik, geser, tombol *Acak* (variasi acak). Titik dan lubang tetap ≥ 0,15 mm.
   **Menambah tekstur ke pustaka**: upload file ke folder [`textures/`](textures/) di
   GitHub, build berikutnya otomatis menampilkannya (lihat `textures/README.md`).
6. **Export banyak (batch)** (tab di bar atas): pilih banyak gambar sekaligus (**Pilih gambar…**) dan folder
   hasil, lalu **Mulai export**. Semua memakai pengaturan di atas (kaos, DPI, halftone,
   tekstur); background dihapus otomatis per gambar dan auto-trim ikut, crop dan penghapus
   tidak ikut. Ukuran: *Lebar tetap* (lebar dari tab Ukuran) atau *Muat dalam kotak*, mis.
   30 × 40 cm: setiap desain dibesarkan sebesar mungkin tanpa melewati lebar maupun
   tingginya. **Perbesar AI** (opsi): gambar dengan resolusi efektif di bawah 150 DPI
   diperbesar dulu dengan AI. Gambar diproses satu per satu (ringan), ada progres dan tombol **Hentikan**;
   file rusak dilewati dan dilaporkan. Nama file yang sudah ada tidak ditimpa (`-2`, `-3`…).
7. **Export PNG**: RGBA 8-bit dengan DPI tersimpan. File selalu dicek dulu: tidak boleh
   ada piksel semi-transparan, warna tidak murni, atau titik di bawah 0,15 mm.

**Gang sheet** (tab *Gang sheet* di kanan atas): menyusun banyak desain siap cetak dalam satu
lembar film supaya hemat. **Tambah gambar…** (bisa pilih banyak sekaligus, atau seret ke
jendela): file PNG hasil Export / Export banyak dipakai apa adanya; **gambar mentah** (JPG,
PNG dengan latar atau tepi setengah transparan, TIFF, WebP) diproses otomatis dengan
pengaturan tab Desain (lebar cetak, DPI, kaos, latar, halftone, tekstur; tanpa crop /
penghapus) seperti satu gambar Export banyak, lalu ditandai *diproses*. Atau tombol
**Desain sekarang** (desain yang sedang dibuka di tab Desain). Isi
jumlah salinan per desain, lalu atur film: lebar (22 inci, 58, 60, 30 cm atau bebas),
panjang *tanpa batas* atau *maksimal* (sisa desain pindah ke lembar berikutnya, file `_1`,
`_2`…), jarak antar desain dan margin tepi (mm), dan *boleh diputar 90°*. Susunan otomatis
mencoba beberapa urutan dan cara memutar, lalu memilih yang paling pendek (paling hemat
film); pratinjau langsung tampil. **Geser desain dengan mouse** di pratinjau untuk menaruhnya
sendiri: desain itu dikunci di posisi tersebut (tanda titik di sudut), yang lain disusun ulang
di sekitarnya; tidak bisa ditaruh di atas desain lain yang dikunci; klik dua kali = lepas,
*Lepas semua* = kembali ke susunan otomatis. File gang sheet memakai susunan yang sama persis.
Piksel desain disalin persis (tidak di-resize), jadi titik halftone tetap sama. Semua desain
harus ber-DPI sama (yang beda ditandai merah dan tidak dipakai).

**Lembar uji cetak** (link di bawah tombol Export): 22×30 cm berisi grid LPI 25–65 ×
7 bentuk titik, tes titik/lubang minimum 0,10–0,40 mm, chart warna, dan gradasi. Cetak di
printer DTF, pres, lalu cuci beberapa kali untuk menentukan LPI, bentuk, dan titik minimum
yang benar-benar tahan di mesin Anda. Dari command line:
`studiozea-testsheet hitam lembar.png 300`.

Tampilan seperti Photoshop: **Asli / Tanpa latar / Hasil / Split**. Roda mouse = zoom ke
arah kursor (1 %–3200 %), tarik atau Spasi + tarik = geser, klik ganda = 100 %,
Ctrl 0 = pas layar, Ctrl 1 = 100 % (piksel cetak asli), Ctrl +/− = langkah zoom. Area
transparan tampil sebagai kotak-kotak; tombol **Warna kaos** menampilkannya di atas warna
kaos.

**Batal / ulangi** (tombol ↶ ↷ di toolbar): **Ctrl Z** = batal, **Ctrl Y** atau
**Ctrl Shift Z** = ulangi, sampai 100 langkah. Semua pengaturan dan crop ikut; perubahan
beruntun dalam 0,4 detik (mis. menggeser slider) dihitung satu langkah. Riwayat mulai baru
saat membuka gambar atau proyek. Di dalam kolom angka, Ctrl Z membatalkan ketikan di kolom
itu saja.

Tombol cepat lain: **Ctrl O** = buka gambar, **Ctrl Shift O** = buka proyek,
**Ctrl S** = simpan proyek, **Ctrl E** = export PNG (setelah render selesai).

Render berjalan per potongan (band) di latar belakang, jadi 600 DPI tetap ringan: cetakan
30×40 cm @600 DPI butuh ±0,9 GB RAM (sebelumnya ±5–6 GB) dan hasilnya identik piksel per
piksel dengan render penuh. Mengubah pengaturan langsung membatalkan render yang sedang
jalan.

## Isi repo

| Folder | Isi |
|---|---|
| `engine/` | engine Rust (halftone, hapus background, resize, PNG, Perbesar AI). Ada juga `studiozea-cli` |
| `engine/models/` | bobot AI Real-ESRGAN Compact (BSD-3-Clause, lihat `LICENSE-Real-ESRGAN`) dan detektor teks PP-OCRv4 (Apache-2.0, lihat `LICENSE-PaddleOCR`), ikut di dalam aplikasi |
| `tools/` | konversi bobot AI dan pembuat data uji AI (butuh PyTorch, hanya untuk pengembang) |
| `app/` | aplikasi desktop Tauri 2 + React |
| `textures/` | pustaka tekstur vintage; setiap file di sini ikut ke aplikasi saat dibuild |
| `prototype/` | engine acuan Python (spesifikasi). Engine Rust harus identik piksel per piksel dengannya |
| `docs/RENCANA-DTF.md` | rencana, analisis referensi, dan semua hasil uji |

## Build sendiri

```bash
# engine + uji golden (identik piksel dengan prototype/)
cargo test --release -p studiozea-engine

# command line
cargo run --release -p studiozea-engine --bin studiozea-cli -- desain.png hasil.png --kaos hitam --lpi 35 --lebar-cm 30 --trim
# dengan Perbesar AI 4x dulu (--ai-halus 0..1), atau hanya memperbesar (--ai-saja)
cargo run --release -p studiozea-engine --bin studiozea-cli -- kecil.jpg besar.png --ai 4 --ai-saja

# aplikasi (butuh Node 22 dan Rust; di Linux juga webkit2gtk-4.1)
cd app && npm ci && npx tauri build      # installer di target/release/bundle/
cd app && npx tauri dev                  # mode pengembangan
```

Kalau algoritma di `prototype/studiozea_ref.py` diubah: jalankan
`python prototype/make_golden.py` untuk memperbarui `engine/tests/golden/`, lalu sesuaikan
engine Rust sampai `cargo test` identik lagi.

Perbesar AI (`engine/src/upscale.rs`) menjalankan jaringan Real-ESRGAN Compact langsung di
Rust (konvolusi Winograd + `gemm`, AVX2/AVX-512 dipilih otomatis), per potongan dengan
konteks 34 px sehingga tanpa sambungan. Di kartu grafis (`engine/src/upscale_gpu.rs`, wgpu:
DirectX 12 / Vulkan / Metal) satu compute shader WGSL menjalankan konvolusi yang sama.
`engine/tests/upscale.rs` membandingkannya dengan PyTorch (data dari `tools/make_ai_ref.py`)
dan kartu grafis dengan prosesor: selisih maksimal 1 level warna; juga simulasi kartu yang
putus di tengah jalan, dan terjemahan shader ke HLSL / MSL / SPIR-V. Tes kartu grafis
memakai renderer software bila tidak ada kartu (WARP di Windows, llvmpipe di Linux);
`STZ_GPU_STRICT=1` membuatnya wajib lolos. CLI: `--ai-perangkat auto|cpu|gpu`
(`STZ_GPU_SOFTWARE=1` menerima renderer software).

Lindungi teks (`engine/src/text.rs`): detektor PP-OCRv4 (ONNX, dijalankan dengan `tract`,
murni Rust) di 1280 px dan 640 px (logo besar yang di 1280 px terpecah) → kata (DB: ambang
0,3, rata-rata 0,6, diperbesar "unclip" 1,8) → per kata
pemisahan Otsu warna huruf/latar → masker `solid` di `Prepared` (ikut crop / trim / putar,
di-resize bersama cetakan) → `render_at_masked` mencetak piksel masker padat (atau, dengan
`text_style: halus`, dari screen kedua di `Settings::text_fine_lpi`) setelah halftone, sebelum
lubang tekstur dan clean-up. Arsir di pratinjau: `overview`/`tile` jenis `text`. `engine/tests/text.rs` (gambar uji sintetis):
tulisan ditemukan, huruf padat, latar tetap halftone, mati = identik, band = render penuh,
kuas Teks/Bukan teks, titik halus (lebih rapat, warna murni). CLI: `--lindungi-teks`
(`--teks-halus` = titik halus); `STZ_TEXT_DEBUG=debug.png` menulis area kata
(berwarna) dan piksel yang dicetak padat (putih) untuk pemeriksaan.
