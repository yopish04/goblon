# StudioZea: rencana implementasi (tahap 1: DTF)

Status: **rencana, belum ada kode aplikasi.** Dokumen ini menjelaskan cara kerja
engine dan aplikasinya. Tujuannya: hasil cetak sama seperti dua gambar referensi
(Iron Maiden *Run to the Hills* di kaos hitam dan *Powerslave* di kaos putih).

Alurnya sederhana: **masukkan gambar → hapus background → crop + ukuran + DPI →
halftone (LPI) → export PNG siap cetak DTF.**

---

## 1. Hasil analisis gambar referensi

Saya ukur langsung dari piksel gambar hasil (bukan dari tebakan visual).

| | Run to the Hills (kaos hitam) | Powerslave (kaos putih) |
|---|---|---|
| Ukuran file | 3543 × 4155 px | 3543 × 4653 px |
| DPI tersimpan | 300 | 300 |
| Ukuran cetak | 30,0 × 35,2 cm | 30,0 × 39,4 cm |
| Alpha | **biner** (0 atau 255 saja) | **biner** |
| Jumlah warna | **7 warna murni** | **7 warna murni** |
| Warna yang ada | putih 60%, hijau 12%, biru 10%, cyan 8%, kuning 4,5%, magenta 3%, merah 2% | hitam 29%, magenta 17%, kuning 16%, hijau 15%, merah 15%, cyan 4,5%, biru 3,5% |
| Warna yang **tidak ada** | hitam → jadi transparan | putih → jadi transparan |
| LPI | **35,0** | **35,0** |
| Sudut | R, G, B semua **20,6°** (fasa sama, *dot-on-dot*) | C **20,6°**, M **69,4°** (−20,6°), Y **20,6°** digeser ±0,13 sel dari C |
| Pembesaran dari sumber | ×2,15 (desain di-trim lalu di-upscale) | ×2,24 (di-trim pas ke desain) |

### Kesimpulan penting

1. **Halftone dibuat per channel warna, bukan satu masker.** Setiap channel
   (R/G/B atau C/M/Y) di-screen sendiri, lalu digabung. Hasilnya, setiap piksel
   hanya bisa jadi salah satu dari 8 kombinasi tinta murni. Warna campuran
   (kulit, ungu langit, emas) muncul dari campuran optik titik-titik itu, seperti
   cetak offset atau sablon CMYK. Inilah tampilan "rosette" di referensi.
2. **Hapus background terjadi otomatis dari model warna.**
   - Kaos hitam: kombinasi "tidak ada tinta" = hitam = **transparan**. Bagian
     gelap desain otomatis jadi bolong dan menampilkan kaos.
   - Kaos putih: kombinasi "tidak ada tinta" = putih = **transparan**.
3. **20,6° itu sebenarnya 22,5°.** Pada 300 DPI dan 35 LPI, satu sel = 8,57 px.
   Supaya pola berulang rapi di grid piksel (tanpa moiré), sudut 22,5° di-*snap*
   ke vektor rasional 8:3 (sel 8,544 px = 35,1 LPI, sudut 20,56°). Ini teknik
   standar RIP (*rational tangent screening*).
4. **Kaos hitam memakai dot-on-dot** (semua channel di grid dan fasa yang sama).
   Hasilnya titik berlapis konsentris: lubang hitam di tengah, cincin kuning atau
   merah di sekeliling. **Kaos putih memakai dua sudut berbeda** (C/Y 22,5° dan
   M 67,5°). Itu yang membentuk pola rosette berwarna-warni.

### Kenapa dua repo sebelumnya tidak sama hasilnya

- `dtf-studio` (`src/engine/pipeline/render.js`): halftone hanya dijalankan pada
  **satu masker alpha** (jarak warna ke warna knockout), dengan satu sudut. Warna
  RGB asli dipertahankan di dalam titik. Hasilnya titik-titik berwarna asli yang
  halus, bukan 7 warna tinta murni yang di-screen per channel. Pendekatannya
  berbeda dari dasarnya, jadi tidak bisa disetel sampai sama.
- `Dtf`: masih kosong (hanya README).

### Bukti: prototipe sudah cocok

Saya buat prototipe kecil (Python, di luar repo, sekitar 60 baris) dengan
algoritma di bawah, dijalankan pada gambar asli Anda:

| Cakupan tinta | Referensi | Prototipe |
|---|---|---|
| RTH channel R / G / B | 15,7 / 19,1 / 18,4 % | 16,8 / 20,4 / 20,0 % |
| Powerslave C / M / Y | 19,8 / 24,4 / 28,7 % | 21,1 / 25,9 / 31,2 % |

Polanya, bentuk titik, cincin warna dan rosette-nya secara visual hampir identik
(lihat gambar perbandingan zoom 300% di percakapan).

Yang masih kurang dari prototipe (dikerjakan di M1):
- Powerslave: area opak 41% vs 38%, tiap channel +1,3–2,5%. Catatan: referensi
  Powerslave ternyata dibuat dari **versi artwork yang sedikit berbeda**
  (proporsi judul dan posisi elemen tidak sama dengan file sumber yang ada),
  jadi angka ini hanya perkiraan. Kalibrasi kurva *dot gain* dan fasa Y tetap
  dikerjakan.
- Pembersihan titik minimum sudah diuji (§3.8), tapi belum ada uji cetak nyata
  di printer DTF. Untuk itu ada *test sheet* (§8 butir 7).
- Belum diuji dengan gambar sumber yang punya background berwarna atau
  transparansi (§3.2, §3.7).

---

## 2. Alur kerja untuk pengguna

Satu jendela. Kanvas di kiri, panel di kanan dengan 4 bagian, lalu tombol Export.

```
┌───────────────────────────────────────────┬──────────────────────────┐
│                                           │ 1. GAMBAR                │
│                                           │   [Buka…]  Auto-trim ☑   │
│                                           │   Crop: [Atur] [Reset]   │
│          KANVAS                           │ 2. BACKGROUND            │
│   Original | Hasil | Split | 100%         │   Jenis kaos: ● Hitam    │
│                                           │               ○ Putih    │
│   (warna kaos tampil di belakang hasil)   │   Hapus: Otomatis ▾      │
│                                           │   Toleransi: ──●── 8     │
│                                           │ 3. UKURAN                │
│                                           │   Lebar 30,0 cm 🔒        │
│                                           │   Tinggi 35,2 cm         │
│                                           │   DPI: 300 ▾             │
│                                           │ 4. HALFTONE              │
│                                           │   LPI: 35 ▾              │
│                                           │   Pola: Referensi ▾      │
│                                           │   Titik min: 6 %         │
│                                           │ [ EXPORT PNG ]           │
└───────────────────────────────────────────┴──────────────────────────┘
```

Preset siap pakai:
- **"Kaos hitam: seperti RTH"**: mode gelap, 35 LPI, 22,5°, dot-on-dot, 300 DPI.
- **"Kaos putih: seperti Powerslave"**: mode terang, 35 LPI, C/Y 22,5°, M 67,5°.
- **"Tajam"** (versi lebih baik): sama dengan dua preset di atas, ditambah tepi tajam
  dan highlight hybrid (§7.1).

LPI, sudut dan bentuk bisa dipilih bebas (lihat §4). Opsi per channel dan kurva
dot gain disembunyikan di bawah "Lanjutan".

---

## 3. Engine: urutan proses

Semua ukuran dihitung dalam **piksel cetak** (mm × DPI), bukan piksel layar.
Preview dan export memakai fungsi yang sama, jadi hasilnya identik.

### 3.1 Muat gambar
PNG/JPG/WebP/TIFF → sRGB 8-bit, diproses dalam float 0..1. Profil ICC dikonversi
ke sRGB kalau ada.

### 3.2 Hapus background
Tiga mode:
1. **Otomatis dari jenis kaos** (default, cara kerja referensi). Tidak ada
   langkah terpisah: piksel yang hasil screening-nya "tanpa tinta" jadi
   transparan. Ditambah *clip*: nada di bawah `titik min` dibuang supaya noise
   JPEG di background hitam atau putih tidak jadi titik liar.
2. **Background berwarna (warna otomatis dari tepi atau dipilih dengan pipet).**
   Algoritma ini sudah diuji (desain di atas background teal dengan tepi halus):
   - Warna background = median piksel tepi gambar.
   - Zona background = piksel mirip background yang **tersambung ke tepi**, plus
     "kantong" di dalam desain yang warnanya *persis* background (≥ 4 px).
     Zona ini dilebarkan 3 px.
   - **Alpha dihitung dengan matting lokal**, bukan dari jarak warna saja:
     α = proyeksi (piksel − bg) ke (F − bg), dengan F = warna paling "desain" di
     sekitarnya (7×7). Setelah itu warna background dikeluarkan dari tepi
     (*un-mix*), jadi tidak ada semburat background di sekeliling garis.
   - **Alpha minimum (colour-to-alpha):** α dinaikkan sampai warna hasil un-mix
     masih di dalam rentang 0..1. Piksel yang bukan campuran bg + F (garis
     hitam, bayangan gelap yang menyentuh background navy/marun) tetap
     **opak hitam**, tidak lagi tembus warna kaos. Lihat §9c.
   - Hasil uji:

| Versi | ΔE vs ideal | Tinta tersisa di area background |
|---|---|---|
| Flood + alpha dari jarak warna | 3,84 | 52.795 px (celah tipis di huruf ikut tercetak) |
| Global color key | 3,79 | 0 px, tapi 5% warna desain yang mirip teal ikut terhapus |
| **Flood + kantong + matting lokal (dipakai)** | **3,12** | **871 px** |
| Pembanding: file asli berbackground putih | 2,92 | — |

3. **AI cut-out** (tahap 2, opsional): model berlisensi komersial yang aman,
   jalan offline di Windows (ONNX Runtime). Jangan pakai RMBG-1.4/2.0 atau
   @imgly (lisensinya tidak boleh untuk komersial).

### 3.3 Crop
- **Auto-trim**: kotak batas konten (piksel yang punya tinta setelah clip).
  Watermark kecil di pojok bisa dibuang dengan crop manual.
- **Crop manual** seperti Photoshop: kotak dengan 8 pegangan dan garis sepertiga; tarik
  sudut/sisi (Shift = rasio tetap), geser dari dalam, Enter = terapkan, Esc = batal.

### 3.4 Ukuran + DPI
- Input lebar/tinggi dalam cm atau inci (rasio terkunci), DPI 150/200/300/360/600
  (default 300).
- Piksel cetak = cm ÷ 2,54 × DPI. Contoh: 30 cm @ 300 = 3543 px, persis seperti
  referensi.
- Resample **Lanczos-3** dari sumber ke ukuran cetak *sebelum* halftone.
- Muncul peringatan kalau resolusi efektif sumber < 150 DPI. Tahap 2: AI upscale
  (Real-ESRGAN, BSD-3).

### 3.5 Pemisahan channel
- **Kaos hitam (aditif):** channel = R, G, B. Nilai 1 = tinta menyala (R+G+B =
  putih, R+G = kuning, dan seterusnya; tidak ada tinta = hitam = transparan).
- **Kaos putih (subtraktif):** channel = C = 1−R, M = 1−G, Y = 1−B. Tidak ada
  tinta = putih = transparan, ketiganya = hitam. Channel K opsional (§4.9).
- Kurva per channel sebelum screening: *clip* (titik min), *max* (titik maks →
  solid), dan kurva dot gain opsional.

### 3.6 Screening halftone (inti)
Untuk setiap channel *c* dan setiap piksel cetak (x, y):

1. **Sel rasional:** `cell = DPI / LPI` (35 LPI @ 300 = 8,57 px). Untuk sudut θ,
   cari pasangan bilangan bulat (P, Q) dengan n = 1..4 sel yang paling dekat ke
   `n·cell·(cosθ, sinθ)`. Sel nyata = |(P,Q)|/n dan sudut nyata = atan2(Q,P).
   Contoh: 22,5° → (8,3) dan 67,5° → (3,8) atau (8,−3) di koordinat layar.
2. **Koordinat sel:** putar (x+0,5, y+0,5) dengan sudut nyata, bagi dengan sel,
   lalu tambah fasa channel. Ambil bagian pecahan → (u, v) ∈ [−0,5; 0,5).
3. **Fungsi titik Euclidean:** `s = 2 − cos 2πu − cos 2πv`. Titik bulat saat
   nada < 50%, papan catur di 50%, lubang bulat saat nada > 50% (persis bentuk di
   referensi: titik putih kecil di area gelap, lubang hitam kecil di area terang).
4. **Linearisasi:** ubah *s* jadi ambang T ∈ [0,1] lewat tabel CDF yang dihitung
   sekali (1024² sampel). Dengan begitu cakupan titik = nilai nada secara persis.
5. **Keputusan:** tinta di channel *c* menyala jika `nada_c(x,y) > T_c(x,y)`.

Konfigurasi sudut dan fasa:

| Pola | Channel 1 | Channel 2 | Channel 3 |
|---|---|---|---|
| Referensi kaos hitam | R 22,5° | G 22,5° | B 22,5° (fasa sama) |
| Referensi kaos putih | C 22,5° | M 67,5° | Y 22,5°, fasa +0,13 sel |
| Rosette klasik | C 15° | M 75° | Y 0° (+K 45° opsional) |

Ambang hanya bergantung pada koordinat piksel cetak absolut. Akibatnya preview
per bagian (tile), loupe 1:1 dan export memberi piksel yang sama (deterministik).

### 3.7 Gabung + alpha (jaminan: tidak ada semi-transparan)
Tiga (atau empat dengan K) bit per piksel → salah satu dari 8 warna. Kombinasi
"kertas" (hitam untuk kaos hitam, putih untuk kaos putih) → alpha 0. Masker
background dari 3.2 → alpha 0.

Alpha **wajib** 0 atau 255, karena DTF tidak bisa mencetak setengah tinta. Untuk
menjamin itu:
- **Tipe data:** alpha disimpan sebagai masker 1-bit (`bool`) sampai saat menulis
  PNG. Tidak ada langkah yang bisa menghasilkan nilai di antaranya.
- **Gambar sumber yang punya transparansi** (PNG dengan tepi halus atau
  bayangan): alpha sumber dipakai sebagai pengali nada sebelum screening
  (premultiplied), lalu ikut di-halftone. Alpha sumber tidak pernah diteruskan
  ke hasil. Resize memakai warna premultiplied supaya tepi tidak berhalo.
- **Warna murni:** setiap kanal RGB yang opak bernilai 0 atau 255.
- **Tidak ada warna tersembunyi:** piksel transparan selalu RGB (0,0,0), supaya
  RIP yang membaca warna di bawah alpha 0 tidak mencetak sesuatu.
- **Preview** boleh diperkecil atau dihaluskan di layar. File export tidak pernah
  melewati proses itu.
- **Cek akhir otomatis sebelum file ditulis:** semi-transparan = 0 dan warna
  tidak murni = 0. Kalau gagal, export dibatalkan dengan pesan error. File yang
  salah tidak pernah tersimpan.

Hasil cek prototipe: semi-transparan = 0, warna tidak murni = 0, 7 warna, untuk
kedua desain. Referensi Anda juga 0.

**Uji PNG transparan:** input berisi **546.903 piksel semi-transparan** (tepi halus
+ bayangan hitam 50%). Hasil: semi-transparan 0, warna tidak murni 0, titik < 3 px
0. ΔE vs ideal di kaos putih 3,02. Bayangan menjadi titik halftone. Catatan:
bayangan abu-abu tanpa channel K tersusun dari titik C, M dan Y terpisah (terlihat
berwarna saat di-zoom). Untuk bayangan dan area netral, aktifkan K/GCR (§4.9).

### 3.7b Mode kaos berwarna (sudah diuji)
Di kaos berwarna, hitam maupun putih sama-sama harus dicetak. Jadi tidak ada warna
yang "gratis" jadi transparan.
- **Screen opasitas** (AM, 45°) menentukan di mana tinta dicetak (di atas white
  underbase) dan di mana kaos dibiarkan terlihat.
- **Screen C/M/Y** (22,5° / 67,5° / 22,5°) menentukan warna di atas putih. Tanpa
  tinta = putih, ketiganya = hitam.
- **Knockout warna kaos:** warna desain yang mirip warna kaos (jarak < 0,22)
  dibuat transparan, jadi kaos sendiri yang tampil (hemat tinta, terasa lebih
  "menyatu").

| Uji (ΔE vs ideal, rata-rata / 95%) | Mode kaos hitam | Mode kaos putih | **Mode kaos berwarna** |
|---|---|---|---|
| Powerslave (PNG transparan) di kaos merah #B01E28 | 14,44 / 42,3 | 33,88 / 86,1 | **3,18 / 8,37** |
| Run to the Hills (bg hitam dihapus) di kaos navy #1C2541 | 8,51 / 18,0 | 10,74 / 26,6 | **4,64 / 13,3** |

Semua lolos cek wajib (semi 0, warna murni, titik < 3 px 0). Navy masih 4,64
(target ≤ 3). Di M1 dicoba: screen opasitas yang sejajar dengan screen warna (dot-on-dot)
supaya dua screen tidak saling menimpa, dan model aditif untuk kaos gelap
berwarna.

### 3.8 Bersih-bersih untuk DTF (titik minimum)
Hasil ukur (pulau tinta yang terpisah, terhubung 4 arah):

| | ≤ 2 px (≈ < 0,15 mm, tidak tertahan powder) |
|---|---|
| Referensi RTH | 4.495 pulau |
| Referensi Powerslave | 61.914 pulau (dari bintik distress di sumber) |
| Prototipe sebelum dibersihkan | 5.873 / 9.652 |
| **Prototipe sesudah dibersihkan** | **0 / 0** |

Aturan default (bisa diatur dalam mm, otomatis dikonversi ke px sesuai DPI):
- Pulau tinta < 0,15 mm² (300 DPI: < 3 px) dibuang. Cakupan hampir tidak
  berubah (24,30% → 24,25%).
- Lubang transparan ≤ 2 px di dalam tinta diisi dengan warna tetangga terdekat.
- Opsional: bagian tipis < 0,15 mm (garis 1 px) ditebalkan, bukan dibuang,
  supaya detail garis tetap ada.
- Opsional: tutup lubang yang menyentuh garis luar desain, supaya tepi tidak
  "berbulu".

### 3.9 Export
- PNG RGBA 8-bit dengan chunk **pHYs** (DPI benar, terbaca di RIP seperti
  CADlink/AcroRIP/Maintop). Encoder streaming supaya file besar (60 × 100 cm) tidak
  kehabisan memori.
- Nama file otomatis: `nama_30x35cm_300dpi_35lpi.png`.

### 3.10 Bit depth: input, proses, output (sudah diuji)
- **Output:** isi hasil sebenarnya hanya 3 bit warna + 1 bit alpha (8 warna +
  transparan). Bentuk filenya:
  - **Default: PNG RGBA 8-bit**, sama seperti referensi, paling aman di semua RIP.
  - **Opsi: PNG palet** (9 entri + transparansi): pikselnya terbukti identik, file
    ±45% lebih kecil (contoh AM RTH 2,4 → 1,2 MB). Hanya dipakai kalau RIP Anda
    terbukti membaca transparansi palet dengan benar.
  - **Tidak ada opsi 16-bit:** nilainya hanya 0/255, jadi 16-bit tidak menambah
    apa pun selain ukuran file, dan sebagian RIP bermasalah dengannya.
- **Pola tidak mengubah bit depth.** Semua pola menghasilkan format yang sama.
  Yang berbeda hanya ukuran file (prototipe: 0,7–3,8 MB untuk 30 × 35–40 cm).
- **Input 8-bit vs 16-bit:** diuji pada gradasi gelap 5–25% (langit malam di kaos
  hitam, hanya ±51 tingkat di 8-bit). Galat nada beda ≤ 0,2%, karena sel AM 35
  LPI @300 DPI hanya punya 73 tingkat. Gambar 8-bit cukup. PNG/TIFF 16-bit tetap
  didukung.
- **Aturan proses:** semua langkah (resize, kurva, pemisahan channel) dihitung
  dalam **float32**, tanpa pembulatan ke 8-bit di antara langkah. Uji: kurva yang
  dibulatkan ke 8-bit melipatgandakan banding AM (0,37 → 0,73). Dengan float
  banding tidak bertambah.
- **Yang lebih berpengaruh daripada bit depth:** resolusi sumber (lihat
  peringatan DPI efektif di 3.4) dan kompresi JPEG. Artefak kotak 8×8 JPEG
  terlihat terutama di pola FM/difusi. Tahap 2: filter penghalus artefak JPEG
  sebelum screening.
- **Profil warna dan RIP:** PNG ditandai sRGB. RIP mengubah RGB → CMYK +
  putih. Supaya warna murni hasil kita (misalnya magenta 255,0,255) tetap jadi
  tinta yang diharapkan, setelan RIP harus konsisten: ukuran 100%, 300 DPI, tanpa
  resample. Konversi warnanya dikalibrasi lewat *test sheet* (§8 butir 7).
- Tahap 2: export masker white underbase terpisah (choke hanya garis luar).

---

## 4. Sistem halftone: banyak pilihan LPI, sudut dan bentuk

Rumus di 3.6 berlaku untuk **LPI, sudut dan bentuk apa pun**. 35 LPI 22,5° bulat
hanyalah satu preset. Setiap channel punya pengaturan screen sendiri:

```
Screen    { lpi, sudut, bentuk, fasa(u,v), rasio_elips }
Halftone  { mode: Gelap|Terang, channel: [Screen; 3 (+K nanti)],
            samakan_lpi, samakan_bentuk, snap: Rapi|Presisi, titik_min_mm }
```

### 4.1 LPI
- Daftar cepat: 15, 20, 25, 30, 35, 40, 45, 50, 55, 60, 65, 75, 85.
- Input bebas 10–150 dengan desimal (misalnya 37,5).
- Default sama untuk semua channel. Di menu Lanjutan bisa beda per channel.

### 4.2 Sudut
- Input bebas 0–180° dengan desimal, per channel.
- Preset kombinasi sudut:

| Preset | C (atau R) | M (atau G) | Y (atau B) | Catatan |
|---|---|---|---|---|
| Referensi kaos hitam | 22,5 | 22,5 | 22,5 | dot-on-dot |
| Referensi kaos putih | 22,5 | 67,5 | 22,5 + fasa | seperti Powerslave |
| Rosette klasik | 15 | 75 | 0 | standar offset/sablon |
| Rosette 45 | 45 | 75 | 15 | |
| Satu sudut | sama semua | | | bisa pilih sudutnya |

- **Snap rasional** (supaya tidak moiré). Angka hasil snap selalu ditampilkan,
  misalnya "nyata: 35,1 LPI · 20,6°".
  - *Rapi* (default, sama dengan referensi): satu sel per langkah.
  - *Presisi*: sampai 4 sel per langkah, jadi lebih dekat ke angka yang diminta.

  Contoh hasil snap pada 300 DPI (mode Presisi, LPI nyata / sudut nyata):

| LPI | 0° | 15° | 22,5° | 45° | 75° |
|---|---|---|---|---|---|
| 20 | 20,0 / 0 | 19,9 / 15,4 | 20,1 / 22,7 | 19,9 / 45 | 19,9 / 74,6 |
| 25 | 25,0 / 0 | 24,9 / 14,4 | 25,1 / 23,0 | 25,0 / 45 | 24,9 / 75,6 |
| 35 | 35,3 / 0 | 35,1 / 15,3 | 34,7 / 22,1 | 35,4 / 45 | 35,1 / 74,7 |
| 45 | 45,0 / 0 | 44,6 / 15,1 | 44,6 / 21,8 | 44,7 / 45 | 44,6 / 74,9 |
| 65 | 64,3 / 0 | 64,2 / 15,5 | 65,3 / 22,4 | 65,3 / 45 | 64,2 / 74,5 |

### 4.3 Bentuk titik
Setiap bentuk cukup ditulis sebagai satu *fungsi titik* s(u,v). Linearisasi CDF
(3.6 langkah 4) membuat cakupannya otomatis tepat untuk semua bentuk.
Menambah bentuk baru = satu fungsi + satu entri daftar di
`engine/src/screen/shapes.rs`.

| Bentuk | Fungsi s(u,v) | Karakter |
|---|---|---|
| Bulat (Euclidean), default | 2 − cos 2πu − cos 2πv | titik bulat → papan catur 50% → lubang bulat |
| Bulat murni | u² + v² | titik tetap bulat sampai bersentuhan |
| Elips | (u/r)² + (v·r)², r = 0,6–0,95 | rantai titik di nada tengah |
| Kotak | max(\|u\|, \|v\|) | kotak tajam, gaya retro |
| Berlian | \|u\| + \|v\| | belah ketupat |
| Garis | \|v\| | line screen (garis sejajar) |
| Silang | min(\|u\|, \|v\|) | grid silang |
| Garis gelombang | 1 − cos 2π(v + 0,16·sin 2πu) | garis bergelombang (gaya "wavy lines") |
| Arsir silang | min(1 − cos 2πv, 1 + 0,5·(1 − cos 2πu)) | garis satu arah; arah tegak lurus masuk mulai ±50 % |
| Titik kasar (analog) | bulat + 0,8 × derau per kelas piksel (hash integer) | titik kasar ala halftone analog lama |

Ketiga bentuk terakhir ada di referensi Python dan Rust (identik, 11 kasus golden
tambahan). Derau "titik kasar" memakai hash integer (splitmix64) per kelas piksel dari
super-cell yang diperbesar, bukan hash trigonometri, supaya numpy dan Rust sama persis.
Tahap berikutnya bisa ditambah: konsentris, spiral, zig-zag, dan bentuk dari gambar PNG
kecil (tile ambang buatan sendiri).

### 4.4 Batas cetak DTF dan peringatan otomatis
- **Tingkat nada** per sel = (DPI/LPI)². Contoh pada 300 DPI: 25 LPI = 144,
  35 = 73, 45 = 44, 65 = 21, 85 = 12. Kalau di bawah 30, muncul saran naik ke
  600 DPI.
- **Titik minimum** (default 0,15 mm). Nada yang titiknya lebih kecil dari ini
  dibuang (atau dibuat solid di ujung atas), dihitung otomatis dari LPI:
  25 LPI ≈ 1,7%, 35 LPI ≈ 3,4%, 45 LPI ≈ 5,6%, 65 LPI ≈ 11,6%.
- **Peringatan moiré:** muncul kalau dua channel dengan LPI sama berbeda sudut
  kurang dari 15°, kecuali sudutnya persis sama (dot-on-dot memang disengaja).
  Selisih sudut dihitung modulo 90° untuk bentuk simetris dan 180° untuk garis
  atau elips.

### 4.5 Kecepatan
Screen rasional berulang tepat setiap (P² + Q²) piksel, misalnya 73 × 73 px untuk
35 LPI 22,5°. Tabel ambang satu tile per channel dihitung sekali (u16). Setelah
itu tiap piksel cukup satu perbandingan per channel. Gambar 16 MP selesai dalam
puluhan milidetik di banyak thread, jadi slider LPI/sudut/bentuk bisa langsung
terlihat hasilnya.

### 4.6 Panel Halftone di aplikasi
```
4. HALFTONE
   Preset: [Referensi kaos putih ▾]  [Simpan preset…]
   Pola:   [AM halftone ▾]  (FM, Index, Bayer, Grain, Garis, Gelombang, Konsentris, Custom)
   Hitam K:[Mati ▾]         (Garis saja / GCR ──●── 70%)   ← hanya kaos putih
   LPI:    [35 ▾]        ☑ sama untuk semua channel
   Bentuk: [Bulat ▾]     (Elips → rasio ──●──)
   Sudut:  C [22,5]  M [67,5]  Y [22,5]
           nyata: 35,1 LPI · 20,6° / 69,4° / 20,6°
   ⚠ 65 LPI @300 DPI: hanya 21 tingkat nada, sarankan 600 DPI
   ▸ Lanjutan: LPI/bentuk/fasa per channel · snap Rapi/Presisi · titik min (mm)
```
Preset buatan sendiri disimpan sebagai JSON di `%APPDATA%\StudioZea\presets\`.

### 4.7 Tes otomatis untuk setiap kombinasi
- Untuk setiap bentuk × LPI (15–85) × sudut: gradasi 0–100% harus menghasilkan
  cakupan linear ±1%.
- FFT tiap channel: puncak di LPI dan sudut nyata yang ditampilkan.
- Render dengan setting sama → hash piksel identik.

### 4.8 Pola lain selain halftone AM (rosette)

Halftone AM (titik di grid teratur, ukurannya berubah sesuai nada) hanyalah satu
jenis *screening*. Engine dibuat dengan dua jenis "mesin pola" supaya pola lain
bisa ditambahkan dengan mudah:

1. **Pola berbasis ambang (threshold):** setiap piksel cukup dibandingkan dengan
   satu nilai ambang di posisinya. Polanya bisa di-tile, cepat, dan preview pasti
   = export. Hampir semua pola masuk jenis ini.
2. **Difusi galat (error diffusion):** keputusan satu piksel memengaruhi piksel
   berikutnya, jadi harus dihitung satu gambar penuh. Preview memakai hasil penuh
   yang di-cache supaya tetap sama dengan export.

| Pola | Jenis | Tampilan | Pengaturan | Cocok untuk |
|---|---|---|---|---|
| **AM halftone / rosette** | ambang | titik teratur (referensi) | LPI, sudut, bentuk | vintage, kaos band, umum |
| **FM / stokastik** | ambang (*blue-noise* 256×256, di-tile) | titik kecil seukuran, tersebar acak rapi, tanpa rosette/moiré | ukuran titik (mm), seed | foto, gradasi halus, detail tinggi |
| **Difusi (Floyd–Steinberg / Atkinson)** | difusi | butiran organik | ukuran titik, algoritma | foto, gaya "printed" |
| **Index (piksel kotak)** | difusi/ambang di grid kasar | kotak-kotak kecil, khas index separation sablon | ukuran piksel (mis. 1/100″) | sablon simulated/index, retro |
| **Dither Bayer** | ambang (matriks 8×8) | pola kotak teratur, gaya 8-bit | ukuran piksel | retro, pixel art |
| **Mezzotint / grain** | ambang (noise halus, seed) | butir kasar acak | ukuran butir, seed | vintage kasar, grunge |
| **Garis lurus** | ambang (bentuk Garis §4.3) | garis sejajar menebal-menipis | LPI, sudut | gaya engraving, poster |
| **Garis gelombang** | ambang | garis bergelombang | LPI, sudut, amplitudo, panjang gelombang | psychedelic, retro 70-an |
| **Lingkaran konsentris / spiral** | ambang | cincin dari satu titik pusat | LPI, titik pusat (klik di kanvas) | pop art, efek fokus |
| **Pola dari gambar (custom)** | ambang (PNG tile dari pengguna) | apa saja (crosshatch, teks, tekstur) | file tile, skala, sudut | desain khusus |

Aturan yang berlaku untuk semua pola:
- Pola dipakai **per channel** (CMY/CMYK atau RGB), jadi hasilnya tetap 7–8 warna
  murni dengan alpha biner. Pola berbasis sudut tetap memakai sudut per channel.
  Pola FM/difusi memakai seed atau urutan berbeda per channel supaya titik
  tidak bertumpuk semua.
- **Batas DTF tetap dijaga:** ukuran titik terkecil FM, index, difusi dan grain
  tidak boleh di bawah 0,15 mm (300 DPI → minimal 2×2 px; 600 DPI → 4×4 px).
  Titik 1 px di 300 DPI (0,085 mm) tidak tertahan powder dan akan rontok.
- Tes otomatis yang sama (§4.7): gradasi linear ±1%, deterministik.
- Contoh visual semua pola (gradasi 0–100%) dan perbandingan pada desain
  Powerslave sudah dibuat di prototipe.

#### Hasil uji pada desain nyata (prototipe, 300 DPI)

Semua pola lolos cek wajib: semi-transparan 0, titik < 3 px 0, 7 warna murni.

| Pola | ΔE RTH* (rata-rata / 95%) | Titik terpisah RTH | Titik kecil ≤ 0,34 mm | Ukuran median titik kecil | Kesan |
|---|---|---|---|---|---|
| AM 35 LPI (referensi) | 2,00 / 4,69 | 94 rb | 59 rb | 10 px | vintage klasik, paling tahan cetak |
| FM stokastik 2×2 | 3,04 / 7,12 (sinkron: 2,80) | 198 rb | 179 rb | 4 px | halus seperti foto, tanpa pola |
| Difusi Floyd–Steinberg 2×2 | **1,56 / 3,62** | 207 rb | 189 rb | 4 px | paling akurat dan detail, kesan "digital" |
| Index kasar 3×3 | 2,48 / 5,66 | 92 rb | 52 rb | 9 px | retro/sablon, setahan AM |
| Dither Bayer 2×2 | 3,05 / 7,16 | 660 rb | 649 rb | 4 px | pola kotak terlihat jelas |
| Mezzotint/grain lama (sinkron) | 6,60 / 17,4 (bebas: 13,5) | 59 rb | 38 rb | 7 px | kasar, terlalu kontras |
| **Grain baru** (green-noise, sinkron, sedang) | **3,97 / 9,56** (halus 3,38 · kasar 6,01) | — | 60 rb (halus 104 rb · kasar 20 rb) | — | butir rapi, tanpa bercak besar |
| Garis 35 LPI | 2,70 / 6,70 | 31 rb | 26 rb | 5 px | gaya engraving, paling sedikit titik lepas |

\* ΔE = selisih warna dari jarak pandang dibanding gambar asli (§7). Referensi:
6,05 / 19,5.

Pelajaran dari uji ini:
- **Kaos hitam: pola acak (FM, grain) harus sinkron antar channel.** Kalau
  butir R, G dan B tidak saling berhubungan, muncul bercak pelangi. Dengan ambang
  yang sama untuk semua channel (setara dot-on-dot di referensi), grain membaik
  13,5 → 6,6 dan FM 3,04 → 2,80. Ini jadi default untuk kaos hitam.
- **FM/difusi paling akurat, tapi titik lepasnya 3× lebih banyak dari AM** dan
  ukurannya pas di batas minimum (4 px = 0,17 mm). Ketahanan cuci perlu dibuktikan
  di uji cetak. Disediakan opsi titik FM 3×3 (0,25 mm) yang lebih tahan.
- **Index** setara AM dalam ketahanan (median 9 px) dan cocok untuk gaya sablon.
- **Bayer** menghasilkan titik kecil paling banyak, jadi khusus untuk gaya retro.
- **Grain sudah diperbaiki.** Penyebab hasil lama yang kasar: noise yang hanya
  diburamkan masih menyimpan bercak besar (frekuensi rendah). Grain baru memakai
  *green noise*: noise di-band-pass (Gauss σ1 − Gauss σ2), jadi hanya butir seukuran
  yang diinginkan yang tersisa. Tile-nya 512×512 dan periodik, jadi cepat dan
  deterministik. Butirnya dibuat **sinkron untuk semua channel di kedua jenis
  kaos**. Hasil:
  - RTH: 6,60 → 3,97.
  - Powerslave: 5,64 (bebas) → 3,50 (sinkron). Pembanding AM 3,03, difusi 1,52.
  - Ada 3 ukuran: halus, sedang (default) dan kasar. Kasar paling tahan cetak
    (titik kecil 20 rb).

**Rekomendasi default:** AM untuk tampilan referensi. Difusi/FM untuk foto dan
detail halus (setelah lolos uji cetak). Index, Garis dan Bayer sebagai pilihan
gaya.

Urutan pengerjaan: AM + FM + Index masuk M1. Pola lain (Bayer, grain, garis
gelombang, konsentris, custom) masuk M2, karena hanya fungsi ambang baru di
mesin yang sama.

### 4.9 Kaos putih: CMY atau CMYK?

Hasil uji pada Powerslave, dengan sudut C/M/Y yang sama dan tambahan K 45°:
- Area netral (batu cokelat-abu): piksel berwarna turun **37% → 31%**, piksel hitam
  naik **12% → 15%**. Pasangan magenta+hijau yang tampak "pelangi" berkurang dan
  bayangan lebih dalam. Detail tulisan di batu lebih terbaca.
- Area berwarna jenuh (emas, biru): hampir tidak berubah.

Perlu dicatat: PNG yang dikirim ke RIP DTF berupa RGB. Piksel hitam (0,0,0) dari
tumpukan CMY dan dari K **sama persis di file**. RIP tetap yang menentukan tinta
fisiknya. Jadi channel K **tidak menghemat tinta di file kita**. Yang berubah adalah
*keputusan screening*: area abu-abu dan gelap jadi titik hitam bersih, bukan
campuran tiga warna.

**Keputusan:** channel K dibuat sebagai pilihan dengan 3 mode:
1. **Mati (CMY saja):** default untuk preset "Referensi kaos putih", supaya sama
   dengan referensi.
2. **Garis saja:** K hanya untuk hitam pekat (nada ≥ 90% di ketiga channel).
   Garis outline jadi solid dan tajam, tampilan warna tetap seperti referensi.
3. **GCR (0–100%):** hitam menggantikan campuran abu-abu. Disarankan 50–80% untuk
   desain banyak bayangan, abu-abu, foto, atau ilustrasi hitam-putih.

Biaya implementasinya kecil: satu channel ke-4 di sistem screen yang sama
(default 45°).

---

### 4.10 Tekstur vintage (pustaka tekstur)

Versi pertama membuat tekstur secara prosedural (retak Voronoi, bintik, goresan, pudar).
Hasilnya dinilai jelek, jadi **dihapus** dan diganti tekstur gambar asli (scan cetakan
aus) dari pustaka di repo. Layer lama di proyek tersimpan diabaikan (`Kind::Lama`).

**Pustaka:** setiap file gambar di folder `textures/` ditanam ke program saat build
(`engine/build.rs` → `engine/src/library.rs`, path `lib:<nama-file>`). Upload file baru
ke folder itu di GitHub → build berikutnya otomatis menampilkannya. Pengguna juga bisa
memilih file sendiri dari komputer. Format: hitam = aus; PNG transparan: bagian yang
tidak transparan = aus.

**Cara tekstur dipasang (supaya alami):**
- *Cover*: tekstur diperbesar sampai menutupi seluruh cetakan (rasio tetap, di tengah),
  jadi satu cetakan = satu scan utuh, tidak terlihat pola berulang. Ukuran (zoom
  0,5–3×), putar 0/90/180/270°, cermin, balik, geser (mm); di luar tekstur diulang secara
  cermin (tanpa sambungan). *Variasi acak* memilih kombinasi putar/cermin/geser.
- **Lubang**: kegelapan > 1 − kekuatan → lubang. Mask dibersihkan (close + open kotak
  titik minimum), lalu dipakai *dot-aware*: titik halftone dibuang/dipertahankan utuh
  (suara mayoritas), area solid dipotong tepat, potongan diselaraskan ke grid blok FM,
  `clean` tetap terakhir.
- **Pudar**: alpha dikali 1 − jumlah × kegelapan sebelum screening, jadi area aus menjadi
  titik lebih kecil yang tetap tercetak.
- **Campur**: keduanya; paling mirip cetakan plastisol yang sudah dicuci.
- Default kekuatan 50 %: retakan besar jadi lubang, butir halus hanya menipiskan.
- Deterministik dan aman per band (koordinat absolut, `sincos_deg` sendiri); uji di
  `engine/tests/distress.rs`: semua tekstur × semua mode di kaos hitam/putih/warna:
  0 piksel semi-transparan, 0 titik/lubang < 0,15 mm, band/jendela identik dengan render
  penuh, hash regresi.

**Hasil uji (RTH, 30×35 cm):** Lubang 50 % terlihat seperti tinta retak alami; Campur
lebih lembut; 300 dan 600 DPI, kaos hitam dan putih: cek semua 0. Ekspor aplikasi
(crop + vintage 02 Campur) byte-identik dengan command line
(`--tekstur gambar:path=lib:vintage-02.jpg,mode=campur`).

Lisensi: tekstur TheVectorLab boleh dipakai untuk bisnis sendiri, tetapi tidak boleh
dibagikan dalam aplikasi ke orang lain. Repo ini privat; jangan bagikan installer yang
berisi tekstur berlisensi.

### 4.11 Pola halftone: ukuran, sudut dan bentuk baru

Permintaan: pola halftone yang bisa dikecilkan/dibesarkan dan diputar dengan opsi.

**Sudut.** `Settings.angle` (derajat, None = 22,5° seperti dulu) dan `Settings.rosette`
(None = rosette kecuali kaos hitam dan ≥ 55 LPI). Rosette: C/Y = sudut, M = sudut + 45°;
titik di atas titik: semua sudut sama. Grid Auto memakai sudut itu. Selisih sudut di
`rational()` dihitung memutar (0–360°), hasil untuk sudut lama tidak berubah. Command line:
`--sudut 45 --rosette auto|ya|tidak`.

**Titik besar (3–150 LPI).** Dulu ambang disimpan sebagai tile S×S (S = P²+Q²): 5 LPI
@600 DPI → S = 14 400 → 207 juta nilai → crash, jadi aplikasi membatasi ≥ 10 LPI.
Sekarang ambang disimpan per kelas piksel (S nilai): piksel (x, y) masuk kelas
((x mod g)·g + (y mod g))·s′ + ((x div g)·P′ + (y div g)·Q′) mod s′ dengan g = fpb(P, Q),
P = gP′, Q = gQ′, s′ = P′²+Q′². Dua piksel sekelas tepat jika selisihnya vektor kisi
(P, Q), (−Q, P), jadi nilainya identik dengan tile lama (dicek 2 448 kombinasi dpi × LPI ×
sudut × bentuk × grid di Python). Baris dihitung bertahap (tanpa pembagian per piksel).
Ramp kalibrasi minimal 64 × 4 sel dan sampel zona aman maksimal 1024 px untuk sel besar
(ukuran biasa tidak berubah).

**Bentuk baru** (dipilih dari 6 kandidat; heksagon, garis putus dan hati ditolak karena
mirip bentuk lama atau tidak terbaca saat dicetak):

| Bentuk | Fungsi titik s(u, v) | Catatan |
|---|---|---|
| Bintang | r(1 + 0,38 sin 5θ), sin 5θ = Im((u+iv)⁵)/r⁵ | satu ujung ke atas di 0°; tanpa fungsi trigonometri supaya Rust = numpy bit demi bit |
| Segitiga | max(v, 0,866u − 0,5v, −0,866u − 0,5v) | ujung ke atas; lubang di area gelap jadi segitiga terbalik |
| Rantai | \|u\|/1,3 + 1,3\|v\| | berlian memanjang yang menyambung jadi rantai |

Semua bentuk: cek 0 (tidak ada titik/lubang < 0,15 mm) di 6–35 LPI, kaos hitam dan putih.

**Uji.** Golden 36 kasus lama identik (hash sama), 12 kasus baru (sudut 45/15/75/120/200°,
rosette paksa, dot-on-dot paksa, 5/6/8/12 LPI, bintang/segitiga/rantai, 300 dan 600 DPI)
identik Python ↔ Rust; semua kasus juga identik per band/jendela. Desain RTH 30×40 cm
@600 DPI pengaturan default: PNG byte-identik dengan versi sebelumnya, waktu dan RAM sama
(±8,7 s, 0,52 GB). Titik besar: 5 LPI 14,7 s / 0,74 GB, 3 LPI 23 s / 0,93 GB. Ekspor
aplikasi (kaos putih, bintang 6 LPI, 0°) byte-identik dengan command line.

**Aplikasi.** Kartu 4: kotak contoh pola (dirender engine pada sudut yang dipilih),
slider ukuran logaritmik 3–150 LPI + jarak titik dalam mm, sudut (Auto / 0 / 15 / 45 / 75 /
90° + slider + angka), pilihan warna Rosette / Titik di atas titik di kaos putih/berwarna,
preset Pop art dan Retro. Lembar uji cetak memuat ke-13 bentuk. Tekstur vintage sekarang
juga bisa diputar bebas (slider 0–359°).

### 4.12 Halftone hanya di area gradasi (slider area)

Permintaan: halftone hanya di area gradasi, blok warna tidak dipecah jadi titik, dengan
slider untuk mengatur area halftone. Diuji dulu di server dengan desain RTH (kaos hitam),
desain uji di kaos putih dan navy.

**Cara kerja** (`Settings.halftone_area`, `render_fades` di Python dan Rust):
- warna yang terlihat di kaos: `seen = a·c + (1 − a)·kaos`;
- jarak dari warna kaos, dinormalkan per channel: `o = max |seen − kaos| / max(kaos, 1 − kaos)`
  (0 = sama dengan kaos, 1 = sejauh mungkin). Sama untuk kaos hitam, putih dan berwarna;
- `o ≥ area` → cetak solid dengan warna asli; `o < area` → opasitas di-screen (AM/FM/grain,
  bentuk, LPI, sudut dan kalibrasi seperti biasa) dengan cakupan `o / area`;
- warna titik = `kaos + (seen − kaos) / cakupan`: rata-rata warna tetap sama dan tidak ada
  garis batas antara area solid dan area titik;
- alpha tetap 0/255, bersih-bersih titik/lubang < 0,15 mm dan tekstur vintage tetap berlaku.
  Channel K tidak dipakai di mode ini (warna asli sudah memuat hitam).

**Hasil uji:** blok warna (huruf hijau, langit terang, kotak abu-abu) solid; glow, bayangan
dan pinggiran yang memudar ke kaos jadi titik. Area 30 % = hanya pinggiran paling tipis,
100 % = hampir semua nada di bawah warna penuh jadi titik. Cek: semi-transparan 0, titik
< 0,15 mm 0 (warna tidak murni memang diizinkan di mode ini). Hasil disimpan RGBA8 (bukan 4
bit) hanya di mode ini: 30×40 cm @600 DPI 6,9 s / 0,68 GB. Golden: 8 kasus baru (AM, FM,
grain + tepi tajam, bintang 8 LPI, knockout navy, 600 DPI) identik Python ↔ Rust dan per
band; 48 kasus lama tidak berubah. Ekspor aplikasi byte-identik dengan command line
(`--gradasi 50`).

### 4.13 Uji bug menyeluruh di server (tanpa GitHub Actions)

- Fuzz 450 kombinasi acak (13 bentuk, LPI 3–150, DPI 100–720, sudut, rosette, gradasi saja,
  FM/grain, tepi tajam, tekstur, crop, K, knockout, 5 warna kaos, input 1×1 px, transparan,
  16-bit, palet, JPEG, alpha lembut): tidak ada crash, cek cetak semua 0, render band =
  render penuh. 60 kombinasi acak Python ↔ Rust identik.
- **Bug sudut** (diperbaiki): untuk sudut > 180° (juga M = sudut + 45° di rosette) selisih
  sudut bisa > 2π sehingga galat negatif dan grid yang salah menang (330° → −35,5°, 210° →
  −153°). Sekarang selisih dihitung modulo 2π; uji baru `tests/screen.rs` memeriksa
  0–360° per 7,5° (sebelum perbaikan 132 kombinasi gagal). Golden `putih_segitiga20_sudut200`
  diperbarui (Python dan Rust sama).
- **Bug pratinjau** (diperbaiki): di mode Gradasi saja, tampilan zoom kecil berwarna putih
  karena overview menghitung warna sebagai 0/255. Sekarang nilai warna asli dirata-rata.
- Command line menerima sudut negatif (`--sudut -30`); label "transparan" hanya untuk gambar
  yang benar-benar punya piksel transparan.
- Aplikasi: 7 perubahan pola cepat berturut-turut → ekspor sama dengan pengaturan terakhir;
  600 DPI 3 LPI + gradasi + tekstur: puncak 1,5 GB; cetakan 45 m ditolak dengan pesan jelas;
  input LPI tidak valid dibatasi 3–150.

### 4.14 Halftone Nyala / Mati (tekstur vintage saja)

`halftone_area = 0` (aplikasi: Halftone **Mati**, command line `--tanpa-halftone`): tidak ada
titik dan tidak ada yang "setengah": piksel yang minimal 50 % tidak transparan (dan berbeda
dari warna kaos) dicetak penuh dengan **warna aslinya**, sisanya tidak dicetak; lalu
bintik/lubang < 0,15 mm dibersihkan. Versi pertama mencetak tepi dengan warna yang dicampur
ke warna kaos (lebih gelap/pucat) sehingga di kaos tetap terlihat setengah transparan;
pengguna tidak mau itu, jadi diganti. Tekstur vintage tetap bisa dipakai (mode pudar di sini
menjadi lubang pada 50 %), jadi tersedia: halftone saja, tekstur saja, keduanya, atau tidak
keduanya.

Uji dengan desain AC/DC "High Voltage" kiriman pengguna (4093×5787 px @350 DPI, RGBA,
sudah bertekstur): file asli punya 190 050 piksel setengah transparan (0,8 %, di pinggir
huruf dan retakan), 27 447 pulau tinta dan 20 698 lubang < 0,15 mm (tidak akan tercetak
atau rontok saat dicuci). Setelah Halftone Mati: 0 / 0 / 0; warna tepi yang dicampur
(lebih gelap/pucat) turun dari 163 230 piksel (versi pertama) ke 10 piksel (isi lubang
kecil); tinta −5 % (piksel < 50 % opak tidak dicetak), tampilan tekstur sama. Ekspor aplikasi byte-identik dengan command line. Golden: 3 kasus
baru (kaos hitam, putih + tepi tajam, navy + knockout), 56 kasus lama tidak berubah.

**Buang warna mirip kaos** (`Settings.shirt_cut`, default 12 %, CLI `--ambang-kaos`): di mode
Halftone Mati, warna yang jaraknya dari warna kaos ≤ ambang tidak dicetak. Uji desain The
Offspring (JPG 896×1200, latar hitam dengan bayangan/vignette lembut dan noise JPG): ambang
2 % mencetak 74,6 % area termasuk 10 % tinta hampir hitam (bayangan + noise, lengkap dengan
underbase putih); ambang 12 % → 63 % area, 0 % tinta hampir hitam, tampilan di kaos hitam
sama. 18 % ke atas mulai membuang warna gelap penting (bata merah tua). Semua mode: 0 piksel
setengah transparan. Ekspor aplikasi byte-identik dengan command line.

### 4.15 Resize: satuan, ukuran cepat, Bicubic

Kartu 3 jadi panel resize: satuan cm / inci / piksel (disimpan sebagai cm dengan presisi
1e-6, jadi angka piksel yang diketik selalu jadi tepat sebanyak itu; dicek 50–8000 px di
150/300/360/600 DPI), tombol ukuran cepat, tampilan ukuran hasil px/cm/inci.
`Settings.resample` (CLI `--resize lanczos|bicubic`): Bicubic = filter Pillow (a = −0,5,
support 2) di jalur float yang sama dengan Lanczos, premultiplied alpha. Golden: 3 kasus
bicubic (perbesar, perkecil, perbesar + halftone mati) identik Python ↔ Rust dan per band;
kasus lama tidak berubah. Ekspor aplikasi (A4, Bicubic) byte-identik dengan command line.

## 5. Teknologi

**Tauri 2 (aplikasi Windows) + engine Rust + UI React.** (Sudah disetujui.)

- **Engine Rust** (`engine/`): satu library untuk semua proses gambar,
  multi-thread (rayon). 16 MP × 3 channel butuh sekitar 0,1–0,3 detik, jadi
  preview bisa langsung (*live*). Ada juga CLI untuk tes otomatis dan proses
  banyak file sekaligus.
- **Tauri**: installer Windows kecil (~10 MB, MSI/NSIS), jalan offline, gambar
  tidak pernah di-upload. Bisa dibuild untuk Mac nanti tanpa ubah engine.
- **React**: UI panel, mengikuti pengalaman dari dtf-studio.
- Engine yang sama dipakai lagi untuk mode **sablon/screen printing** (tahap
  berikutnya): separasi spot color, simulated process, film positif per warna,
  tanda registrasi.

Rencana struktur repo:
```
studiozea/
  engine/        crate Rust: load, bg, crop, resize, screen, compose, cleanup, png
  cli/           studiozea-cli in.png --kaos hitam --lpi 35 --lebar 30cm --dpi 300 out.png
  app/           Tauri + React (panel 1–4, kanvas, export)
  tests/golden/  cek otomatis terhadap angka referensi (gambar referensi tidak di-commit)
  prototype/     engine acuan Python yang sudah teruji (spesifikasi untuk port Rust)
  docs/          dokumen ini
```

---

## 6. Tahapan kerja

| Tahap | Isi | Selesai jika |
|---|---|---|
| **M1: Engine + CLI** | 3.1–3.10 (termasuk mode kaos berwarna 3.7b) + sistem screen §4 (semua LPI, sudut, bentuk; pola AM, FM, Index; CMYK opsional) di Rust, plus unit test | CLI menghasilkan PNG dari 2 gambar sumber yang lolos kriteria di §8 dan target §7, plus PNG *test sheet* untuk uji cetak |
| **M2: Aplikasi** | Jendela Tauri, 4 panel, pola tambahan (§4.8), kanvas (Original/Hasil/Split/100%), preset, export | Alur buka → atur → export berjalan tanpa error, preview = export |
| **M3: Installer Windows** | Build MSI/NSIS, ikon, asosiasi file | Terpasang dan jalan di Windows 10/11 bersih |
| **M4: Tambahan DTF** | AI hapus background, AI upscale, masker underbase, simpan proyek | — |

**Status (Okt 2026):**
- **M1 selesai**. `engine/` (Rust) + `studiozea-cli`. Uji golden 23
  kasus **identik SHA-256** dengan `prototype/studiozea_ref.py`. Desain asli di ukuran
  cetak juga identik piksel. Waktu render 30×40 cm @300 DPI 2–5 detik (Python ±60 detik).
  *Test sheet* cetak sudah ada (`studiozea-testsheet`, dan tombol di aplikasi).
  Belum: pola Index, Bayer, garis gelombang, konsentris.
- **M2 selesai**: `app/` (Tauri 2 + React). Ada 4 panel, preset, Asli/Hasil/Split,
  Pas layar/100 %, crop manual dengan menarik kotak, export.
  - Diuji di Linux (Xvfb): buka → proses → export. File export identik dengan CLI.
  - Tambahan dari uji GUI: penghapusan latar otomatis untuk kaos berwarna, dan kontrol
    toleransi latar + "di dalam desain" dengan default otomatis untuk latar putih/hitam.
    Tanpa ini, Powerslave di kaos navy kehilangan 31% isi desain; dengan default
    otomatis tinggal 0,1%.
- **M3**: installer NSIS/MSI + portable dibuat oleh GitHub Actions dan dipasang di
  Releases. Asosiasi file belum ada.
| **M5: Sablon** | Separasi spot/simulated process, film positif, registrasi | — |

## 7. Target: referensi = acuan minimum, engine harus lebih baik

Gambar referensi adalah **standar minimum**. Tampilannya (pola rosette, 35 LPI,
sudut, 7 warna murni, alpha biner) harus bisa dibuat sama persis lewat preset
"Referensi". Selain itu engine harus **terukur lebih baik** di poin-poin berikut.
Semuanya diukur otomatis, bukan dinilai dengan mata saja.

| Ukuran | Referensi | Prototipe sekarang | Target engine |
|---|---|---|---|
| Piksel semi-transparan | 0 | 0 | 0 (dijamin, §3.7) |
| Warna tidak murni | 0 | 0 | 0 |
| Pulau tinta < 3 px (rontok di DTF) | RTH 4.495 · PS 61.914 | **0 · 0** | 0 |
| Selisih warna dari jarak pandang vs gambar asli, RTH (ΔE rata-rata / 95%) | 6,05 / 19,5 | **2,31 / 5,35** | ≤ 2,5 / ≤ 6 |
| Kecerahan & saturasi rata-rata vs asli, RTH (L / chroma) | −2,1 / −1,7 (lebih gelap, lebih kusam) | −0,6 / +0,2 | ±1 / ±1 |
| Tepi huruf/garis | bergerigi, bintik di luar tepi | sama dengan referensi | **tepi tajam** (opsi, lihat di bawah) |
| LPI / sudut | 35 / 22,5° saja | bebas | bebas + peringatan (§4) |
| Pola | AM saja | AM + 8 pola lain | §4.8 |

Cara mengukur ΔE: hasil ditempel di atas warna kaos, lalu hasil dan gambar asli
sama-sama diburamkan (σ = 5 px, kira-kira mata melihat dari jarak 30–50 cm).
Setelah itu dihitung selisih warnanya dalam Lab. Posisi referensi sudah
disejajarkan ke sumber (selisih ≤ 1 px). Powerslave tidak dibandingkan dengan
cara ini karena versi artwork-nya berbeda.

### 7.1 Perbaikan yang membuat hasil lebih baik dari referensi
1. **Titik minimum DTF** (§3.8): tidak ada titik yang rontok. Sudah terbukti di
   prototipe.
2. **Nada dan warna akurat:** linearisasi CDF + kurva dot gain. Hasil tidak lebih
   gelap atau lebih kusam dari aslinya. Sudah terbukti (ΔE 6,05 → 2,31).
3. **Tepi tajam (opsi):** di tepi keras (huruf, outline, logo), cincin tepi
   selebar 2 px (≥ 0,15 mm) dibuat solid dengan warna aslinya, dan bintik liar
   sampai 3 px di luar tepi dibuang. Gradasi halus (langit, glow) tidak
   tersentuh karena hanya tepi dengan lonjakan nada > 45% yang diproses. Sudah
   diuji di prototipe: huruf IRON MAIDEN jauh lebih bersih, ΔE 2,57 (sedikit naik
   karena garis tepi, tapi tetap jauh di bawah referensi). Default: mati di
   preset "Referensi", hidup di preset "Tajam".
4. **Highlight tidak hilang:** nada yang lebih kecil dari titik minimum diganti
   titik FM jarang berukuran minimum (*hybrid AM/FM*), bukan dibuang. Gradasi tipis
   (kabut, glow) tetap ada dan tetap bisa dicetak. **Sudah diuji:** gradasi
   0–15%: nada 2–6% yang hilang total di AM biasa sekarang muncul (2% → 1,08%,
   4% → 3,90%). Galat nada maksimum 5,78% → 1,80%. ΔE RTH 2,00 → 1,88. Titik
   < 3 px tetap 0. Catatan: titik highlight ini jarang dan terpisah (0,17 mm),
   jadi ketahanannya ikut dicek di uji cetak. Ukuran klaster bisa dinaikkan ke
   3×3.
5. **Upscale lebih tajam:** Lanczos-3 (sekarang), AI upscale Real-ESRGAN di M4.
   Referensi diperbesar ×2,15 dari sumber kecil, jadi kualitas upscale berpengaruh
   besar.
6. **Channel K opsional** untuk kaos putih (§4.9): area abu-abu lebih bersih.

## 8. Kriteria lulus otomatis

Diuji otomatis di M1 (`tests/golden`):
1. Alpha hanya 0/255 (semi-transparan = 0), maksimal 7–8 warna opak, semuanya
   warna murni (0/255 per kanal), piksel transparan RGB = 0.
2. Tidak ada pulau tinta di bawah titik minimum (0 pulau < 3 px @ 300 DPI).
3. FFT tiap channel: puncak di **35 ± 0,5 LPI** dan sudut sesuai preset (±0,5°).
4. Cakupan tiap channel dalam **±1,5%** dari referensi untuk gambar sumber yang
   sama (setelah kalibrasi kurva dot gain).
5. Ukuran piksel dan DPI di file = input (30 cm @ 300 → 3543 px, pHYs 300).
6. Render ulang dengan setting sama → hash piksel identik.
   Ditambah semua angka target di tabel §7 (ΔE, L/chroma, pulau tinta).
7. **Uji cetak nyata:** engine bisa membuat *test sheet* PNG (tangga LPI 25–65,
   tiap bentuk titik, tangga nada 0–100%, titik minimum 0,10–0,30 mm). Sheet ini
   dicetak di printer DTF Anda dan dipres ke kaos hitam dan putih. Dari hasilnya
   ditentukan nilai default titik minimum dan kurva dot gain.

## 9. Status perbaikan dan rekomendasi

| Perbaikan | Status |
|---|---|
| Titik minimum DTF (0 titik < 3 px) | ✅ terbukti di prototipe |
| Nada/warna akurat (ΔE RTH 6,05 → 2,00) | ✅ terbukti |
| Tepi tajam | ✅ terbukti di kaos hitam dan putih (PS: ΔE 3,03 → 3,27) |
| Grain disetel ulang (green noise, sinkron) | ✅ terbukti (RTH 3,97 · PS 3,50) |
| Hybrid AM/FM (highlight + shadow, rentang aman per bentuk) | ✅ terbukti di 224 kombinasi (§9b) |
| Semua LPI × semua bentuk akurat | ✅ ΔE rata-rata 0,37, galat nada ≤ 2,9% (§9b) |
| Kalibrasi dot gain + fasa Y Powerslave | ⏳ M1, dari hasil uji cetak (referensi PS beda versi artwork) |
| Ketahanan FM/difusi/highlight hybrid di kaos | ⏳ butuh uji cetak nyata |
| Background berwarna (matting lokal) | ✅ terbukti (ΔE 2,66; sisa 10 px) |
| PNG transparan (547 rb piksel semi → 0) | ✅ terbukti (ΔE 3,02) |
| Kaos berwarna (merah, navy) | ✅ terbukti dan disetel: 2,44 · 2,48 (§9b) |
| Filter artefak JPEG | ⏳ M2 |

**Rekomendasi default:** AM 35 LPI rosette (tampilan referensi) + pembersihan
titik minimum + hybrid highlight. Tepi tajam sebagai opsi (preset "Tajam").
Difusi untuk foto. Grain sinkron untuk gaya vintage. Urutan pola di M1: AM,
difusi, FM, Index, grain. Garis dan Bayer di M2.

**Langkah terpenting berikutnya:** semua bukti di atas masih **di layar**. Sebelum
setelan default dikunci, *test sheet* (§8 butir 7) harus dicetak di printer DTF dan
dipres ke kaos hitam dan putih, lalu dicuci beberapa kali.

## 9b. Uji menyeluruh: setiap LPI × setiap bentuk (prototipe v1 → v5)

**Cara uji.** Ada 224 kombinasi: 7 bentuk × 13 LPI (15–85) × kaos hitam/putih di
300 DPI, ditambah LPI 55/65/85 di 600 DPI. Bahan uji:
- **Color chart 60 warna**: abu-abu 0–100%, primer dan sekunder 25–100%, warna
  kulit, pastel, warna gelap, warna menengah. Ukuran patch 2 cm.
- **Gradasi 0–100%** per channel.

Yang diukur: ΔE tiap patch (rata-rata area, setara dilihat dari jarak), galat
nada, nada pertama yang tercetak, nada yang jadi solid, semi-transparan, dan titik
< minimum.

**Masalah yang ditemukan dan diperbaiki:**

| # | Masalah (v1) | Penyebab | Perbaikan |
|---|---|---|---|
| 1 | **55 LPI anjlok** di semua bentuk (ΔE 6–12, patch terburuk 49) | Pada grid 5:2 semua sel identik dan banyak piksel punya ambang **kembar**, jadi nada melompat ±14% | **Tile ambang ber-ranking**: setiap kelas posisi piksel di super-cell dapat urutan unik, tanpa nilai kembar; titik diseimbangkan antar sel |
| 2 | Nada < 6% **hilang total**; pastel bergeser warna | Clip 6% per channel | Clip hanya untuk **total** tinta < 2% (noise). Nada kecil dicetak lewat FM |
| 3 | LPI ≥ 60 @300 DPI: nada pertama 8–22%, solid sudah di 78–92% | Titik/lubang lebih kecil dari minimum DTF, lalu dibuang/ditambal | **Hybrid AM/FM** dengan titik FM berukuran minimum di luar rentang aman |
| 4 | Kotak, Silang, Garis: galat nada 5–24%, patch terburuk 11–20 | Bentuk ini membuat garis 1 px/celah sempit di nada tertentu | **Rentang aman per bentuk** diukur otomatis (contoh 35 LPI: Bulat 4–94%, Kotak 4–76%, Garis 14–86%, Silang 24–94%). Di luar rentang dipakai FM |
| 5 | Sisa kurva nada tidak linear | Bentuk + hybrid | **Kalibrasi otomatis** kurva nada per bentuk × LPI × DPI, diukur pada screen tanpa pembersihan. Satu kurva berlaku untuk abu-abu dan warna |
| 6 | Grid kasar di LPI tinggi (mis. 55 LPI hanya 29 tingkat) | Pemilihan grid rasional | Grid dipilih dengan skor galat + 1/jumlah tingkat (n ≤ 6, tile ≤ 1600 px) → 55 LPI: 1.069 tingkat |
| 7 | Kaos putih LPI ≥ 55: galat nada abu-abu 3–10% | Celah putih kecil di antara 3 screen bersudut beda ikut ditambal | **LPI ≥ 55 di kaos putih → dot-on-dot otomatis** (galat ≤ 0,9%). Di bawah 55 tetap rosette seperti referensi |
| 8 | Batas titik minimum tetap dalam px | — | Batas dalam **mm** (0,15 mm) dikonversi sesuai DPI |

**Hasil ringkas (300 DPI, 196 kombinasi):**

| | v1 | **v5** |
|---|---|---|
| ΔE rata-rata semua kombinasi | 2,17 | **0,37** |
| ΔE rata-rata kombinasi terburuk | 11,82 | **0,97** |
| Patch terburuk | 49,2 | **6,8** (Kotak 85 LPI, kaos putih) |
| Kombinasi dengan patch > 5 | 151 | **3** (semuanya 85 LPI) |
| Galat nada maks | 24,4% | **2,9%** |
| Nada pertama yang tercetak | ≤ 24,7% | **≤ 2,6%** |
| Jadi solid paling awal | 77,9% | **97,7%** |
| Semi-transparan / titik < minimum | 0 / 0 | **0 / 0** |

**ΔE rata-rata 60 warna, kaos hitam, 300 DPI: v1 → v5**

| Bentuk | 15 | 20 | 25 | 30 | 35 | 40 | 45 | 50 | 55 | 60 | 65 | 75 | 85 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Bulat (Euclidean) | 0,1→0,53 | 0,2→0,20 | 0,4→0,33 | 0,8→0,16 | 0,5→0,20 | 0,6→0,20 | 0,7→0,16 | 1,0→0,12 | 8,5→0,23 | 1,7→0,21 | 2,7→0,30 | 1,8→0,48 | 4,0→0,67 |
| Bulat murni | 0,2→0,65 | 0,6→0,37 | 0,8→0,45 | 1,1→0,27 | 0,9→0,33 | 1,0→0,31 | 0,8→0,28 | 1,7→0,32 | 8,0→0,37 | 2,3→0,33 | 3,1→0,38 | 2,6→0,51 | 5,1→0,67 |
| Elips | 0,1→0,39 | 0,4→0,26 | 0,4→0,24 | 0,7→0,19 | 0,5→0,15 | 0,7→0,26 | 0,8→0,23 | 1,1→0,24 | 6,4→0,39 | 1,6→0,35 | 2,0→0,35 | 2,3→0,55 | 6,6→0,58 |
| Kotak | 0,5→0,50 | 0,6→0,21 | 0,8→0,27 | 1,1→0,25 | 1,2→0,27 | 1,7→0,28 | 2,7→0,34 | 2,8→0,39 | 7,5→0,42 | 4,4→0,48 | 4,2→0,54 | 4,7→0,66 | 5,8→0,97 |
| Berlian | 0,1→0,52 | 0,1→0,20 | 0,4→0,27 | 0,6→0,18 | 0,4→0,14 | 0,5→0,20 | 0,7→0,19 | 0,6→0,13 | 7,1→0,23 | 1,3→0,18 | 2,1→0,26 | 1,7→0,48 | 3,1→0,68 |
| Garis | 0,4→0,57 | 0,4→0,13 | 0,4→0,35 | 0,5→0,14 | 0,6→0,29 | 0,8→0,22 | 1,2→0,26 | 1,2→0,31 | 3,5→0,33 | 2,2→0,37 | 2,2→0,41 | 4,7→0,45 | 8,9→0,60 |
| Silang | 0,3→0,39 | 0,4→0,19 | 0,3→0,21 | 0,7→0,27 | 0,8→0,30 | 1,4→0,30 | 2,3→0,29 | 3,3→0,29 | 11,8→0,33 | 5,1→0,41 | 4,9→0,50 | 5,3→0,59 | 7,2→0,70 |

**ΔE rata-rata 60 warna, kaos putih, 300 DPI: v1 → v5**

| Bentuk | 15 | 20 | 25 | 30 | 35 | 40 | 45 | 50 | 55 | 60 | 65 | 75 | 85 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Bulat (Euclidean) | 0,6→0,27 | 0,6→0,18 | 0,8→0,22 | 1,2→0,19 | 0,9→0,24 | 0,9→0,29 | 1,1→0,34 | 1,2→0,42 | 8,4→0,28 | 1,9→0,25 | 3,0→0,39 | 2,4→0,54 | 4,4→0,68 |
| Bulat murni | 0,6→0,31 | 1,0→0,22 | 1,0→0,26 | 1,4→0,25 | 1,1→0,26 | 1,2→0,39 | 1,3→0,43 | 1,8→0,53 | 8,1→0,29 | 2,7→0,32 | 3,9→0,43 | 3,0→0,57 | 4,4→0,63 |
| Elips | 0,6→0,23 | 0,7→0,22 | 0,7→0,29 | 1,0→0,20 | 0,8→0,27 | 0,9→0,36 | 1,1→0,41 | 1,4→0,42 | 5,7→0,49 | 1,7→0,46 | 2,2→0,45 | 2,4→0,52 | 4,2→0,59 |
| Kotak | 0,7→0,29 | 0,8→0,23 | 0,9→0,29 | 1,2→0,31 | 1,3→0,37 | 1,5→0,32 | 1,9→0,36 | 1,9→0,44 | 6,8→0,41 | 2,9→0,43 | 3,4→0,43 | 3,7→0,54 | 4,2→0,81 |
| Berlian | 0,5→0,27 | 0,5→0,17 | 0,8→0,20 | 0,8→0,19 | 0,8→0,27 | 0,9→0,30 | 1,0→0,39 | 1,1→0,47 | 7,3→0,29 | 1,7→0,23 | 2,4→0,35 | 2,1→0,55 | 3,5→0,66 |
| Garis | 0,5→0,47 | 0,5→0,14 | 0,6→0,27 | 0,8→0,20 | 0,9→0,30 | 1,1→0,35 | 1,4→0,46 | 1,6→0,55 | 3,6→0,43 | 2,6→0,50 | 2,8→0,50 | 3,5→0,53 | 4,2→0,57 |
| Silang | 0,6→0,26 | 0,8→0,18 | 0,9→0,25 | 1,2→0,35 | 1,3→0,50 | 1,4→0,50 | 1,8→0,57 | 2,0→0,64 | 7,5→0,35 | 2,9→0,43 | 3,4→0,59 | 3,4→0,76 | 4,8→0,81 |


600 DPI (v5) untuk LPI 55–85: ΔE 0,17–0,94, galat nada ≤ 1,7%. Sejak v5, 600 DPI
tidak lagi diperlukan untuk akurasi warna. Tetap disarankan untuk LPI ≥ 65 kalau
ingin bentuk titik lebih halus.

**Kasus nyata dengan engine v5** (ΔE dari jarak pandang, σ = 5 px. Angka ini
sudah termasuk tekstur titik yang memang terlihat, jadi tidak mungkin mendekati
0 di 35 LPI):

| Kasus | Sebelumnya | **v5** |
|---|---|---|
| Run to the Hills, kaos hitam (referensi: 6,05) | 2,00 | **1,41** |
| Powerslave, kaos putih CMY | 3,03 | **2,59** |
| Powerslave, kaos putih CMYK GCR 100% | — | **2,50** |
| Background teal dihapus → kaos putih | 3,12 (sisa 871 px) | **2,66** (sisa 10 px, semua di tepi halus) |
| PNG transparan + bayangan → kaos putih | 3,02 | **2,53** (GCR: 2,45) |
| Kaos merah | 3,18 | **2,44** |
| Kaos navy | 4,64 | **2,48** |

Semua: semi-transparan 0, warna tidak murni 0, titik < minimum 0.

**Perubahan lain hasil uji ini:**
- **Kaos berwarna:** screen opasitas sekarang **FM** (tidak bertabrakan dengan
  screen warna AM), dan knockout warna kaos **default mati** (opsi "hemat tinta"
  0,08). Knockout 0,22 yang lama justru penyebab utama galat navy.
- **Background berwarna:** kantong warna background di dalam desain dihapus mulai
  **4 px** (sebelumnya 20). Sisa 901 → 10 px.
- **Channel K:** GCR 50% justru memperburuk (pola campur). **GCR 100%** membuat
  area netral kembali tepat (chroma 2,25 → 1,56; asli 1,55) dan bayangan murni
  titik hitam. **Tapi tampilannya berubah** (krem/abu-abu muda jadi berbintik
  hitam). Jadi tetap opsi: "Mati" untuk preset referensi, "GCR 100%" untuk desain
  netral atau foto.
- **Grid "Rapi" vs "Presisi":** di 35 LPI grid Rapi 8:3 (sama dengan referensi)
  sedikit lebih baik di desain nyata (RTH 1,62 vs 1,77). Rapi jadi default
  preset referensi, Presisi untuk LPI lain.

**Sisa yang belum sempurna:**
- 85 LPI dengan Kotak/Silang masih punya patch terburuk 5–7.
- Kaos navy: tepi area yang dihapus terlihat berkotak (FM 2×2) saat di-zoom.
- Semua angka masih di layar. Uji cetak nyata tetap wajib (§8 butir 7).

**Prototipe acuan:** semua algoritma v5 ada di [`prototype/`](../prototype/)
(`studiozea_ref.py` + command line + skrip uji). Hasilnya terverifikasi identik
piksel per piksel dengan hasil uji di atas. Port Rust di M1 dicek terhadap modul
ini.

## 9c. Uji desain RTH di background kuning, marun dan navy (kaos warna sama)

Tiga desain Run to the Hills (JPEG 896×1200) dengan background kuning (gradasi
ke hitam di bawah), marun dan navy. Dicetak di kaos berwarna sama dengan
background, 30 cm @ 300 DPI, 35 LPI bulat, hapus background otomatis. Tanda
generator di pojok kanan bawah ditutup dulu dengan warna sekitarnya.
ΔE dihitung terhadap desain asli (yang memang menunjukkan tampilan di kaos itu).

**Masalah yang ditemukan:** di navy dan marun, garis luar hitam dan bayangan
gelap yang menyentuh background ikut dianggap campuran background (proyeksi ke
F − bg negatif → α = 0), jadi di cetakan tembus warna kaos. Perbaikan: alpha
minimum (§3.2). Rekonstruksi sebelum halftone (komposit di atas bg vs asli):

| Desain | ΔE lama (rata-rata / p95 / p99,5) | ΔE baru |
|---|---|---|
| Kuning | 0,35 / 1,07 / 7,59 | **0,23 / 0,95 / 2,52** |
| Marun | 1,20 / 4,13 / 22,96 | **0,52 / 1,70 / 2,81** |
| Navy | 2,83 / 14,75 / 26,03 | **0,52 / 2,35 / 4,89** |
| Powerslave (bg putih) | 0,39 | 0,38 (isi hilang 0,07%, tidak berubah) |
| Teal (uji lama) | 0,10 | 0,01 |

Hasil cetak penuh (ΔE σ = 5 px, rata-rata / p95; semua cek lolos: 0 semi
transparan, 0 warna tidak murni, 0 titik kecil):

| Desain | Lama | **Baru** | + tepi tajam | + K GCR | knockout 0,15 |
|---|---|---|---|---|---|
| Kuning | 1,52 / 4,61 | **1,45 / 4,36** | 1,51 / 4,64 | 1,82 / 5,45 | 1,56 / 4,85 |
| Marun | 2,22 / 6,58 | **1,75 / 4,63** | 1,78 / 4,76 | 2,05 / 5,55 | 1,80 / 4,84 |
| Navy | 3,79 / 15,35 | **2,21 / 6,63** | 2,26 / 6,83 | 2,28 / 6,37 | 2,26 / 6,76 |

Varian lain (lama, sebelum perbaikan; urutannya sama): 45 LPI dan 55 LPI sedikit
lebih buruk dari 35 LPI, FM jauh lebih buruk (kuning 2,75, marun 3,95).

Kesimpulan:
- Default (35 LPI bulat, K mati, knockout mati) adalah yang terbaik untuk ketiga
  warna. Tepi tajam dan GCR tidak membantu desain ini.
- Gradasi kuning → hitam di bawah tercetak sebagai tinta hitam yang memudar
  (benar); area kuning yang sama dengan kaos tidak dicetak.
- Knockout hanya menghemat 1–2% tinta di sini (background sudah terbuang).
- Sisa galat tersebar rata (noise halftone), bukan di area tertentu.
- Sumber 896 px untuk 30 cm = ±76 DPI efektif: detail lembut. Untuk hasil
  maksimal pakai file sumber minimal ±150 DPI di ukuran cetak.

## 9d. Aplikasi ringan dan perbaikan bug (uji di server)

**Keluhan:** versi portable crash di 600 DPI, background tertentu tidak terhapus otomatis,
kanvas tidak transparan seperti Photoshop, zoom kurang nyaman.

**Penyebab crash (diukur):** render lama memegang seluruh gambar sekaligus dalam float:
68–110 byte per piksel cetak. 30×40 cm @600 DPI = 67 Mpx → 4,6 GB (kaos hitam),
5,1 GB (putih/berwarna), 7,4 GB dengan tepi tajam. Aplikasi menambah pratinjau ukuran
penuh dan menyimpan hasil lama, dan dua proses bisa jalan bersamaan (8,9–9,5 GB). Di PC
8 GB alokasi gagal → Rust menghentikan aplikasi tanpa pesan.

**Perbaikan: render per band** (`engine/src/band.rs`).
- Gambar siap cetak disimpan di resolusi sumber; resize Lanczos dihitung per jendela
  dengan aritmetika yang sama persis (`Resizer`).
- Setiap band/jendela dirender dengan halo (`halo_px`: filter nada hybrid, blok FM,
  ring tepi tajam, lalu bersih-bersih pulau/lubang). Ambang AM, derau dan blok FM memakai
  koordinat absolut (`render_at`).
- Hasil disimpan 4 bit per piksel (R, G, B, A masing-masing 0/255): 34 MB untuk 67 Mpx.
  Cek "titik kecil" dihitung per band dengan margin, ekspor PNG di-stream dari buffer itu.
- Bukti: 36 kasus golden + band 40 baris + jendela acak identik dengan render penuh
  (`tests/band.rs`); desain navy 30×40 cm @600 DPI: PNG byte-identik dengan render penuh
  untuk kaos hitam, putih dan navy.

| 30×40 cm @600 DPI | Lama | Baru |
|---|---|---|
| RAM puncak command line | 4,3–4,4 GB | **0,48–0,59 GB** |
| Waktu command line | 18,7–20,5 s | **11,7–18,0 s** |
| RAM puncak aplikasi | 4,8–5,7 GB (diukur), sampai 9,5 GB | **0,87 GB** |

**Viewer baru** (`app/src/CanvasViewer.jsx`): satu canvas; roda = zoom ke arah kursor
(titik di bawah kursor tetap, diuji di Chromium), tarik/Spasi/tombol tengah = geser,
Ctrl 0/1/+/−, klik ganda = 100 %, latar kotak-kotak transparan (atau warna kaos), split,
crop. Piksel diambil per tile 512 px dengan level mip (rata-rata kotak) dari buffer hasil;
di ≥ 100 % piksel digambar tanpa smoothing (165/165 sampel layar = piksel cetak).
Render jalan di thread latar dengan progres; band yang terlihat dirender lebih dulu;
pengaturan baru membatalkan render lama; ekspor hanya untuk hasil terbaru yang selesai.

**Background otomatis:** dulu hanya kaos berwarna. Sekarang di semua kaos, bila tepi
gambar rata (≥ 50 % piksel tepi dalam jarak 0,10 dari median) dan warnanya bukan "warna
tanpa tinta" kaos itu (hitam di kaos hitam, putih di kaos putih). Uji: navy/marun/kuning
di kaos hitam dan putih, Powerslave (putih) di kaos hitam, RTH (hitam) di kaos putih:
tepi hasil tanpa tinta (kecuali gradasi hitam desain kuning di kaos putih, yang memang
bagian desain).

**Bug lain yang ditemukan dan diperbaiki:** DPI 150/200 panic (ramp kalibrasi kosong);
LPI < 10 membuat tile ambang sangat besar (LPI 1 → crash) → aplikasi membatasi LPI 10–150
dan DPI 100–720; kunci matting background meluap untuk sumber > 33,5 Mpx (indeks 25 bit
→ 32 bit, Python juga); file 16-bit/besar ditolak decoder (batas 512 MiB → 6 GiB, pesan
lengkap); hasil lama tetap bisa diekspor setelah error/saat pengaturan berubah; klik label
baris mengubah pilihan; kolom tinggi tidak bisa diketik; setiap jeda mengetik memicu
render (sekarang diterapkan saat Enter/keluar kolom); cache prepare memakai warna kaos
(sekarang jenis kaos + identitas gambar); cache tile ambang tidak pernah dikosongkan;
`--crop` di command line menolak `x0,y0,x1,y1`.

## 10. Pertanyaan terbuka

1. ~~Teknologi~~: **Tauri + Rust** (disetujui).
2. ~~Mode kaos putih CMY atau CMYK~~: K sebagai pilihan (Mati / Garis saja /
   GCR). Default Mati untuk preset referensi (§4.9).
3. ~~Kaos berwarna~~: mode tersendiri (screen opasitas + CMY + knockout warna
   kaos), sudah diuji (§3.7b). Masuk M1.
